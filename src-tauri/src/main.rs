#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod package_manager;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter};

#[derive(Clone, serde::Serialize)]
struct AppInfo {
    file: String,
    app_type: String,
    size: u64,
    is_running: bool,
    is_dir: bool,
    icon: String, // empty string since Linux doesn't natively provide an easy getFileIcon
}

mod icon_finder;
use icon_finder::get_app_icon;

#[tauri::command]
fn open_path(path: String, is_dir: bool) {
    if is_dir {
        let _ = Command::new("xdg-open").arg(path).spawn();
    } else {
        if let Some(p) = Path::new(&path).parent() {
            let _ = Command::new("xdg-open").arg(p).spawn();
        }
    }
}

fn exec_search(regex_pattern: &str, is_regex: bool) -> Vec<String> {
    // fd
    let fd_res = if is_regex {
        Command::new("fd")
            .arg("-t")
            .arg("f")
            .arg(regex_pattern)
            .arg("/")
            .arg("-H")
            .arg("-E")
            .arg("/proc")
            .arg("-E")
            .arg("/sys")
            .arg("-E")
            .arg("/dev")
            .output()
    } else {
        Command::new("fd")
            .arg("-t")
            .arg("f")
            .arg("-F")
            .arg(regex_pattern)
            .arg("/")
            .arg("-H")
            .arg("-E")
            .arg("/proc")
            .arg("-E")
            .arg("/sys")
            .arg("-E")
            .arg("/dev")
            .output()
    };

    if let Ok(output) = fd_res
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return stdout.lines().map(|s| s.to_string()).collect();
    }

    // fallback find
    let mut find_cmd = Command::new("find");
    find_cmd
        .arg("/")
        .arg("-path")
        .arg("/proc")
        .arg("-prune")
        .arg("-o")
        .arg("-path")
        .arg("/sys")
        .arg("-prune")
        .arg("-o")
        .arg("-path")
        .arg("/dev")
        .arg("-prune")
        .arg("-o")
        .arg("-type")
        .arg("f");
    if is_regex {
        find_cmd.arg("-regex").arg(format!(".*{}.*", regex_pattern));
    } else {
        find_cmd.arg("-name").arg(format!("*{}*", regex_pattern));
    }
    find_cmd.arg("-print");

    if let Ok(output) = find_cmd.output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return stdout.lines().map(|s| s.to_string()).collect();
    }
    vec![]
}

fn dir_size(dir: &Path, cache: &mut HashSet<u64>, deep: u32) -> u64 {
    if deep > 10 {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let ino = meta.ino();
                if cache.insert(ino) {
                    total += meta.size();
                    if meta.is_dir() {
                        total += dir_size(&entry.path(), cache, deep + 1);
                    }
                }
            }
        }
    }
    total
}

