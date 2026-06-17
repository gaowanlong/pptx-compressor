//! Media file list panel — Apple-style table with status badges.

use crate::app::{PptxCompressorApp, BLUE, GLASS_WHITE, TEXT_PRIMARY, TEXT_SECONDARY, GREEN, ORANGE, RED};
use crate::core::MediaStatus;

/// Pill-shaped status badge.
fn status_badge(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) {
    let badge = egui::Frame::none()
        .fill(bg)
        .rounding(egui::Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(fg).size(11.0).strong());
        });
    let _ = badge;
}

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    // Reserve consistent height so empty placeholder and populated table
    // occupy the same vertical space — prevents layout jump on file load.
    ui.set_min_height(200.0);

    if app.media_files.is_empty() {
        // Empty state inside same-style container as the populated table area,
        // so the card height never changes between "no files" and "has files".
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(252, 252, 253))
            .rounding(8.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 237, 240)))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(72.0);
                            ui.label(
                                egui::RichText::new("Select a PPTX file to see its media contents")
                                    .color(TEXT_SECONDARY)
                                    .size(12.0),
                            );
                            ui.add_space(72.0);
                        });
                    });
            });
        return;
    }

    // ── Summary bar ────────────────────────────────────
    let total_original: u64 = app.media_files.iter().map(|m| m.original_size).sum();

    // Use actual compressed size for processed files, estimate for pending ones.
    let has_actual = app.media_files.iter().any(|m| {
        matches!(m.status, MediaStatus::Done | MediaStatus::Skipped | MediaStatus::Failed)
    });
    let display_total: u64 = app
        .media_files
        .iter()
        .map(|m| match m.status {
            MediaStatus::Done if m.compressed_size > 0 => m.compressed_size,
            MediaStatus::Skipped | MediaStatus::Failed => m.original_size,
            _ => m.estimated_size(&app.settings),
        })
        .sum();
    let savings = if total_original > 0 {
        ((1.0 - display_total as f64 / total_original as f64) * 100.0) as u8
    } else {
        0
    };

    let summary = egui::Frame::none()
        .fill(egui::Color32::from_rgb(246, 247, 250))
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} files", app.media_files.len()))
                        .color(TEXT_PRIMARY)
                        .size(12.0)
                        .strong(),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(bytesize::ByteSize(total_original).to_string())
                        .color(TEXT_SECONDARY)
                        .size(12.0),
                );
                ui.separator();
                if has_actual {
                    ui.label(
                        egui::RichText::new(format!("→ {}", bytesize::ByteSize(display_total)))
                            .color(BLUE)
                            .size(12.0)
                            .strong(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!("→ estimated {}", bytesize::ByteSize(display_total)))
                            .color(BLUE)
                            .size(12.0)
                            .strong(),
                    );
                }
                ui.separator();
                let savings_color = match savings {
                    s if s >= 30 => GREEN,
                    s if s > 0 => BLUE,
                    _ => ORANGE,
                };
                if has_actual {
                    ui.label(
                        egui::RichText::new(format!("save {}%", savings))
                            .color(savings_color)
                            .size(12.0)
                            .strong(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!("save~{}%", savings))
                            .color(savings_color)
                            .size(12.0)
                            .strong(),
                    );
                }
            });
        });
    let _ = summary;

    ui.add_space(8.0);

    // ── Table ──────────────────────────────────────────
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .show(ui, |ui| {
            egui::Frame::none()
                .fill(GLASS_WHITE)
                .rounding(8.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(237, 238, 242)))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    egui::Grid::new("media_grid")
                        .num_columns(5)
                        .spacing([12.0, 6.0])
                        .min_col_width(50.0)
                        .show(ui, |ui| {
                            // ── Header ──
                            let hdr = egui::Color32::from_rgb(244, 246, 251);
                            for header in &["File", "Type", "Size", "Status", "Compress"] {
                                egui::Frame::none()
                                    .fill(hdr)
                                    .rounding(6.0)
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(*header)
                                                .color(BLUE)
                                                .size(11.0)
                                                .strong(),
                                        );
                                    });
                            }
                            ui.end_row();

                            // ── Rows ──
                            for (idx, media) in app.media_files.iter_mut().enumerate() {
                                let row_bg = if idx % 2 == 0 {
                                    egui::Color32::TRANSPARENT
                                } else {
                                    egui::Color32::from_rgb(248, 249, 252)
                                };

                                // File name
                                let fname = media
                                    .extracted_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                egui::Frame::none()
                                    .fill(row_bg)
                                    .rounding(4.0)
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(&fname)
                                                .color(TEXT_PRIMARY)
                                                .size(11.0),
                                        );
                                    });

                                // Type badge
                                let (type_lbl, type_color) = match media.media_type {
                                    crate::core::MediaType::Jpeg => ("JPEG", egui::Color32::from_rgb(0, 140, 200)),
                                    crate::core::MediaType::Png => ("PNG", BLUE),
                                    crate::core::MediaType::Gif => ("GIF", egui::Color32::from_rgb(200, 110, 40)),
                                    crate::core::MediaType::Video => ("Video", egui::Color32::from_rgb(0, 180, 150)),
                                    crate::core::MediaType::Other => ("Other", TEXT_SECONDARY),
                                };
                                egui::Frame::none()
                                    .fill(row_bg)
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        status_badge(ui, type_lbl, type_color.linear_multiply(0.12), type_color);
                                    });

                                // Size
                                egui::Frame::none()
                                    .fill(row_bg)
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(bytesize::ByteSize(media.original_size).to_string())
                                                .color(TEXT_PRIMARY)
                                                .size(11.0),
                                        );
                                    });

                                // Status badge
                                let (stat_text, stat_bg, stat_fg) = match media.status {
                                    MediaStatus::Pending => ("Pending", egui::Color32::from_rgb(237, 238, 242), TEXT_SECONDARY),
                                    MediaStatus::Processing => ("Processing", egui::Color32::from_rgb(224, 236, 255), BLUE),
                                    MediaStatus::Done => ("Done", egui::Color32::from_rgb(224, 248, 234), GREEN),
                                    MediaStatus::Skipped => ("Skipped", egui::Color32::from_rgb(255, 244, 224), ORANGE),
                                    MediaStatus::Failed => ("Failed", egui::Color32::from_rgb(255, 232, 230), RED),
                                };
                                egui::Frame::none()
                                    .fill(row_bg)
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        status_badge(ui, stat_text, stat_bg, stat_fg);
                                    });

                                // Checkbox
                                egui::Frame::none()
                                    .fill(row_bg)
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        if app.state != crate::app::AppState::Compressing
                                            && media.media_type.is_compressible()
                                        {
                                            ui.checkbox(&mut media.enabled, "");
                                        }
                                    });

                                ui.end_row();

                                // ── Detail row for compressed items ──
                                if media.status == MediaStatus::Done && media.compressed_size > 0 {
                                    let saved = media.original_size.saturating_sub(media.compressed_size);
                                    let pct = if media.original_size > 0 {
                                        (saved as f64 / media.original_size as f64 * 100.0) as u8
                                    } else {
                                        0
                                    };
                                    egui::Frame::none()
                                        .fill(egui::Color32::from_rgb(243, 250, 244))
                                        .rounding(4.0)
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.add_space(40.0);
                                            ui.label("");
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "→ {} (saved {} / {}%)",
                                                    bytesize::ByteSize(media.compressed_size),
                                                    bytesize::ByteSize(saved),
                                                    pct,
                                                ))
                                                .color(GREEN)
                                                .size(11.0),
                                            );
                                            ui.label("");
                                            ui.label("");
                                        });
                                    ui.end_row();
                                }
                            }
                        });
                });
        });
}
