use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use std::sync::LazyLock;

#[derive(Clone)]
pub enum RawIcon {
    Svg(Vec<u8>),
    PngOrIco(Vec<u8>),
    Empty,
}

static RAW_ICON_CACHE: LazyLock<Mutex<HashMap<PathBuf, RawIcon>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static DESKTOP_CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static ICON_THEME_CACHE: LazyLock<Mutex<HashMap<String, Option<PathBuf>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static NEIGHBOR_ICON_CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn read_file_bytes(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

fn try_icon_from_path(path: &Path) -> Option<RawIcon> {
    let bytes = read_file_bytes(path)?;
    let is_svg = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("svg"))
        .unwrap_or(false);
    Some(if is_svg {
        RawIcon::Svg(bytes)
    } else {
        RawIcon::PngOrIco(bytes)
    })
}

// ---- icon-theme lookup ----

pub fn find_icon_in_theme(icon_name: &str) -> Option<PathBuf> {
    {
        let cache = ICON_THEME_CACHE.lock();
        if let Some(cached) = cache.get(icon_name) {
            return cached.clone();
        }
    }

    let search_dirs: &[&str] = &[
        "/usr/share/pixmaps",
        "/usr/share/icons/hicolor/512x512/apps",
        "/usr/share/icons/hicolor/256x256/apps",
        "/usr/share/icons/hicolor/128x128/apps",
        "/usr/share/icons/hicolor/64x64/apps",
        "/usr/share/icons/hicolor/48x48/apps",
        "/usr/share/icons/hicolor/32x32/apps",
        "/usr/share/icons/hicolor/scalable/apps",
        "/usr/share/icons/Adwaita/512x512/apps",
        "/usr/share/icons/Adwaita/256x256/apps",
        "/usr/share/icons/Adwaita/scalable/apps",
    ];

    let home_dirs: Option<[String; 5]> = std::env::var("HOME").ok().map(|h| {
        [
            format!("{}/.local/share/icons/hicolor/512x512/apps", h),
            format!("{}/.local/share/icons/hicolor/256x256/apps", h),
            format!("{}/.local/share/icons/hicolor/128x128/apps", h),
            format!("{}/.local/share/icons/hicolor/scalable/apps", h),
            format!("{}/.local/share/icons", h),
        ]
    });

    let result = {
        let mut found = None;
        for ext in ["png", "svg"] {
            for dir in search_dirs {
                let p = Path::new(dir).join(format!("{}.{}", icon_name, ext));
                if p.exists() {
                    found = Some(p);
                    break;
                }
            }
            if found.is_none()
                && let Some(ref dirs) = home_dirs
            {
                for dir in dirs {
                    let p = Path::new(dir).join(format!("{}.{}", icon_name, ext));
                    if p.exists() {
                        found = Some(p);
                        break;
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }
        found
    };

    ICON_THEME_CACHE
        .lock()
        .insert(icon_name.to_owned(), result.clone());
    result
}

// ---- neighboring-icon lookup (same dir as executable) ----

pub fn find_neighboring_icon(exe_path: &Path) -> Option<PathBuf> {
    let parent = exe_path.parent()?;

    {
        let cache = NEIGHBOR_ICON_CACHE.lock();
        if let Some(cached) = cache.get(parent) {
            return cached.clone();
        }
    }

    let exe_name = exe_path.file_name()?.to_string_lossy().to_string();

    let result = (|| {
        let mut dirs_to_check = vec![parent.to_path_buf()];
        let resources_dir = parent.join("resources");
        if resources_dir.exists() {
            dirs_to_check.push(resources_dir);
        }
        let assets_dir = parent.join("assets");
        if assets_dir.exists() {
            dirs_to_check.push(assets_dir);
        }

        let name_pat = [format!("{}.png", exe_name), format!("{}.svg", exe_name)];
        let name_fixed = ["icon.png", "logo.png", "app.png"];

        for dir in dirs_to_check {
            for name in name_pat
                .iter()
                .map(|s| s.as_str())
                .chain(name_fixed.iter().copied())
            {
                let p = dir.join(name);
                if p.exists() {
                    return Some(p);
                }
            }
        }
        None
    })();

    NEIGHBOR_ICON_CACHE
        .lock()
        .insert(parent.to_path_buf(), result.clone());
    result
}

// ---- desktop-file icon lookup ----

pub fn find_icon_via_desktop_file(exe_path: &Path) -> Option<PathBuf> {
    {
        let cache = DESKTOP_CACHE.lock();
        if let Some(cached) = cache.get(exe_path) {
            return cached.clone();
        }
    }

    let exe_name = exe_path.file_name()?.to_string_lossy().to_string();

    let result = (|| {
        let mut search_dirs = vec![
            "/usr/share/applications".to_string(),
            "/var/lib/flatpak/exports/share/applications".to_string(),
        ];
        if let Ok(home) = std::env::var("HOME") {
            search_dirs.push(format!("{}/.local/share/applications", home));
        }

        for dir in search_dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "desktop")
                        && let Ok(content) = fs::read_to_string(&path)
                    {
                        let mut has_exec = false;
                        let mut icon_val = None;
                        for line in content.lines() {
                            if line.starts_with("Exec=") && line.contains(&exe_name) {
                                has_exec = true;
                            }
                            if line.starts_with("Icon=") {
                                icon_val = Some(line.trim_start_matches("Icon=").to_string());
                            }
                        }
                        if has_exec && let Some(icon) = icon_val {
                            if icon.starts_with('/') {
                                let p = PathBuf::from(icon);
                                if p.exists() {
                                    return Some(p);
                                }
                            } else if let Some(p) = find_icon_in_theme(&icon) {
                                return Some(p);
                            }
                        }
                    }
                }
            }
        }
        None
    })();

    DESKTOP_CACHE
        .lock()
        .insert(exe_path.to_path_buf(), result.clone());
    result
}

