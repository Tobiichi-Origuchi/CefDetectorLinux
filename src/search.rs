use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::models::AppInfo;

pub fn open_path(path: String, is_dir: bool) {
    if is_dir {
        let _ = Command::new("xdg-open").arg(path).spawn();
    } else {
        if let Some(p) = Path::new(&path).parent() {
            let _ = Command::new("xdg-open").arg(p).spawn();
        }
    }
}

use ignore::{WalkBuilder, WalkState};
use regex::Regex;

fn exec_search(regex_pattern: &str, is_regex: bool) -> Vec<String> {
    let re = if is_regex {
        Regex::new(regex_pattern).ok()
    } else {
        let escaped = regex::escape(regex_pattern);
        Regex::new(&escaped).ok()
    };

    let re = match re {
        Some(r) => r,
        None => return vec![],
    };

    let results = Arc::new(Mutex::new(Vec::new()));

    let mut builder = WalkBuilder::new("/");
    builder
        .hidden(false)
        .git_exclude(false)
        .filter_entry(|entry| {
            let path = entry.path();
            // Exclude special file systems
            if path.starts_with("/proc") || path.starts_with("/sys") || path.starts_with("/dev") {
                return false;
            }
            true
        });

    builder.build_parallel().run(|| {
        let results = Arc::clone(&results);
        let re = re.clone();
        Box::new(move |result| {
            if let Ok(entry) = result
                && let Some(file_type) = entry.file_type()
                && file_type.is_file()
            {
                let name = entry.file_name().to_string_lossy();
                if re.is_match(&name) {
                    let mut res = results.lock().unwrap();
                    res.push(entry.path().to_string_lossy().to_string());
                }
            }
            WalkState::Continue
        })
    });

    let mut res = results.lock().unwrap();
    let mut final_res = Vec::new();
    std::mem::swap(&mut *res, &mut final_res);
    final_res
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

fn get_running_processes() -> HashSet<String> {
    let mut procs = HashSet::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();
            if file_name_str.chars().all(char::is_numeric) {
                let exe_path = entry.path().join("exe");
                if let Ok(link) = fs::read_link(&exe_path) {
                    procs.insert(link.to_string_lossy().into_owned());
                }
            }
        }
    }
    procs
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(haystack, needle).is_some()
}

pub fn core_search<F>(mut on_found: F)
where
    F: FnMut(AppInfo),
{
    let running_procs = get_running_processes();
    let mut found_files = HashSet::new();
    let mut visited_dirs = HashSet::new();
    let mut global_ino_cache = HashSet::new();

    let mut add_app = |file: &Path, app_type: &str, is_dir: bool| {
        let file_str = file.to_string_lossy().to_string();
        if !found_files.insert(file_str.clone()) {
            return;
        }
        let target_dir = if is_dir {
            file.to_path_buf()
        } else {
            file.parent().unwrap().to_path_buf()
        };
        let size = dir_size(&target_dir, &mut global_ino_cache, 0);

        let is_running = if !is_dir {
            running_procs.contains(&file_str)
        } else {
            false
        };

        on_found(AppInfo {
            file: file_str,
            app_type: app_type.to_string(),
            size,
            is_running,
            is_dir,
            icon: "".to_string(),
        });
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
                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if !meta.is_file() {
                    continue;
                }

                // On Linux, executable files and .so files are relevant
                let is_so = path.extension().is_some_and(|e| e == "so");
                let is_exec = meta.permissions().mode() & 0o111 != 0;

                if !is_so && !is_exec {
                    continue;
                }

                let file = match fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => continue,
                };

                let mmap = unsafe { memmap2::MmapOptions::new().map(&file) };
                if let Ok(data) = mmap {
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
            if file.contains("/.Trash")
                || file.contains("/Trash/")
                || file.to_lowercase().ends_with(".log")
            {
                continue;
            }
            let path = Path::new(&file);
            let dir = if let Some(p) = path.parent() {
                p
            } else {
                continue;
            };

            if !visited_dirs.insert(dir.to_string_lossy().to_string()) {
                continue;
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
        if file.contains("/.Trash") || file.contains("/Trash/") {
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
                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if !meta.is_file() {
                    continue;
                }
                // check executables or .so
                let is_exec = meta.permissions().mode() & 0o111 != 0;
                let is_so = path.extension().is_some_and(|e| e == "so");

                if (is_exec || is_so)
                    && let Ok(file_handle) = fs::File::open(&path)
                {
                    let mmap = unsafe { memmap2::MmapOptions::new().map(&file_handle) };
                    if let Ok(data) = mmap {
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
    }
}

// pub fn start_search(app: AppHandle) { ... }
