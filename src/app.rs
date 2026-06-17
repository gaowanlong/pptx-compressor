//! Main application state and egui App implementation.
//!
//! Apple-native glassmorphism layout with blue primary color.
//! Layout is stable — Media Files section is always shown to prevent
//! visual shifts when state transitions from Idle to Ready.

use crate::core::{CompressMessage, CompressionSettings, MediaInfo};
use crate::ui::{file_panel, media_list, progress, settings};
use crate::utils::ffmpeg;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

// ── Apple-native glassmorphism color palette ─────────────────
pub const BLUE: egui::Color32 = egui::Color32::from_rgb(0, 122, 255);
pub const GLASS_WHITE: egui::Color32 = egui::Color32::from_rgba_premultiplied(255, 255, 255, 245);
pub const BG: egui::Color32 = egui::Color32::from_rgb(242, 242, 244);
pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(29, 29, 31);
pub const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(134, 134, 139);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(52, 199, 89);
pub const ORANGE: egui::Color32 = egui::Color32::from_rgb(255, 149, 0);
pub const RED: egui::Color32 = egui::Color32::from_rgb(255, 59, 48);

/// Current application state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Loading,
    Ready,
    Compressing,
    Done,
    Error,
}

/// The main application struct holding all UI and processing state.
pub struct PptxCompressorApp {
    pub state: AppState,
    pub input_files: Vec<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub media_files: Vec<MediaInfo>,
    pub settings: CompressionSettings,
    pub _temp_dir: Option<tempfile::TempDir>,
    pub extract_dir: Option<PathBuf>,
    pub rx: Option<mpsc::Receiver<CompressMessage>>,
    pub cancel_flag: Arc<AtomicBool>,
    pub progress_current: usize,
    pub progress_total: usize,
    pub progress_file: String,
    pub result_original: u64,
    pub result_compressed: u64,
    pub error_messages: Vec<String>,
    pub ffmpeg_available: bool,
    pub show_advanced: bool,
    pub output_path: Option<PathBuf>,
}

impl Default for PptxCompressorApp {
    fn default() -> Self {
        Self {
            state: AppState::Idle,
            input_files: Vec::new(),
            output_dir: None,
            media_files: Vec::new(),
            settings: CompressionSettings::default(),
            _temp_dir: None,
            extract_dir: None,
            rx: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            progress_current: 0,
            progress_total: 0,
            progress_file: String::new(),
            result_original: 0,
            result_compressed: 0,
            error_messages: Vec::new(),
            ffmpeg_available: ffmpeg::find_ffmpeg().is_some(),
            show_advanced: false,
            output_path: None,
        }
    }
}

impl eframe::App for PptxCompressorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_messages();

        // ── macOS-style unified header ──────────────────────
        egui::TopBottomPanel::top("header_panel")
            .frame(egui::Frame::none().fill(BLUE))
            .show(ctx, |ui| {
                let header_max = ui.max_rect();
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.add_space(16.0);

                    // App icon
                    ui.label(
                        egui::RichText::new("📦")
                            .size(22.0),
                    );
                    ui.add_space(10.0);

                    // Title
                    ui.heading(
                        egui::RichText::new("PPTX Compressor")
                            .color(egui::Color32::WHITE)
                            .size(20.0)
                            .strong(),
                    );

                    // Spacer pushes everything to the right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(16.0);

                        // Version badge (glass-like capsule)
                        let version = egui::Frame::none()
                            .fill(egui::Color32::from_rgba_premultiplied(255, 255, 255, 25))
                            .rounding(12.0)
                            .inner_margin(egui::Margin::symmetric(12.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("v0.2")
                                        .color(egui::Color32::from_rgba_premultiplied(255, 255, 255, 180))
                                        .size(11.0),
                                );
                            });
                        let _ = version;

                        if !self.ffmpeg_available {
                            ui.add_space(12.0);
                            ui.colored_label(
                                ORANGE,
                                "⚠ FFmpeg not found",
                            );
                        }
                    });
                });
                ui.add_space(10.0);

                // Thin highlight line at bottom of header
                let painter = ui.painter();
                painter.line_segment(
                    [
                        egui::pos2(header_max.left(), header_max.bottom()),
                        egui::pos2(header_max.right(), header_max.bottom()),
                    ],
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(255, 255, 255, 30)));

            });

        // ── Glassmorphism content area ──────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                ui.add_space(16.0);

                // Action bar (above File Selection — primary CTA)
                action_bar(ui, self);

                ui.add_space(12.0);

                // Card 1: File Selection (always visible, stable height)
                glass_card(ui, "File Selection", BLUE, Some(210.0), |ui| {
                    file_panel::show(ui, self);
                });

                ui.add_space(12.0);

                // Card 2+3: Settings + Progress side-by-side (equal width via columns)
                ui.columns(2, |columns| {
                    glass_card(&mut columns[0], "Compression Settings", BLUE, Some(210.0), |ui| {
                        settings::show(ui, self);
                    });
                    glass_card(&mut columns[1], "Progress", BLUE, Some(210.0), |ui| {
                        progress::show(ui, self);
                    });
                });

                ui.add_space(12.0);

                // Card 4: Media Files (always visible; empty/populated handled inside)
                glass_card(ui, "Media Files", BLUE, Some(250.0), |ui| {
                    media_list::show(ui, self);
                });

                ui.add_space(16.0);
                    }); // ScrollArea
            }); // CentralPanel

        if self.state == AppState::Compressing {
            ctx.request_repaint();
        }
    }
}