// ---- PE icon extraction (Windows .exe files on Linux, e.g. Wine/Proton apps) ----

fn assemble_valid_ico(group: pelite::resources::group::GroupResource<'_>) -> Vec<u8> {
    let mut out = Vec::new();

    // Write ICO header
    out.extend_from_slice(&0u16.to_le_bytes()); // idReserved
    out.extend_from_slice(&1u16.to_le_bytes()); // idType

    let entries = group.entries();
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes()); // idCount

    let mut image_data = Vec::new();
    let mut image_offset = 6 + entries.len() as u32 * 16;

    for entry in entries {
        let bytes = group.image(entry.nId).unwrap_or(&[]);
        let actual_size = bytes.len() as u32;

        // Write ICONDIRENTRY
        out.push(entry.bWidth);
        out.push(entry.bHeight);
        out.push(entry.bColorCount);
        out.push(entry.bReserved);
        out.extend_from_slice(&entry.wPlanes.to_le_bytes());
        out.extend_from_slice(&entry.wBitCount.to_le_bytes());
        out.extend_from_slice(&actual_size.to_le_bytes());
        out.extend_from_slice(&image_offset.to_le_bytes());

        image_data.push(bytes);
        image_offset += actual_size;
    }

    // Append all image data
    for data in image_data {
        out.extend_from_slice(data);
    }

    out
}

fn extract_pe_icon_bytes(exe_path: &Path) -> Option<Vec<u8>> {
    let map = pelite::FileMap::open(exe_path).ok()?;
    let pe = pelite::PeFile::from_bytes(&map).ok()?;
    let resources = pe.resources().ok()?;

    for (_name, group) in resources.icons().filter_map(Result::ok) {
        let bytes = assemble_valid_ico(group);
        if !bytes.is_empty() {
            return Some(bytes);
        }
    }
    None
}

pub fn find_icon_via_pe(exe_path: &Path) -> Option<RawIcon> {
    let path_str = exe_path.to_string_lossy();
    if !path_str.to_lowercase().ends_with(".exe") {
        return None;
    }

    // Try the exact executable
    if let Some(bytes) = extract_pe_icon_bytes(exe_path) {
        return Some(RawIcon::PngOrIco(bytes));
    }

    // Try sibling executables in the same directory, then parent directory
    let mut current_dir = exe_path.parent();
    for _ in 0..2 {
        if let Some(dir) = current_dir {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file()
                        && p.extension().is_some_and(|e| e == "exe")
                        && p != exe_path
                        && let Some(bytes) = extract_pe_icon_bytes(&p)
                    {
                        return Some(RawIcon::PngOrIco(bytes));
                    }
                }
            }
            current_dir = dir.parent();
        } else {
            break;
        }
    }

    None
}