fn get_running_processes() -> HashMap<String, bool> {
    let mut procs = HashMap::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();
            if file_name_str.chars().all(char::is_numeric) {
                let exe_path = entry.path().join("exe");
                if let Ok(link) = fs::read_link(&exe_path) {
                    procs.insert(link.to_string_lossy().to_string(), true);
                }
            }
        }
    }
    procs
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tauri::command]
fn start_search(app: AppHandle) {
    thread::spawn(move || {
        let running_procs = get_running_processes();
        let cache = Arc::new(Mutex::new(HashSet::new()));
        let dir_cache2 = Arc::new(Mutex::new(HashSet::new()));
        let mut global_ino_cache = HashSet::new();

        let mut add_app = |file: &Path, app_type: &str, is_dir: bool| {
            let file_str = file.to_string_lossy().to_string();
            {
                let mut c = cache.lock().unwrap();
                if !c.insert(file_str.clone()) {
                    return;
                }
            }
            let target_dir = if is_dir {
                file.to_path_buf()
            } else {
                file.parent().unwrap().to_path_buf()
            };
            let size = dir_size(&target_dir, &mut global_ino_cache, 0);

            let is_running = if !is_dir {
                running_procs.contains_key(&file_str)
            } else {
                false
            };

            let _ = app.emit(
                "app-found",
                AppInfo {
                    file: file_str,
                    app_type: app_type.to_string(),
                    size,
                    is_running,
                    is_dir,
                    icon: "".to_string(),
                },
            );
        };

        let search_dir = |dir: &Path| -> (bool, Option<PathBuf>, Option<String>) {
            let msedge = dir.join("msedge");
            if msedge.exists() {
                return (true, Some(msedge), Some("Edge".to_string()));
            }
            // Check Linux Chrome equivalents
            let chrome = dir.join("chrome");
            if chrome.exists() {
                return (true, Some(chrome), Some("Chrome".to_string()));
            }

            let mut first_exe = None;
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let meta = entry.metadata();
                    if meta.is_err() || !meta.unwrap().is_file() {
                        continue;
                    }

                    // On Linux, executable files and .so files are relevant
                    let is_so = path.extension().is_some_and(|e| e == "so");
                    use std::os::unix::fs::PermissionsExt;
                    let is_exec = entry.metadata().unwrap().permissions().mode() & 0o111 != 0;

                    if !is_so && !is_exec {
                        continue;
                    }

                    if let Ok(data) = fs::read(&path) {
                        let typ = if contains_bytes(&data, b"third_party/electron_node")
                            || contains_bytes(&data, b"register_atom_browser_web_contents")
                        {
                            "Electron"
                        } else if contains_bytes(&data, b"url-nwjs") {
                            "NWJS"
                        } else if contains_bytes(&data, b"CefSharp.Internals") {
                            "CefSharp"
                        } else if contains_bytes(&data, b"cef_string_utf8_to_utf16") {
                            "CEF"
                        } else {
                            if first_exe.is_none() {
                                let fname = path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_lowercase();
                                if !fname.contains("unins")
                                    && !fname.contains("setup")
                                    && !fname.contains("report")
                                    && is_exec
                                {
                                    first_exe = Some(path.clone());
                                }
                            }
                            continue;
                        };

                        return (true, Some(path), Some(typ.to_string()));
                    }
                }
            }
            (false, first_exe, None)
        };

        let mut search_cef = |stdout: Vec<String>, default_type: &str| {
            for file in stdout {
                if file.contains("Trash") || file.to_lowercase().ends_with(".log") {
                    continue;
                }
                let path = Path::new(&file);
                let dir = if let Some(p) = path.parent() {
                    p
                } else {
                    continue;
                };

                {
                    let mut c2 = dir_cache2.lock().unwrap();
                    if !c2.insert(dir.to_string_lossy().to_string()) {
                        continue;
                    }
                }

                if path.is_dir() {
                    continue;
                }

                let (found, exe, typ) = search_dir(dir);
                if found {
                    add_app(&exe.unwrap(), &typ.unwrap(), false);
                    continue;
                }
                if let Some(e) = exe {
                    add_app(&e, default_type, false);
                } else {
                    let parent_dir = if let Some(p) = dir.parent() {
                        p
                    } else {
                        continue;
                    };
                    let (found2, exe2, typ2) = search_dir(parent_dir);
                    if found2 {
                        add_app(&exe2.unwrap(), &typ2.unwrap(), false);
                        continue;
                    }
                    if let Some(e) = exe2 {
                        add_app(&e, default_type, false);
                    } else {
                        add_app(dir, default_type, true);
                    }
                }
            }
        };

        search_cef(exec_search("_100_(.+?)\\.pak$", true), "Unknown");
        search_cef(exec_search("libcef", false), "CEF");

        let node_dlls = exec_search("libnode.*?\\.so", true);
        for file in node_dlls {
            if file.contains("Trash") {
                continue;
            }
            let path = Path::new(&file);
            if path.is_dir() {
                continue;
            }
            let dir = if let Some(p) = path.parent() {
                p
            } else {
                continue;
            };

            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let meta = entry.metadata();
                    if meta.is_err() || !meta.unwrap().is_file() {
                        continue;
                    }
                    // check executables or .so
                    use std::os::unix::fs::PermissionsExt;
                    let is_exec = entry.metadata().unwrap().permissions().mode() & 0o111 != 0;
                    let is_so = path.extension().is_some_and(|e| e == "so");

                    if (is_exec || is_so)
                        && let Ok(data) = fs::read(&path)
                    {
                        let typ = if contains_bytes(&data, b"napi_create_buffer") {
                            "Mini Electron"
                        } else if contains_bytes(&data, b"miniblink") {
                            "Mini Blink"
                        } else {
                            continue;
                        };

                        add_app(&path, typ, false);
                        break;
                    }
                }
            }
        }

        let _ = app.emit("search-done", ());
    });
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_app_icon,
            start_search,
            open_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
