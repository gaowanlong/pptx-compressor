//! Progress and results panel — Apple-style with glass elements.

use crate::app::{AppState, PptxCompressorApp, BLUE, TEXT_PRIMARY, TEXT_SECONDARY, GREEN, RED};
use std::sync::atomic::Ordering;

/// Glass-style filled button.
fn glass_button(ui: &mut egui::Ui, label: &str, color: egui::Color32) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label)
            .color(egui::Color32::WHITE)
            .size(13.0)
            .strong(),
    )
    .fill(color)
    .rounding(10.0)
    .min_size(egui::vec2(0.0, 34.0));
    ui.add(btn)
}

/// Outline button.
fn outline_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label)
            .color(TEXT_SECONDARY)
            .size(12.0),
    )
    .fill(egui::Color32::TRANSPARENT)
    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 212, 217)))
    .rounding(8.0)
    .min_size(egui::vec2(0.0, 30.0));
    ui.add(btn)
}

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    // Reserve consistent height so idle/ready/compressing/done/error states
    // all occupy the same vertical space — prevents layout jump mid-processing.
    ui.set_min_height(210.0);

    match app.state {
        AppState::Idle => {
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Select a PPTX file to begin.")
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
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("Ready — {} media files to compress", app.media_files.len()))
                        .color(TEXT_PRIMARY)
                        .size(12.0),
                );
            });
        }

        AppState::Compressing => {
            let progress = if app.progress_total > 0 {
                app.progress_current as f32 / app.progress_total as f32
            } else {
                0.0
            };

            // Apple-style progress bar
            let bar_h = 6.0;
            let avail_w = ui.available_width() - 4.0;
            let (_bar_id, bar_rect) = ui.allocate_space(egui::vec2(avail_w.max(100.0), bar_h + 24.0));

            let track = egui::Rect::from_min_size(
                egui::pos2(bar_rect.left(), bar_rect.top() + 12.0),
                egui::vec2(avail_w.max(100.0), bar_h),
            );
            // Track
            ui.painter().rect(
                track,
                egui::Rounding::same(3.0),
                egui::Color32::from_rgb(225, 226, 230),
                egui::Stroke::NONE,
            );
            // Fill
            if progress > 0.0 {
                let fill_w = (track.width() * progress).max(4.0);
                let fill = egui::Rect::from_min_size(track.min, egui::vec2(fill_w, bar_h));
                ui.painter().rect(
                    fill,
                    egui::Rounding::same(3.0),
                    BLUE,
                    egui::Stroke::NONE,
                );
            }

            // Percentage label
            let pct = (progress * 100.0) as u8;
            let label_pos = egui::pos2(track.left(), track.top());
            ui.painter().text(
                egui::pos2(label_pos.x, label_pos.y),
                egui::Align2::LEFT_TOP,
                format!("{}%", pct),
                egui::FontId::proportional(12.0),
                TEXT_PRIMARY,
            );

            // File name and cancel row
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                if !app.progress_file.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "({}/{}) {}",
                            app.progress_current + 1,
                            app.progress_total,
                            app.progress_file
                        ))
                        .color(TEXT_SECONDARY)
                        .size(11.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if outline_button(ui, "Cancel").clicked() {
                        app.cancel_flag.store(true, Ordering::Relaxed);
                    }
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

            // Success card — green glassmorphism
            let card = egui::Frame::none()
                .fill(egui::Color32::from_rgba_premultiplied(235, 252, 240, 250))
                .rounding(12.0)
                .stroke(egui::Stroke::new(1.0, GREEN.linear_multiply(0.25)))
                .inner_margin(egui::Margin::symmetric(18.0, 16.0))
                .show(ui, |ui| {
                    // Title
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("✓  Compression Complete")
                                .color(GREEN)
                                .size(16.0)
                                .strong(),
                        );
                    });

                    ui.add_space(10.0);

                    // Stats row
                    let stat_bg = egui::Color32::from_rgba_premultiplied(245, 253, 248, 240);
                    egui::Frame::none()
                        .fill(stat_bg)
                        .rounding(8.0)
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Original ")
                                        .color(TEXT_SECONDARY)
                                        .size(12.0),
                                );
                                ui.label(
                                    egui::RichText::new(bytesize::ByteSize(app.result_original).to_string())
                                        .color(TEXT_PRIMARY)
                                        .size(13.0)
                                        .strong(),
                                );
                                ui.separator();
                                ui.label(
                                    egui::RichText::new("Compressed ")
                                        .color(TEXT_SECONDARY)
                                        .size(12.0),
                                );
                                ui.label(
                                    egui::RichText::new(bytesize::ByteSize(app.result_compressed).to_string())
                                        .color(GREEN)
                                        .size(13.0)
                                        .strong(),
                                );
                                ui.separator();
                                let savings_color = if pct >= 30 { GREEN } else { BLUE };
                                ui.label(
                                    egui::RichText::new(format!("Saved {}%", pct))
                                        .color(savings_color)
                                        .size(14.0)
                                        .strong(),
                                );
                            });
                        });

                    ui.add_space(8.0);

                    // Output path + actions
                    ui.horizontal(|ui| {
                        if let Some(ref path) = app.output_path {
                            ui.label(
                                egui::RichText::new(path.to_string_lossy().as_ref())
                                    .color(TEXT_SECONDARY)
                                    .size(10.0),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if glass_button(ui, "📂  Open Folder", BLUE).clicked() {
                                if let Some(ref path) = app.output_path {
                                    if let Some(parent) = path.parent() {
                                        let _ = open_folder(parent);
                                    }
                                } else if let Some(file) = app.input_files.first() {
                                    if let Some(parent) = file.parent() {
                                        let _ = open_folder(parent);
                                    }
                                }
                            }
                        });
                    });
                });
            let _ = card;
        }

        AppState::Error => {
            // Error card — red glassmorphism
            let card = egui::Frame::none()
                .fill(egui::Color32::from_rgba_premultiplied(255, 240, 238, 250))
                .rounding(12.0)
                .stroke(egui::Stroke::new(1.0, RED.linear_multiply(0.25)))
                .inner_margin(egui::Margin::symmetric(18.0, 16.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("✕  Compression Failed")
                                .color(RED)
                                .size(16.0)
                                .strong(),
                        );
                    });

                    ui.add_space(10.0);

                    for err in &app.error_messages {
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(255, 245, 243))
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(err)
                                        .color(RED)
                                        .size(12.0),
                                );
                            });
                        ui.add_space(4.0);
                    }

                });
            let _ = card;
        }
    }
}

fn open_folder(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}
