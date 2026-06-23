#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
pub mod icon_finder;
pub mod models;
pub mod package_manager;
pub mod search;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use eframe::egui;
use icon_finder::{RawIcon, get_app_icon};
use search::core_search;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

// ---- data ----

struct CefCard {
    file: String,
    app_type: String,
    size_str: String,
    is_running: bool,
    is_dir: bool,
    icon_rgba: Vec<u8>,
    filename: String,
    raw_size: i32,
}

enum UiMsg {
    AddItem(CefCard),
    Status { cnt: usize, total: u64 },
    Done { cnt: usize, total: u64 },
}

// ---- helpers ----

fn format_size(len: u64) -> String {
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
        .map(|i| image::imageops::resize(&i, 64, 64, image::imageops::FilterType::Nearest).into_raw())
        .unwrap_or_else(|_| vec![0u8; 64 * 64 * 4])
});

fn raw_to_rgba(raw: &RawIcon) -> Vec<u8> {
    match raw {
        RawIcon::PngOrIco(bytes) => image::load_from_memory(bytes)
            .map(|i| image::imageops::resize(&i, 64, 64, image::imageops::FilterType::Nearest).into_raw())
            .unwrap_or_else(|_| DEFAULT_ICON_RGBA.clone()),
        RawIcon::Svg(_) | RawIcon::Empty => DEFAULT_ICON_RGBA.clone(),
    }
}

