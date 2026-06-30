use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::models::AppInfo;

// ---- fast filename classifier (replaces RegexSet for 3 simple patterns) ----

enum FileClass {
    Pak,
    Cef,
    Node,
    None,
}

fn classify_name(name: &str) -> FileClass {
    if name.ends_with(".pak") && name.contains("_100_") {
        FileClass::Pak
    } else if name.contains("libcef") {
        FileClass::Cef
    } else if name.starts_with("libnode") && name.ends_with(".so") {
        FileClass::Node
    } else {
        FileClass::None
    }
}

// ---- path open helper ----

pub fn open_path(path: String, is_dir: bool) {
    if is_dir {
        let _ = Command::new("xdg-open").arg(path).spawn();
    } else if let Some(p) = Path::new(&path).parent() {
        let _ = Command::new("xdg-open").arg(p).spawn();
    }
}

// ---- ignore config ----

use ignore::{WalkBuilder, WalkState};

struct IgnoreConfig {
    dir_names: HashSet<String>,
    abs_paths: HashSet<PathBuf>,
}

fn load_ignore_config() -> IgnoreConfig {
    let mut config = IgnoreConfig {
        dir_names: HashSet::new(),
        abs_paths: HashSet::new(),
    };

    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".config")
        });

    let ignore_file = config_dir.join("cefdetector").join(".ignore");
    if let Ok(content) = std::fs::read_to_string(ignore_file) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('/') {
                config.abs_paths.insert(PathBuf::from(line));
            } else {
                config.dir_names.insert(line.to_string());
            }
        }
    }

    config
}

// ---- single-pass parallel scan ----

struct ScanResults {
    pak_files: Vec<String>,
    cef_files: Vec<String>,
    node_files: Vec<String>,
}

fn single_pass_scan() -> ScanResults {
    let results = Arc::new(Mutex::new(ScanResults {
        pak_files: Vec::new(),
        cef_files: Vec::new(),
        node_files: Vec::new(),
    }));

    let ignore_config = load_ignore_config();

    let mut builder = WalkBuilder::new("/");
    builder
        .standard_filters(false)
        .hidden(false)
        .threads(
            std::thread::available_parallelism()
                .map(|n| n.get().min(8))
                .unwrap_or(4),
        )
        .filter_entry(move |entry| {
            let path = entry.path();
            if path.starts_with("/proc")
                || path.starts_with("/sys")
                || path.starts_with("/dev")
                || path.starts_with("/run")
                || path.starts_with("/tmp")
                || path.starts_with("/boot")
                || path.starts_with("/lost+found")
            {
                return false;
            }

            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                if ignore_config.abs_paths.contains(path) {
                    return false;
                }
                if let Some(name) = path.file_name()
                    && ignore_config
                        .dir_names
                        .contains(name.to_string_lossy().as_ref())
                {
                    return false;
                }
            }

            true
        });

    builder.build_parallel().run(|| {
        let results = Arc::clone(&results);

        Box::new(move |entry_result| {
            if let Ok(entry) = entry_result
                && let Some(file_type) = entry.file_type()
                && file_type.is_file()
            {
                let name = entry.file_name().to_string_lossy();
                let path_str = entry.path().to_string_lossy().into_owned();

                match classify_name(&name) {
                    FileClass::Pak => {
                        results.lock().pak_files.push(path_str);
                    }
                    FileClass::Cef => {
                        results.lock().cef_files.push(path_str);
                    }
                    FileClass::Node => {
                        results.lock().node_files.push(path_str);
                    }
                    FileClass::None => {}
                }
            }
            WalkState::Continue
        })
    });

    let mut locked = results.lock();
    ScanResults {
        pak_files: std::mem::take(&mut locked.pak_files),
        cef_files: std::mem::take(&mut locked.cef_files),
        node_files: std::mem::take(&mut locked.node_files),
    }
}

// ---- dir size (with path-cache for repeated calls on the same tree) ----

fn dir_size(
    dir: &Path,
    inode_cache: &mut HashSet<u64>,
    deep: u32,
    dir_cache: &mut HashMap<PathBuf, u64>,
) -> u64 {
    if deep > 10 {
        return 0;
    }
    if let Some(&cached) = dir_cache.get(dir) {
        return cached;
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let ino = meta.ino();
                if inode_cache.insert(ino) {
                    total += meta.size();
                    if meta.is_dir() {
                        total += dir_size(&entry.path(), inode_cache, deep + 1, dir_cache);
                    }
                }
            }
        }
    }
    dir_cache.insert(dir.to_path_buf(), total);
    total
}

// ---- running processes ----

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

// ---- fast byte-pattern search in memory-mapped files ----

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(haystack, needle).is_some()
}