/// Render a glassmorphism-style card with frosted-glass aesthetic.
///
/// When `content_height` is `Some(h)`, the content area is allocated at a fixed height
/// and wrapped in a `ScrollArea` so that overflow scrolls instead of expanding the card.
/// This keeps card sizes consistent across state transitions (empty ↔ loaded, etc.).
fn glass_card(
    ui: &mut egui::Ui,
    title: &str,
    accent_color: egui::Color32,
    content_height: Option<f32>,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    // Capture full available width before entering Frame; used inside
    // to set a minimum width on the content Ui so the card never shrinks.
    let desired_width = ui.available_width();

    // Glass card: semi-transparent white, light border, generous rounding
    let card_frame = egui::Frame::none()
        .fill(GLASS_WHITE)
        .rounding(14.0)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 235, 240)))
        .inner_margin(egui::Margin::symmetric(18.0, 14.0))
        .show(ui, |ui| {
            // Accent dot + section title
            ui.horizontal(|ui| {
                let dot = egui::Frame::none()
                    .fill(accent_color)
                    .rounding(3.0)
                    .inner_margin(egui::Margin::symmetric(5.0, 5.0))
                    .show(ui, |_| {});
                let _ = dot;
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(title)
                        .color(TEXT_PRIMARY)
                        .size(15.0)
                        .strong(),
                );
            });

            // Separator line below title
            ui.add_space(6.0);
            let sep = ui.min_rect();
            let painter = ui.painter();
            painter.line_segment(
                [
                    egui::pos2(sep.left(), sep.top()),
                    egui::pos2(sep.right(), sep.top()),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_rgb(240, 240, 245)),
            );
            ui.add_space(10.0);

            // Force the content area's min_rect to span the full card width,
            // preventing the Frame from shrinking when content transitions
            // between states (e.g. drop zone with icon ↔ compact file info).
            // Frame.inner_margin = symmetric(18, 14) → 36px horizontal total.
            ui.set_min_width(desired_width - 36.0);

            // Content area
            if let Some(h) = content_height {
                // Fixed-size scrollable area: always h tall, scrolls if content overflows.
                // id_source gives each card a unique scroll state (critical for nested ScrollAreas).
                // auto_shrink(false) ensures the ScrollArea always fills the full h allocation
                // even when content is smaller, so the Frame's size stays consistent.
                egui::ScrollArea::vertical()
                    .id_salt(title)
                    .auto_shrink([false; 2])
                    .min_scrolled_height(h)
                    .max_height(h)
                    .show(ui, |ui| {
                        add_contents(ui);
                    });
            } else {
                // Auto-height: original behavior
                add_contents(ui);
            }
        });
    let _ = card_frame;
}

