#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod cli;
mod icon_finder;
pub mod models;
pub mod package_manager;
pub mod search;

use base64::Engine;
use icon_finder::get_app_icon;
use search::core_search;
use slint::{Image, ModelRc, SharedString, VecModel};
use std::rc::Rc;

fn format_size(len: u64) -> String {
    if len == 0 {
        return "0.00 B".to_string();
    }
    let sizes = ["B", "KB", "MB", "GB", "TB"];
    let mut order = 0;
    let mut val = len as f64;
    while val >= 1024.0 && order < sizes.len() - 1 {
        order += 1;
        val /= 1024.0;
    }
    format!("{:.2} {}", val, sizes[order])
}

fn load_image_from_base64(b64: &str) -> Image {
    if let Some(data) = b64.split(',').nth(1) {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
            if b64.starts_with("data:image/svg+xml") {
                return Image::load_from_svg_data(&bytes).unwrap_or_default();
            } else if let Ok(dynamic_image) = image::load_from_memory(&bytes) {
                let rgba = dynamic_image.into_rgba8();
                let buffer = slint::SharedPixelBuffer::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height());
                return Image::from_rgba8(buffer);
            }
        }
    }
    Image::default()
}

// A helper struct that is Send
#[derive(Clone)]
struct TrackedApp {
    file_str: String,
    app_type_str: String,
    size_str: String,
    is_running: bool,
    is_dir: bool,
    icon_b64: String,
    filename: String,
    size: u64,
}

fn main() -> Result<(), slint::PlatformError> {
    cli::handle_cli();

    let ui = AppWindow::new()?;
    let ui_handle = ui.as_weak();

    let apps_model = Rc::new(VecModel::default());
    ui.set_apps(ModelRc::from(apps_model.clone()));

    ui.on_open_path({
        move |path, is_dir| {
            search::open_path(path.to_string(), is_dir);
        }
    });

    ui.on_open_repo(|| {
        search::open_path("https://github.com/Tobiichi-Origuchi/CefDetectorLinux".into(), false);
    });

    let ui_handle_clone = ui_handle.clone();
    std::thread::spawn(move || {
        let mut cnt = 0;
        let mut total_size = 0;
        let mut tracked_apps: Vec<TrackedApp> = Vec::new();

        core_search(|info| {
            cnt += 1;
            total_size += info.size;

            let file_str = info.file.clone();
            let app_type_str = info.app_type.clone();
            let size = info.size;
            let is_running = info.is_running;
            let is_dir = info.is_dir;

            // Extract filename
            let filename = std::path::Path::new(&file_str)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let icon_b64 = get_app_icon(file_str.clone());
            let size_str = format_size(size);

            tracked_apps.push(TrackedApp {
                file_str,
                app_type_str,
                size_str,
                is_running,
                is_dir,
                icon_b64,
                filename,
                size,
            });

            // Sort descending by size
            tracked_apps.sort_by(|a, b| b.size.cmp(&a.size));
            
            // Clone items to send to UI thread
            let items_to_send: Vec<TrackedApp> = tracked_apps.clone();

            let ui_handle_cb = ui_handle_clone.clone();
            let current_cnt = cnt;
            let current_total = total_size;
            
            slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle_cb.upgrade() {
                    let mut app_items = Vec::new();
                    for a in items_to_send {
                        let icon = load_image_from_base64(&a.icon_b64);
                        app_items.push(AppItem {
                            file: SharedString::from(a.file_str),
                            app_type: SharedString::from(a.app_type_str),
                            size_str: SharedString::from(a.size_str),
                            is_running: a.is_running,
                            is_dir: a.is_dir,
                            icon,
                            filename: SharedString::from(a.filename),
                        });
                    }
                    let new_model = Rc::new(VecModel::from(app_items));
                    ui.set_apps(ModelRc::from(new_model));

                    let status = format!("这台电脑上已找到 {} 个 Chromium 内核的应用 ({}) - 搜索中...", current_cnt, format_size(current_total));
                    ui.set_search_status(SharedString::from(status));
                }
            })
            .unwrap();
        });

        let ui_handle_cb = ui_handle_clone.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle_cb.upgrade() {
                let status = if cnt > 0 {
                    format!("搜索完成！这台电脑上总共有 {} 个 Chromium 内核的应用 ({})", cnt, format_size(total_size))
                } else {
                    "搜索完成！这台电脑上没有 Chromium 内核的应用".to_string()
                };
                ui.set_search_status(SharedString::from(status));
            }
        })
        .unwrap();
    });

    ui.run()
}
