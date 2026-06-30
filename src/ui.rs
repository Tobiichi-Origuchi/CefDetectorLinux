use eframe::egui;
use std::collections::HashMap;
use std::sync::Arc;

use crate::icon_finder;
use crate::package_manager;

// ---- data ----

pub struct CefCard {
    pub file: String,
    pub app_type: String,
    pub is_running: bool,
    pub is_dir: bool,
    pub icon_rgba: Vec<u8>,
    pub filename: String,
    /// Size in KiB, for sorting (use `size_str()` for display).
    pub raw_size: i32,
}

impl CefCard {
    pub fn size_str(&self) -> String {
        let len = (self.raw_size as u64) * 1024;
        crate::format_size(len)
    }
}

pub enum UiMsg {
    AddItem(CefCard),
    Status { cnt: usize, total: u64 },
    Done { cnt: usize, total: u64 },
}

// ---- layout constants ----

const CARD_W: f32 = 94.0;
const CARD_H: f32 = 116.0;
const CELL_W: f32 = 106.0;
const CELL_H: f32 = 128.0;
const CARD_PAD_LR: f32 = 6.0;
const CARD_PAD_TB: f32 = 12.0;
const CARD_INNER_SPACING: f32 = 2.0;
const ICON_SIZE: f32 = 36.0;

// ---- card widget ----

fn card_ui(
    ui: &mut egui::Ui,
    card: &CefCard,
    texture: Option<&egui::TextureHandle>,
) -> egui::Response {
    let text_color = if card.is_running {
        egui::Color32::from_rgb(76, 175, 80)
    } else {
        egui::Color32::BLACK
    };

    let (rect, response) = ui.allocate_exact_size(egui::vec2(CARD_W, CARD_H), egui::Sense::click());

    if response.hovered() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
    }

    if ui.is_rect_visible(rect) {
        let bg_alpha: u8 = if response.hovered() { 140 } else { 76 };
        let bg = egui::Color32::from_rgba_unmultiplied(255, 255, 255, bg_alpha);

        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(4),
            egui::Stroke::new(1.0, bg),
            egui::StrokeKind::Middle,
        );

        let inner = rect.shrink2(egui::vec2(CARD_PAD_LR, CARD_PAD_TB));
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down_justified(egui::Align::Center)),
            |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                ui.set_min_width(inner.width());

                if let Some(tex) = texture {
                    ui.add(
                        egui::Image::new(tex).fit_to_exact_size(egui::vec2(ICON_SIZE, ICON_SIZE)),
                    );
                } else {
                    ui.add_sized(egui::vec2(ICON_SIZE, ICON_SIZE), egui::Spinner::new());
                }
                ui.add_space(CARD_INNER_SPACING);

                // Pre-truncated filename from CefCard; no per-frame truncation needed.
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&card.filename)
                            .color(text_color)
                            .size(11.0)
                            .strong(),
                    )
                    .truncate(),
                );
                ui.add_space(CARD_INNER_SPACING);

                ui.label(
                    egui::RichText::new(&card.app_type)
                        .color(text_color)
                        .size(10.0),
                );
                ui.add_space(CARD_INNER_SPACING);

                // Compute size_str on the fly from raw_size (one format! per visible card)
                ui.label(
                    egui::RichText::new(card.size_str())
                        .color(egui::Color32::from_black_alpha(214))
                        .size(9.0),
                );
            },
        );
    }

    response
}

fn open_card(card: &CefCard) {
    if card.is_dir {
        let _ = std::process::Command::new("xdg-open")
            .arg(&card.file)
            .spawn();
    } else if let Some(p) = std::path::Path::new(&card.file).parent() {
        let _ = std::process::Command::new("xdg-open").arg(p).spawn();
    }
}

// ---- app ----

pub struct CefDetectorApp {
    pub cards: Vec<CefCard>,
    pub status: String,
    pub done: bool,
    pub rx: std::sync::mpsc::Receiver<UiMsg>,
    pub textures: HashMap<String, egui::TextureHandle>,
    pub pending: Vec<CefCard>,
    pub bg: Option<egui::TextureHandle>,
    /// Cached status text galley to avoid re-layout every frame.
    pub status_galley: Option<(String, Arc<egui::Galley>)>,
}

