#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod cli;
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod icon_finder;
pub mod models;
pub mod package_manager;
pub mod search;

use icon_finder::{RawIcon, get_app_icon};
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

fn load_image_from_raw(raw: RawIcon) -> Image {
    match raw {
        RawIcon::Svg(bytes) => Image::load_from_svg_data(&bytes).unwrap_or_default(),
        RawIcon::PngOrIco(bytes) => {
            if let Ok(mut dynamic_image) = image::load_from_memory(&bytes) {
                if dynamic_image.width() > 64 || dynamic_image.height() > 64 {
                    dynamic_image = dynamic_image.thumbnail(64, 64);
                }
                let rgba = dynamic_image.into_rgba8();
                let buffer = slint::SharedPixelBuffer::clone_from_slice(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                );
                return Image::from_rgba8(buffer);
            }
            Image::default()
        }
        RawIcon::Empty => Image::default(),
    }
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
        search::open_path(
            "https://github.com/Tobiichi-Origuchi/CefDetectorLinux".into(),
            false,
        );
    });

    let ui_handle_clone = ui_handle.clone();
    std::thread::spawn(move || {
        let mut cnt = 0;
        let mut total_size = 0;

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

            let ui_handle_cb = ui_handle_clone.clone();
            let current_cnt = cnt;
            let current_total = total_size;

            slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle_cb.upgrade() {
                    let icon_raw = get_app_icon(file_str.clone());
                    let icon = load_image_from_raw(icon_raw);
                    let size_str = format_size(size);

                    let raw_size_kb = (size / 1024) as i32;

                    let new_item = AppItem {
                        file: SharedString::from(file_str),
                        app_type: SharedString::from(app_type_str),
                        size_str: SharedString::from(size_str),
                        is_running,
                        is_dir,
                        icon,
                        filename: SharedString::from(filename),
                        raw_size: raw_size_kb,
                    };

                    let model = ui.get_apps();
                    use slint::Model;
                    if let Some(vec_model) =
                        model.as_any().downcast_ref::<slint::VecModel<AppItem>>()
                    {
                        let mut insert_idx = 0;
                        use slint::Model;
                        let count = vec_model.row_count();
                        while insert_idx < count {
                            if let Some(item) = vec_model.row_data(insert_idx)
                                && item.raw_size < raw_size_kb
                            {
                                break;
                            }
                            insert_idx += 1;
                        }
                        vec_model.insert(insert_idx, new_item);
                    }

                    let status = format!(
                        "这台电脑上已找到 {} 个 Chromium 内核的应用 ({}) - 搜索中...",
                        current_cnt,
                        format_size(current_total)
                    );
                    ui.set_search_status(SharedString::from(status));
                }
            })
            .unwrap();
        });

        let ui_handle_cb = ui_handle_clone.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle_cb.upgrade() {
                let status = if cnt > 0 {
                    format!(
                        "搜索完成！这台电脑上总共有 {} 个 Chromium 内核的应用 ({})",
                        cnt,
                        format_size(total_size)
                    )
                } else {
                    "搜索完成！这台电脑上没有 Chromium 内核的应用".to_string()
                };
                ui.set_search_status(SharedString::from(status));
                ui.set_search_done(true);
            }
        })
        .unwrap();
    });

    ui.run()
}
