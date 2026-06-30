#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod font;
pub mod icon_finder;
pub mod models;
pub mod package_manager;
pub mod search;
mod ui;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use eframe::egui;
use icon_finder::{RawIcon, get_app_icon};
use search::core_search;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use ui::{CefCard, CefDetectorApp, UiMsg};

pub(crate) fn format_size(len: u64) -> String {
    if len == 0 {
        return "0.00 B".into();
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

static DEFAULT_ICON_RGBA: LazyLock<Vec<u8>> = LazyLock::new(|| {
    image::load_from_memory(include_bytes!("../icons/default_cef_icon.ico"))
        .map(|i| {
            image::imageops::resize(&i, 64, 64, image::imageops::FilterType::Nearest).into_raw()
        })
        .unwrap_or_else(|_| vec![0u8; 64 * 64 * 4])
});

fn raw_to_rgba(raw: &RawIcon) -> Vec<u8> {
    match raw {
        RawIcon::PngOrIco(bytes) => image::load_from_memory(bytes)
            .map(|i| {
                image::imageops::resize(&i, 64, 64, image::imageops::FilterType::Nearest).into_raw()
            })
            .unwrap_or_else(|_| DEFAULT_ICON_RGBA.clone()),
        RawIcon::Svg(_) | RawIcon::Empty => DEFAULT_ICON_RGBA.clone(),
    }
}

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

thread_local! {
    static DECODED_IMAGE_CACHE: std::cell::RefCell<HashMap<u64, Vec<u8>>> =
        std::cell::RefCell::new(HashMap::new());
}

fn load_icon_cached(raw: &RawIcon) -> Vec<u8> {
    let hash = hash_raw_icon(raw);
    if let Some(c) = DECODED_IMAGE_CACHE.with(|c| c.borrow().get(&hash).cloned()) {
        return c;
    }
    let rgba = raw_to_rgba(raw);
    DECODED_IMAGE_CACHE.with(|c| c.borrow_mut().insert(hash, rgba.clone()));
    rgba
}

pub(crate) fn clear_decoded_image_cache() {
    DECODED_IMAGE_CACHE.with(|c| c.borrow_mut().clear());
}

fn main() -> Result<(), eframe::Error> {
    cli::handle_cli();

    let icon_data = image::load_from_memory(include_bytes!("../icons/128x128.png"))
        .ok()
        .map(|i| {
            let rgba = i.into_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            Arc::new(egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            })
        });

    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut cnt = 0usize;
        let mut total = 0u64;
        let mut batch: Vec<CefCard> = Vec::new();
        let mut last_flush = Instant::now();
        const FLUSH_MS: u64 = 50;
        const BATCH_MAX: usize = 20;

        core_search(|info| {
            cnt += 1;
            total += info.size;

            let icon_raw = get_app_icon(info.file.clone());
            let icon_rgba = load_icon_cached(&icon_raw);

            let filename = std::path::Path::new(&info.file)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            batch.push(CefCard {
                file: info.file.clone(),
                app_type: info.app_type.clone(),
                is_running: info.is_running,
                is_dir: info.is_dir,
                icon_rgba,
                filename,
                raw_size: (info.size / 1024) as i32,
            });

            if batch.len() >= BATCH_MAX || last_flush.elapsed().as_millis() as u64 >= FLUSH_MS {
                for item in batch.drain(..) {
                    let _ = tx.send(UiMsg::AddItem(item));
                }
                let _ = tx.send(UiMsg::Status { cnt, total });
                last_flush = Instant::now();
            }
        });

        for item in batch.drain(..) {
            let _ = tx.send(UiMsg::AddItem(item));
        }
        let _ = tx.send(UiMsg::Done { cnt, total });
    });

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([800.0, 600.0])
        .with_title("CEF Detector Linux");

    let viewport = if let Some(icon) = icon_data {
        viewport.with_icon(icon)
    } else {
        viewport
    };

    eframe::run_native(
        "CEF Detector Linux",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| {
            if let Some(cjk_font) = font::load_cjk_font() {
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    "CJK".into(),
                    Arc::new(
                        egui::FontData::from_owned(cjk_font).tweak(egui::FontTweak::default()),
                    ),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "CJK".into());
                cc.egui_ctx.set_fonts(fonts);
            }

            Ok(Box::new(CefDetectorApp {
                cards: Vec::new(),
                status: "正在全盘搜索 CEF 应用，请耐心等待...".into(),
                done: false,
                rx,
                textures: HashMap::new(),
                pending: Vec::new(),
                bg: None,
                status_galley: None,
            }))
        }),
    )
}
