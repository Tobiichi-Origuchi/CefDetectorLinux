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
use slint::{Image, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

fn hash_raw_icon(icon: &RawIcon) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    match icon {
        RawIcon::Svg(b) | RawIcon::PngOrIco(b) => b.hash(&mut h),
        RawIcon::Empty => 0_u8.hash(&mut h),
    }
    h.finish()
}

// Thread-local because slint::Image is !Send
thread_local! {
    static DECODED_IMAGE_CACHE: std::cell::RefCell<HashMap<u64, Image>> =
        std::cell::RefCell::new(HashMap::new());
}

pub fn clear_decoded_image_cache() {
    DECODED_IMAGE_CACHE.with(|c| c.borrow_mut().clear());
}

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

fn load_image_from_raw(raw: &RawIcon) -> Image {
    let hash = hash_raw_icon(raw);

    // Check thread-local decoded-image cache
    if let Some(cached) = DECODED_IMAGE_CACHE.with(|c| c.borrow().get(&hash).cloned()) {
        return cached;
    }

    let image = match raw {
        RawIcon::Svg(bytes) => Image::load_from_svg_data(bytes).unwrap_or_default(),
        RawIcon::PngOrIco(bytes) => {
            if let Ok(mut dynamic_image) = image::load_from_memory(bytes) {
                if dynamic_image.width() > 64 || dynamic_image.height() > 64 {
                    dynamic_image = dynamic_image.thumbnail(64, 64);
                }
                let rgba = dynamic_image.into_rgba8();
                let buffer = slint::SharedPixelBuffer::clone_from_slice(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                );
                Image::from_rgba8(buffer)
            } else {
                Image::default()
            }
        }
        RawIcon::Empty => Image::default(),
    };

    DECODED_IMAGE_CACHE.with(|c| c.borrow_mut().insert(hash, image.clone()));
    image
}

fn main() -> Result<(), slint::PlatformError> {
    cli::handle_cli();

    let ui = AppWindow::new()?;

    if let Err(e) = slint::set_xdg_app_id("cefdetector") {
        eprintln!("Warning: Failed to set XDG app ID: {:?}", e);
    }

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

    // Slint's Image / SharedString are !Send → defer construction to UI thread
    struct PendingItem {
        file: String,
        app_type: String,
        size: u64,
        is_running: bool,
        is_dir: bool,
        icon_raw: RawIcon,
        filename: String,
        raw_size_kb: i32,
    }

    let ui_handle_clone = ui_handle.clone();
    std::thread::spawn(move || {
        let mut cnt = 0;
        let mut total_size = 0;

        let mut ui_batch: Vec<PendingItem> = Vec::new();
        let mut last_flush = Instant::now();
        const FLUSH_INTERVAL_MS: u64 = 50;
        const BATCH_SIZE: usize = 20;

        core_search(|info| {
            cnt += 1;
            total_size += info.size;

            let icon_raw = get_app_icon(info.file.clone());

            let filename = std::path::Path::new(&info.file)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let raw_size_kb = (info.size / 1024) as i32;

            ui_batch.push(PendingItem {
                file: info.file.clone(),
                app_type: info.app_type.clone(),
                size: info.size,
                is_running: info.is_running,
                is_dir: info.is_dir,
                icon_raw,
                filename,
                raw_size_kb,
            });

            let elapsed = last_flush.elapsed().as_millis() as u64;
            if ui_batch.len() >= BATCH_SIZE || elapsed >= FLUSH_INTERVAL_MS {
                let items = std::mem::take(&mut ui_batch);
                let ui_h = ui_handle_clone.clone();
                let current_cnt = cnt;
                let current_total = total_size;

                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        let model = ui.get_apps();
                        if let Some(vec_model) =
                            model.as_any().downcast_ref::<slint::VecModel<AppItem>>()
                        {
                            for p in items {
                                let icon = load_image_from_raw(&p.icon_raw);
                                let size_str = format_size(p.size);

                                vec_model.push(AppItem {
                                    file: SharedString::from(p.file),
                                    app_type: SharedString::from(p.app_type),
                                    size_str: SharedString::from(size_str),
                                    is_running: p.is_running,
                                    is_dir: p.is_dir,
                                    icon,
                                    filename: SharedString::from(p.filename),
                                    raw_size: p.raw_size_kb,
                                });
                            }
                        }
                        ui.set_search_status(SharedString::from(format!(
                            "这台电脑上已找到 {} 个 Chromium 内核的应用 ({}) - 搜索中...",
                            current_cnt,
                            format_size(current_total)
                        )));
                    }
                })
                .unwrap();

                last_flush = Instant::now();
            }
        });

        // Flush remaining batched items
        if !ui_batch.is_empty() {
            let items = std::mem::take(&mut ui_batch);
            let ui_h = ui_handle_clone.clone();
            let current_cnt = cnt;
            let current_total = total_size;

            slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_h.upgrade() {
                    let model = ui.get_apps();
                    if let Some(vec_model) =
                        model.as_any().downcast_ref::<slint::VecModel<AppItem>>()
                    {
                        for p in items {
                            let icon = load_image_from_raw(&p.icon_raw);
                            let size_str = format_size(p.size);
                            vec_model.push(AppItem {
                                file: SharedString::from(p.file),
                                app_type: SharedString::from(p.app_type),
                                size_str: SharedString::from(size_str),
                                is_running: p.is_running,
                                is_dir: p.is_dir,
                                icon,
                                filename: SharedString::from(p.filename),
                                raw_size: p.raw_size_kb,
                            });
                        }
                    }
                    ui.set_search_status(SharedString::from(format!(
                        "这台电脑上已找到 {} 个 Chromium 内核的应用 ({}) - 搜索中...",
                        current_cnt,
                        format_size(current_total)
                    )));
                }
            })
            .unwrap();
        }

        let ui_handle_cb = ui_handle_clone.clone();
        let final_cnt = cnt;
        let final_total = total_size;
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle_cb.upgrade() {
                let model = ui.get_apps();
                if let Some(vec_model) = model.as_any().downcast_ref::<slint::VecModel<AppItem>>() {
                    let count = vec_model.row_count();
                    let mut items: Vec<AppItem> =
                        (0..count).filter_map(|i| vec_model.row_data(i)).collect();
                    items.sort_by_key(|item| std::cmp::Reverse(item.raw_size));
                    vec_model.set_vec(items);
                }

                let status = if final_cnt > 0 {
                    format!(
                        "搜索完成！这台电脑上总共有 {} 个 Chromium 内核的应用 ({})",
                        final_cnt,
                        format_size(final_total)
                    )
                } else {
                    "搜索完成！这台电脑上没有 Chromium 内核的应用".to_string()
                };
                ui.set_search_status(SharedString::from(status));
                ui.set_search_done(true);

                clear_decoded_image_cache();
                icon_finder::clear_icon_caches();
                package_manager::clear_pm_cache();
            }
        })
        .unwrap();
    });

    ui.run()
}