/// Memory-map a file for read-only scanning.
/// Uses MAP_PRIVATE (copy-on-write) to reduce SIGBUS risk from concurrent
/// truncation — already-faulted pages survive file truncation.
fn mmap_file(file: &fs::File) -> Option<memmap2::MmapMut> {
    // SAFETY: read-only map_copy (MAP_PRIVATE). Slightly safer than MAP_SHARED
    // since pages faulted in before a concurrent truncate remain accessible.
    // SIGBUS is still possible for pages not yet faulted, but this is rare
    // for installed binaries.
    unsafe { memmap2::MmapOptions::new().map_copy(file) }.ok()
}

// ---- core search ----

pub fn core_search<F>(mut on_found: F)
where
    F: FnMut(AppInfo),
{
    let running_procs = get_running_processes();
    let mut found_files = HashSet::new();
    let mut visited_dirs = HashSet::new();
    let mut global_ino_cache = HashSet::new();
    let mut dir_size_cache = HashMap::new();

    let mut add_app = |file: &Path, app_type: &str, is_dir: bool| {
        let file_str = file.to_string_lossy().into_owned();
        if !found_files.insert(file_str.clone()) {
            return;
        }
        let target_dir = if is_dir {
            file.to_path_buf()
        } else {
            file.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| file.to_path_buf())
        };
        let size = dir_size(&target_dir, &mut global_ino_cache, 0, &mut dir_size_cache);

        let is_running = if !is_dir {
            running_procs.contains(&file_str)
        } else {
            false
        };

        on_found(AppInfo {
            file: file_str,
            app_type: app_type.to_owned(),
            size,
            is_running,
            is_dir,
        });
    };

    // ---- search a directory for CEF/Electron/NWJS signatures ----
    //
    // Returns (found_known_type, first_exe_fallback, type_name)

    let search_dir = |dir: &Path| -> (bool, Option<PathBuf>, Option<String>) {
        // Check well-known browser binary names first (no mmap needed)
        let msedge = dir.join("msedge");
        if msedge.exists() {
            return (true, Some(msedge), Some("Edge".to_string()));
        }
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

                // Executables and .so files may embed CEF signatures
                let is_so = path.extension().is_some_and(|e| e == "so");
                let is_exec = meta.permissions().mode() & 0o111 != 0;

                if !is_so && !is_exec {
                    continue;
                }

                let file = match fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => continue,
                };

                let data = match mmap_file(&file) {
                    Some(m) => m,
                    None => continue,
                };

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
                    // No signature matched — track as fallback candidate
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
                            first_exe = Some(path);
                        }
                    }
                    continue;
                };

                return (true, Some(path), Some(typ.to_string()));
            }
        }
        (false, first_exe, None)
    };

    // ---- process a list of file paths (either pak-file hits or cef-file hits) ----

    let mut process_file_hits = |files: Vec<String>, default_type: &str| {
        for file in files {
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

            if !visited_dirs.insert(dir.to_path_buf()) {
                continue;
            }

            if path.is_dir() {
                continue;
            }

            let (found, exe, typ) = search_dir(dir);
            if found {
                add_app(exe.as_ref().unwrap(), typ.as_ref().unwrap(), false);
                continue;
            }
            if let Some(ref e) = exe {
                add_app(e, default_type, false);
                continue;
            }

            // One level up
            if let Some(parent_dir) = dir.parent() {
                let (found2, exe2, typ2) = search_dir(parent_dir);
                if found2 {
                    add_app(exe2.as_ref().unwrap(), typ2.as_ref().unwrap(), false);
                    continue;
                }
                if let Some(ref e) = exe2 {
                    add_app(e, default_type, false);
                } else {
                    add_app(dir, default_type, true);
                }
            } else {
                add_app(dir, default_type, true);
            }
        }
    };

    let scan = single_pass_scan();

    process_file_hits(scan.pak_files, "Unknown");
    process_file_hits(scan.cef_files, "CEF");

    // ---- node .so files: look for Mini Electron / Mini Blink signatures ----

    for file in scan.node_files {
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
                let exe_path = entry.path();
                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if !meta.is_file() {
                    continue;
                }
                let is_exec = meta.permissions().mode() & 0o111 != 0;
                let is_so = exe_path.extension().is_some_and(|e| e == "so");

                if !is_exec && !is_so {
                    continue;
                }

                let file_handle = match fs::File::open(&exe_path) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let data = match mmap_file(&file_handle) {
                    Some(m) => m,
                    None => continue,
                };

                let typ = if contains_bytes(&data, b"napi_create_buffer") {
                    "Mini Electron"
                } else if contains_bytes(&data, b"miniblink") {
                    "Mini Blink"
                } else {
                    continue;
                };

                add_app(&exe_path, typ, false);
                break;
            }
        }
    }
}
