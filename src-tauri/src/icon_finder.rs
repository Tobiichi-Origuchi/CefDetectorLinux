use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn find_icon_via_package_manager(exe_path: &Path) -> Option<PathBuf> {
    let path_str = exe_path.to_string_lossy();

    // Try dpkg first
    if let Ok(output) = Command::new("dpkg").arg("-S").arg(&*path_str).output()
        && output.status.success()
    {
        let out_str = String::from_utf8_lossy(&output.stdout);
        if let Some(pkg) = out_str.split(':').next()
            && let Ok(list_out) = Command::new("dpkg").arg("-L").arg(pkg).output()
        {
            let files = String::from_utf8_lossy(&list_out.stdout);
            let mut best_icon = None;
            for line in files.lines() {
                if (line.contains("/icons/") || line.contains("/pixmaps/"))
                    && (line.ends_with(".png") || line.ends_with(".svg"))
                {
                    best_icon = Some(PathBuf::from(line));
                    if line.contains("256x256")
                        || line.contains("512x512")
                        || line.ends_with(".svg")
                    {
                        break;
                    }
                }
            }
            if best_icon.is_some() {
                return best_icon;
            }
        }
    }

    // Try rpm
    if let Ok(output) = Command::new("rpm").arg("-qf").arg(&*path_str).output()
        && output.status.success()
    {
        let pkg = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Ok(list_out) = Command::new("rpm").arg("-ql").arg(&pkg).output() {
            let files = String::from_utf8_lossy(&list_out.stdout);
            let mut best_icon = None;
            for line in files.lines() {
                if (line.contains("/icons/") || line.contains("/pixmaps/"))
                    && (line.ends_with(".png") || line.ends_with(".svg"))
                {
                    best_icon = Some(PathBuf::from(line));
                    if line.contains("256x256")
                        || line.contains("512x512")
                        || line.ends_with(".svg")
                    {
                        break;
                    }
                }
            }
            if best_icon.is_some() {
                return best_icon;
            }
        }
    }
    None
}

pub fn find_icon_via_pe(exe_path: &Path) -> Option<String> {
    let path_str = exe_path.to_string_lossy();
    if !path_str.to_lowercase().ends_with(".exe") {
        return None;
    }

    let map = pelite::FileMap::open(exe_path).ok()?;
    let pe = pelite::PeFile::from_bytes(&map).ok()?;
    let resources = pe.resources().ok()?;

    // Iterate over group icons, extract the first valid one
    for (_name, group) in resources.icons().filter_map(Result::ok) {
        let mut bytes = Vec::new();
        if group.write(&mut bytes).is_ok() {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Some(format!("data:image/x-icon;base64,{}", encoded));
        }
    }
    None
}

pub fn find_icon_via_appimage(exe_path: &Path) -> Option<String> {
    let path_str = exe_path.to_string_lossy();
    if !path_str.to_lowercase().ends_with(".appimage") {
        return None;
    }

    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp_dir = std::env::temp_dir().join(format!("cef_extract_{}", ts));
    let _ = fs::create_dir_all(&tmp_dir);

    if let Ok(output) = Command::new(&*path_str)
        .arg("--appimage-extract")
        .arg(".DirIcon")
        .current_dir(&tmp_dir)
        .output()
        && output.status.success()
    {
        let extracted_icon = tmp_dir.join("squashfs-root").join(".DirIcon");
        let encoded = encode_file_to_base64(&extracted_icon)
            .map(|b64| format!("data:image/png;base64,{}", b64));
        let _ = fs::remove_dir_all(&tmp_dir);
        return encoded;
    }
    let _ = fs::remove_dir_all(&tmp_dir);
    None
}

#[tauri::command]
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

    if let Some(p) = find_icon_via_package_manager(exe_path) {
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

    "".to_string()
}