/// Contextual action bar — shows the primary action button above File Selection.
/// Always visible with stable height to prevent layout jumps.
fn action_bar(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    ui.set_min_height(50.0);

    match app.state {
        AppState::Idle => {
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Open a PPTX file to begin")
                        .color(TEXT_SECONDARY)
                        .size(12.0),
                );
            });
        }

        AppState::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Extracting PPTX...")
                        .color(TEXT_PRIMARY)
                        .size(13.0)
                        .strong(),
                );
            });
        }

        AppState::Ready => {
            egui::Frame::none()
                .fill(GLASS_WHITE)
                .rounding(14.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 235, 240)))
                .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{} media files ready", app.media_files.len()))
                                .color(TEXT_PRIMARY)
                                .size(13.0),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn = egui::Button::new(
                                egui::RichText::new("▶  Start Compression")
                                    .color(egui::Color32::WHITE)
                                    .size(13.0)
                                    .strong(),
                            )
                            .fill(BLUE)
                            .rounding(10.0)
                            .min_size(egui::vec2(0.0, 34.0));
                            if ui.add(btn).clicked() {
                                app.start_compression();
                            }
                        });
                    });
                });
        }

        AppState::Compressing => {
            let pct = if app.progress_total > 0 {
                app.progress_current as f32 / app.progress_total as f32 * 100.0
            } else {
                0.0
            };
            egui::Frame::none()
                .fill(GLASS_WHITE)
                .rounding(14.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 235, 240)))
                .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("Compressing... ({:.0}%)", pct))
                                .color(TEXT_PRIMARY)
                                .size(13.0),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn = egui::Button::new(
                                egui::RichText::new("Cancel")
                                    .color(TEXT_SECONDARY)
                                    .size(12.0),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 212, 217)))
                            .rounding(8.0)
                            .min_size(egui::vec2(0.0, 30.0));
                            if ui.add(btn).clicked() {
                                app.cancel_flag.store(true, Ordering::Relaxed);
                            }
                        });
                    });
                });
        }

        AppState::Done => {
            let saved = app.result_original.saturating_sub(app.result_compressed);
            let pct = if app.result_original > 0 {
                (saved as f64 / app.result_original as f64 * 100.0) as u8
            } else {
                0
            };
            egui::Frame::none()
                .fill(GLASS_WHITE)
                .rounding(14.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 235, 240)))
                .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("✓ Compression complete — saved {}%", pct))
                                .color(GREEN)
                                .size(13.0)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn = egui::Button::new(
                                egui::RichText::new("Process another file")
                                    .color(egui::Color32::WHITE)
                                    .size(12.0)
                                    .strong(),
                            )
                            .fill(BLUE)
                            .rounding(10.0)
                            .min_size(egui::vec2(0.0, 32.0));
                            if ui.add(btn).clicked() {
                                *app = PptxCompressorApp::default();
                            }
                        });
                    });
                });
        }

        AppState::Error => {
            egui::Frame::none()
                .fill(GLASS_WHITE)
                .rounding(14.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 235, 240)))
                .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Compression failed")
                                .color(RED)
                                .size(13.0)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn = egui::Button::new(
                                egui::RichText::new("↻  Try Again")
                                    .color(egui::Color32::WHITE)
                                    .size(12.0)
                                    .strong(),
                            )
                            .fill(RED)
                            .rounding(10.0)
                            .min_size(egui::vec2(0.0, 32.0));
                            if ui.add(btn).clicked() {
                                app.error_messages.clear();
                                app.state = AppState::Ready;
                            }
                        });
                    });
                });
        }
    }
}

impl PptxCompressorApp {
    fn process_messages(&mut self) {
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    CompressMessage::StatusUpdate(idx, status, compressed_size) => {
                        if idx < self.media_files.len() {
                            self.media_files[idx].status = status;
                            if compressed_size > 0 {
                                self.media_files[idx].compressed_size = compressed_size;
                            }
                        }
                    }
                    CompressMessage::Progress(current, total, file) => {
                        self.progress_current = current;
                        self.progress_total = total;
                        self.progress_file = file;
                    }
                    CompressMessage::Finished(original, compressed) => {
                        self.result_original = original;
                        self.result_compressed = compressed;
                        self.state = AppState::Done;
                        self.progress_file = String::new();
                    }
                    CompressMessage::Error(msg) => {
                        self.error_messages.push(msg);
                    }
                    CompressMessage::FatalError(msg) => {
                        self.error_messages.push(msg);
                        self.state = AppState::Error;
                    }
                }
            }
        }
    }

    pub fn start_compression(&mut self) {
        if self.input_files.is_empty() || self.extract_dir.is_none() {
            return;
        }

        self.state = AppState::Compressing;
        self.error_messages.clear();
        self.cancel_flag.store(false, Ordering::Relaxed);

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let mut media_files = self.media_files.clone();
        let settings = self.settings.clone();
        let extract_dir = self.extract_dir.clone().unwrap();
        let cancel = self.cancel_flag.clone();

        let input = &self.input_files[0];
        let original_pptx_size = std::fs::metadata(input)
            .map(|m| m.len())
            .unwrap_or(0);
        let output_path = if let Some(ref out_dir) = self.output_dir {
            let stem = input.file_stem().unwrap_or_default().to_string_lossy();
            out_dir.join(format!("{}_compressed.pptx", stem))
        } else {
            let stem = input.file_stem().unwrap_or_default().to_string_lossy();
            let parent = input.parent().unwrap_or(input.as_path());
            parent.join(format!("{}_compressed.pptx", stem))
        };

        self.output_path = Some(output_path.clone());

        std::thread::spawn(move || {
            let result = crate::core::pipeline::run_pipeline(
                &extract_dir,
                &output_path,
                &mut media_files,
                &settings,
                original_pptx_size,
                tx.clone(),
                cancel,
            );
            if let Err(e) = result {
                let _ = tx.send(CompressMessage::FatalError(e));
            }
        });
    }
}