// ---- AppImage icon extraction ----

pub fn find_icon_via_appimage(exe_path: &Path) -> Option<RawIcon> {
    use backhand::{InnerNode, Squashfs};
    use memmap2::MmapOptions;
    use std::io::{Read, Seek, SeekFrom};

    let path_str = exe_path.to_string_lossy();
    if !path_str.to_lowercase().ends_with(".appimage") {
        return None;
    }

    let mut file = fs::File::open(exe_path).ok()?;
    // SAFETY: read-only MAP_PRIVATE; AppImages are not expected to be modified
    // while we read them, so SIGBUS risk is negligible.
    let mmap = unsafe { MmapOptions::new().map_copy(&file) }.ok()?;

    // Find squashfs magic 'hsqs'
    let offset = memchr::memmem::find(&mmap, b"hsqs")?;

    file.seek(SeekFrom::Start(offset as u64)).ok()?;

    let mut buf_reader = std::io::BufReader::new(file);
    let squashfs = Squashfs::from_reader(&mut buf_reader).ok()?;
    let fs_reader = squashfs.into_filesystem_reader().ok()?;

    let mut target_path = PathBuf::from("/.DirIcon");
    let mut resolved_node = None;

    for _ in 0..5 {
        let node = fs_reader.files().find(|n| n.fullpath == target_path);

        if let Some(n) = node {
            match &n.inner {
                InnerNode::Symlink(sym) => {
                    let link = PathBuf::from(&sym.link);
                    if link.has_root() {
                        target_path = link;
                    } else {
                        target_path = target_path
                            .parent()
                            .unwrap_or_else(|| Path::new("/"))
                            .join(link);
                    }
                }
                InnerNode::File(_) => {
                    resolved_node = Some(n.clone());
                    break;
                }
                _ => break,
            }
        } else {
            break;
        }
    }

    if let Some(n) = resolved_node
        && let InnerNode::File(file_node) = &n.inner
    {
        let mut reader = fs_reader.file(file_node).reader();
        let mut bytes = Vec::new();
        if reader.read_to_end(&mut bytes).is_ok() {
            let ext = target_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_lowercase();
            if ext == "svg" {
                return Some(RawIcon::Svg(bytes));
            } else {
                return Some(RawIcon::PngOrIco(bytes));
            }
        }
    }

    None
}

/// Release all icon-lookup caches. Call once after scan completes.
pub fn clear_icon_caches() {
    RAW_ICON_CACHE.lock().clear();
    DESKTOP_CACHE.lock().clear();
    ICON_THEME_CACHE.lock().clear();
    NEIGHBOR_ICON_CACHE.lock().clear();
}

// ---- main entry point: find the best icon for a given executable path ----

pub fn get_app_icon(path: String) -> RawIcon {
    let exe_path = Path::new(&path);

    // Check raw-icon cache
    {
        let cache = RAW_ICON_CACHE.lock();
        if let Some(cached) = cache.get(exe_path) {
            return cached.clone();
        }
    }

    // Try PE icon extraction (.exe)
    if let Some(icon) = find_icon_via_pe(exe_path) {
        RAW_ICON_CACHE
            .lock()
            .insert(exe_path.to_path_buf(), icon.clone());
        return icon;
    }

    // Try AppImage squashfs icon
    if let Some(icon) = find_icon_via_appimage(exe_path) {
        RAW_ICON_CACHE
            .lock()
            .insert(exe_path.to_path_buf(), icon.clone());
        return icon;
    }

    // Try path-based finders (neighbor, package-manager, desktop-file)
    for path_finder in [
        |ep: &Path| find_neighboring_icon(ep),
        |ep: &Path| crate::package_manager::find_icon_via_package_manager(ep),
        |ep: &Path| find_icon_via_desktop_file(ep),
    ] {
        if let Some(p) = path_finder(exe_path)
            && let Some(icon) = try_icon_from_path(&p)
        {
            RAW_ICON_CACHE
                .lock()
                .insert(exe_path.to_path_buf(), icon.clone());
            return icon;
        }
    }

    // Fallback — not cached: static bytes, to_vec() is cheap enough per call
    RawIcon::PngOrIco(include_bytes!("../icons/default_cef_icon.ico").to_vec())
}
