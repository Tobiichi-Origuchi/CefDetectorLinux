use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};

pub fn encode_file_to_base64(path: &Path) -> Option<String> {
    if let Ok(data) = fs::read(path) {
        return Some(base64::engine::general_purpose::STANDARD.encode(data));
    }
    None
}

pub fn find_icon_in_theme(icon_name: &str) -> Option<PathBuf> {
    let search_dirs = [
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
    let current_home = std::env::var("HOME").ok();

    let mut all_dirs: Vec<String> = search_dirs.iter().map(|&s| s.to_string()).collect();
    if let Some(h) = current_home {
        all_dirs.push(format!("{}/.local/share/icons/hicolor/512x512/apps", h));
        all_dirs.push(format!("{}/.local/share/icons/hicolor/256x256/apps", h));
        all_dirs.push(format!("{}/.local/share/icons/hicolor/128x128/apps", h));
        all_dirs.push(format!("{}/.local/share/icons/hicolor/scalable/apps", h));
        all_dirs.push(format!("{}/.local/share/icons", h));
    }

    for ext in ["png", "svg"] {
        for dir in &all_dirs {
            let p = Path::new(dir).join(format!("{}.{}", icon_name, ext));
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

pub fn find_neighboring_icon(exe_path: &Path) -> Option<PathBuf> {
    let parent = exe_path.parent()?;
    let exe_name = exe_path.file_name()?.to_string_lossy().to_string();

    let mut dirs_to_check = vec![parent.to_path_buf()];
    let resources_dir = parent.join("resources");
    if resources_dir.exists() {
        dirs_to_check.push(resources_dir);
    }
    let assets_dir = parent.join("assets");
    if assets_dir.exists() {
        dirs_to_check.push(assets_dir);
    }

    let possible_names = [
        format!("{}.png", exe_name),
        format!("{}.svg", exe_name),
        "icon.png".to_string(),
        "logo.png".to_string(),
        "app.png".to_string(),
    ];

    for dir in dirs_to_check {
        for name in &possible_names {
            let p = dir.join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

pub fn find_icon_via_desktop_file(exe_path: &Path) -> Option<PathBuf> {
    let exe_name = exe_path.file_name()?.to_string_lossy().to_string();

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
}

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

pub fn find_icon_via_pe(exe_path: &Path) -> Option<String> {
    let path_str = exe_path.to_string_lossy();
    if !path_str.to_lowercase().ends_with(".exe") {
        return None;
    }

    // Try the exact executable
    if let Some(bytes) = extract_pe_icon_bytes(exe_path) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Some(format!("data:image/x-icon;base64,{}", encoded));
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
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        return Some(format!("data:image/x-icon;base64,{}", encoded));
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

pub fn find_icon_via_appimage(exe_path: &Path) -> Option<String> {
    use backhand::{InnerNode, Squashfs};
    use memmap2::MmapOptions;
    use std::io::{Read, Seek, SeekFrom};

    let path_str = exe_path.to_string_lossy();
    if !path_str.to_lowercase().ends_with(".appimage") {
        return None;
    }

    let mut file = fs::File::open(exe_path).ok()?;
    let mmap = unsafe { MmapOptions::new().map(&file).ok()? };

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
            let mime = match ext.as_str() {
                "svg" => "image/svg+xml",
                _ => "image/png",
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Some(format!("data:{};base64,{}", mime, encoded));
        }
    }

    None
}


pub fn get_app_icon(path: String) -> String {
    let exe_path = Path::new(&path);

    if let Some(b64) = find_icon_via_pe(exe_path) {
        return b64;
    }

    if let Some(b64) = find_icon_via_appimage(exe_path) {
        return b64;
    }

    if let Some(p) = find_neighboring_icon(exe_path)
        && let Some(b64) = encode_file_to_base64(&p)
    {
        return format!("data:image/png;base64,{}", b64);
    }

    if let Some(p) = crate::package_manager::find_icon_via_package_manager(exe_path) {
        let ext = p.extension().unwrap_or_default().to_string_lossy();
        let mime = if ext == "svg" {
            "image/svg+xml"
        } else {
            "image/png"
        };
        if let Some(b64) = encode_file_to_base64(&p) {
            return format!("data:{};base64,{}", mime, b64);
        }
    }

    if let Some(p) = find_icon_via_desktop_file(exe_path) {
        let ext = p.extension().unwrap_or_default().to_string_lossy();
        let mime = if ext == "svg" {
            "image/svg+xml"
        } else {
            "image/png"
        };
        if let Some(b64) = encode_file_to_base64(&p) {
            return format!("data:{};base64,{}", mime, b64);
        }
    }

    include_str!("default_cef_icon.txt").to_string()
}