impl eframe::App for CefDetectorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // ---- drain channel ----
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                UiMsg::AddItem(card) => self.pending.push(card),
                UiMsg::Status { cnt, total } => {
                    self.status = format!(
                        "这台电脑上已找到 {} 个 Chromium 内核的应用 ({}) - 搜索中...",
                        cnt,
                        crate::format_size(total)
                    );
                }
                UiMsg::Done { cnt, total } => {
                    self.done = true;
                    self.cards.sort_by_key(|a| std::cmp::Reverse(a.raw_size));
                    self.status = if cnt > 0 {
                        format!(
                            "搜索完成！这台电脑上总共有 {} 个 Chromium 内核的应用 ({})",
                            cnt,
                            crate::format_size(total)
                        )
                    } else {
                        "搜索完成！这台电脑上没有 Chromium 内核的应用".into()
                    };
                    crate::clear_decoded_image_cache();
                    icon_finder::clear_icon_caches();
                    package_manager::clear_pm_cache();
                }
            }
        }

        // ---- load textures (and free CPU-side icon_rgba) ----
        for mut card in self.pending.drain(..) {
            let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], &card.icon_rgba);
            let handle = ctx.load_texture(card.file.clone(), img, egui::TextureOptions::default());
            self.textures.insert(card.file.clone(), handle);
            // Release the 16 KB CPU-side RGBA buffer now that GPU owns the texture
            card.icon_rgba = Vec::new();
            self.cards.push(card);
        }

        // ---- background (lazy init, once) ----
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

        // ---- UI ----
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                let win = ui.max_rect();

                // background
                if let Some(ref bg) = self.bg {
                    let ts = bg.size_vec2();
                    let s = (win.width() / ts.x).max(win.height() / ts.y);
                    let dw = ts.x * s;
                    let dh = ts.y * s;
                    ui.painter().image(
                        bg.id(),
                        egui::Rect::from_min_size(
                            egui::pos2(
                                win.left() + (win.width() - dw) * 0.5,
                                win.top() + (win.height() - dh) * 0.5,
                            ),
                            egui::vec2(dw, dh),
                        ),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                // status text (cached galley to avoid re-layout every frame)
                let status_y = win.top() + win.height() * 0.21;
                let status_fg = if self.done {
                    egui::Color32::from_rgb(33, 150, 243)
                } else {
                    egui::Color32::WHITE
                };

                let galley = match &self.status_galley {
                    Some((cached_text, cached_galley)) if cached_text == &self.status => {
                        cached_galley.clone()
                    }
                    _ => {
                        let font_id = egui::FontId::new(18.0, egui::FontFamily::Proportional);
                        let g =
                            ui.painter()
                                .layout_no_wrap(self.status.clone(), font_id, status_fg);
                        self.status_galley = Some((self.status.clone(), g.clone()));
                        g
                    }
                };
                let galsz = galley.size();
                let x = win.left() + (win.width() - galsz.x) * 0.5;
                ui.painter()
                    .galley(egui::pos2(x, status_y), galley, status_fg);

                // scroll area
                let sa_left = win.left() + win.width() * 0.10;
                let sa_top = win.top() + win.height() * 0.30;
                let sa_w = win.width() * 0.80;
                let sa_h = win.height() * 0.60;
                let sa_rect =
                    egui::Rect::from_min_size(egui::pos2(sa_left, sa_top), egui::vec2(sa_w, sa_h));

                let cols = ((sa_w / CELL_W).floor() as usize).max(1);
                let rows = self.cards.len().div_ceil(cols);
                let content_h = (rows as f32 * CELL_H).max(sa_h);

                ui.scope_builder(egui::UiBuilder::new().max_rect(sa_rect), |ui| {
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
                });

                // repo link
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
                ui.painter()
                    .galley(egui::pos2(repo_rect.left(), text_y), galley, repo_color);
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

        // Only request repaints while the scan is still in progress.
        // Once done the UI is static — no need to burn CPU/GPU every 120 ms.
        if !self.done {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}