fn hash_raw_icon(icon: &RawIcon) -> u64 {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
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

pub fn clear_decoded_image_cache() {
    DECODED_IMAGE_CACHE.with(|c| c.borrow_mut().clear());
}

// ---- layout constants (matching original Slint pixel-for-pixel) ----

const CARD_W: f32 = 94.0;
const CARD_H: f32 = 116.0;
const CELL_W: f32 = 106.0;
const CELL_H: f32 = 128.0;
const CARD_PAD_LR: f32 = 6.0;
const CARD_PAD_TB: f32 = 12.0;
const CARD_INNER_SPACING: f32 = 2.0;
const ICON_SIZE: f32 = 36.0;

// ---- card widget ----

fn card_ui(ui: &mut egui::Ui, card: &CefCard, texture: Option<&egui::TextureHandle>) -> egui::Response {
    let text_color = if card.is_running {
        egui::Color32::from_rgb(76, 175, 80)
    } else {
        egui::Color32::BLACK
    };

    let (rect, response) = ui.allocate_exact_size(egui::vec2(CARD_W, CARD_H), egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let bg_alpha: u8 = if response.hovered() { 140 } else { 76 };
        let bg = egui::Color32::from_rgba_unmultiplied(255, 255, 255, bg_alpha);

        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
        painter.rect_stroke(rect, egui::CornerRadius::same(4), egui::Stroke::new(1.0, bg), egui::StrokeKind::Middle);

        let inner = rect.shrink2(egui::vec2(CARD_PAD_LR, CARD_PAD_TB));
        let mut cy = inner.top();

        // Icon (centered horizontally)
        if let Some(tex) = texture {
            let ix = inner.center().x - ICON_SIZE * 0.5;
            painter.image(
                tex.id(),
                egui::Rect::from_min_size(egui::pos2(ix, cy), egui::vec2(ICON_SIZE, ICON_SIZE)),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        cy += ICON_SIZE + CARD_INNER_SPACING;

        // Filename: 11px bold, max-width 76px, centered
        let font_11 = egui::FontId::new(11.0, egui::FontFamily::Proportional);
        let galley = painter.layout(card.filename.clone(), font_11, text_color, ui.available_width().min(76.0));
        let tx = rect.center().x - galley.size().x * 0.5;
        painter.galley(egui::pos2(tx, cy), galley, text_color);
        cy += 11.0 + CARD_INNER_SPACING;

        // App type: 10px
        let font_10 = egui::FontId::new(10.0, egui::FontFamily::Proportional);
        let galley = painter.layout_no_wrap(card.app_type.clone(), font_10, text_color);
        let tx = rect.center().x - galley.size().x * 0.5;
        painter.galley(egui::pos2(tx, cy), galley, text_color);
        cy += 10.0 + CARD_INNER_SPACING;

        // Size: 9px, rgba(0,0,0,0.84)
        let size_color = egui::Color32::from_black_alpha(214);
        let font_9 = egui::FontId::new(9.0, egui::FontFamily::Proportional);
        let galley = painter.layout_no_wrap(card.size_str.clone(), font_9, size_color);
        let tx = rect.center().x - galley.size().x * 0.5;
        painter.galley(egui::pos2(tx, cy), galley, size_color);
    }

    response
}

// ---- app ----

struct CefDetectorApp {
    cards: Vec<CefCard>,
    status: String,
    done: bool,
    rx: std::sync::mpsc::Receiver<UiMsg>,
    textures: HashMap<String, egui::TextureHandle>,
    pending: Vec<CefCard>,
    bg: Option<egui::TextureHandle>,
}

impl eframe::App for CefDetectorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ---- drain channel ----
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                UiMsg::AddItem(card) => self.pending.push(card),
                UiMsg::Status { cnt, total } => {
                    self.status = format!(
                        "这台电脑上已找到 {} 个 Chromium 内核的应用 ({}) - 搜索中...",
                        cnt,
                        format_size(total)
                    );
                }
                UiMsg::Done { cnt, total } => {
                    self.done = true;
                    self.cards.sort_by_key(|a| std::cmp::Reverse(a.raw_size));
                    self.status = if cnt > 0 {
                        format!(
                            "搜索完成！这台电脑上总共有 {} 个 Chromium 内核的应用 ({})",
                            cnt,
                            format_size(total)
                        )
                    } else {
                        "搜索完成！这台电脑上没有 Chromium 内核的应用".into()
                    };
                    clear_decoded_image_cache();
                    icon_finder::clear_icon_caches();
                    package_manager::clear_pm_cache();
                }
            }
        }

        // ---- load textures ----
        for card in self.pending.drain(..) {
            let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], &card.icon_rgba);
            let handle = ctx.load_texture(card.file.clone(), img, egui::TextureOptions::default());
            self.textures.insert(card.file.clone(), handle);
            self.cards.push(card);
        }

        // ---- background ----
        if self.bg.is_none()
            && let Ok(mut img) = image::load_from_memory(include_bytes!("../ui/background.webp"))
        {
            if img.width() > 1920 {
                img = img.resize(1920, u32::MAX, image::imageops::FilterType::CatmullRom);
            }
            let rgba = img.into_rgba8();
            let [w, h] = [rgba.width() as usize, rgba.height() as usize];
            self.bg = Some(ctx.load_texture(
                "bg",
                egui::ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw()),
                egui::TextureOptions::default(),
            ));
        }

        // ---- UI (absolute positioning — matching Slint layout exactly) ----
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let win = ui.max_rect(); // full window in screen coords

                // -- background (image-fit: cover) --
                if let Some(ref bg) = self.bg {
                    let ts = bg.size_vec2();
                    let s = (win.width() / ts.x).max(win.height() / ts.y);
                    let dw = ts.x * s;
                    let dh = ts.y * s;
                    ui.painter().image(
                        bg.id(),
                        egui::Rect::from_min_size(
                            egui::pos2(win.left() + (win.width() - dw) * 0.5, win.top() + (win.height() - dh) * 0.5),
                            egui::vec2(dw, dh),
                        ),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                // -- status text: y = 21vh, full-width centered --
                let status_y = win.top() + win.height() * 0.21;
                let status_fg = if self.done {
                    egui::Color32::from_rgb(33, 150, 243)
                } else {
                    egui::Color32::WHITE
                };
                {
                    let font_id = egui::FontId::new(18.0, egui::FontFamily::Proportional);
                    let galley = ui.painter().layout_no_wrap(
                        self.status.clone(),
                        font_id,
                        status_fg,
                    );
                    let galsz = galley.size();
                    let x = win.left() + (win.width() - galsz.x) * 0.5;
                    ui.painter().galley(egui::pos2(x, status_y), galley, status_fg);
                }

                // -- scroll area: x = 10vw, y = 30vh, w = 80vw, h = 60vh --
                let sa_left = win.left() + win.width() * 0.10;
                let sa_top = win.top() + win.height() * 0.30;
                let sa_w = win.width() * 0.80;
                let sa_h = win.height() * 0.60;
                let sa_rect = egui::Rect::from_min_size(egui::pos2(sa_left, sa_top), egui::vec2(sa_w, sa_h));

                let cols = ((sa_w / CELL_W).floor() as usize).max(1);
                let rows = self.cards.len().div_ceil(cols);
                let content_h = (rows as f32 * CELL_H).max(sa_h);

                ui.allocate_new_ui(
                    egui::UiBuilder::new().max_rect(sa_rect),
                    |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(sa_h)
                        .show(ui, |ui| {
                            ui.set_min_height(content_h);
                            ui.add_space(6.0);

                            for row_chunk in self.cards.chunks(cols) {
                                ui.horizontal(|ui| {
                                    ui.add_space(6.0);
                                    let mut first = true;
                                    for card in row_chunk {
                                        if !first {
                                            ui.add_space(12.0);
                                        }
                                        first = false;
                                        let tex = self.textures.get(&card.file);
                                        let resp = card_ui(ui, card, tex);
                                        if resp.clicked() {
                                            open_card(card);
                                        }
                                    }
                                });
                                ui.add_space(12.0);
                            }
                        });
                },
            );

                // -- repo link: x = 10px, y = window_bottom - 32px --
                let repo_rect = egui::Rect::from_min_max(
                    egui::pos2(win.left() + 10.0, win.bottom() - 32.0),
                    egui::pos2(win.right(), win.bottom()),
                );
                let repo_id = ui.make_persistent_id("repo_link");
                let repo_resp = ui.interact(repo_rect, repo_id, egui::Sense::click());
                let repo_color = if repo_resp.hovered() {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 204)
                };
                let repo_text = "Repo: github.com/Tobiichi-Origuchi/CefDetectorLinux (求个STAR!)";
                let galley = ui.painter().layout_no_wrap(
                    repo_text.into(),
                    egui::FontId::proportional(12.0),
                    repo_color,
                );
                let text_y = repo_rect.center().y - galley.size().y * 0.5;
                ui.painter().galley(egui::pos2(repo_rect.left(), text_y), galley, repo_color);
                if repo_resp.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
                if repo_resp.clicked() {
                    ctx.open_url(egui::OpenUrl {
                        url: "https://github.com/Tobiichi-Origuchi/CefDetectorLinux".into(),
                        new_tab: true,
                    });
                }
            });

        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }
}

fn open_card(card: &CefCard) {
    if card.is_dir {
        let _ = std::process::Command::new("xdg-open").arg(&card.file).spawn();
    } else if let Some(p) = std::path::Path::new(&card.file).parent() {
        let _ = std::process::Command::new("xdg-open").arg(p).spawn();
    }
}

// ---- main ----

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
                size_str: format_size(info.size),
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
        Box::new(|_cc| {
            Ok(Box::new(CefDetectorApp {
                cards: Vec::new(),
                status: "正在全盘搜索 CEF 应用，请耐心等待...".into(),
                done: false,
                rx,
                textures: HashMap::new(),
                pending: Vec::new(),
                bg: None,
            }))
        }),
    )
}
