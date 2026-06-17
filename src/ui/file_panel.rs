//! File selection panel — glassmorphism drag & drop zone and Apple-style file card.

use crate::app::{AppState, PptxCompressorApp, BLUE, TEXT_PRIMARY, TEXT_SECONDARY};
use crate::core::extractor::extract_pptx;

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    // Reserve consistent height so drop zone and file card occupy same vertical space
    ui.set_min_height(195.0);

    if app.input_files.is_empty() {
        show_drop_zone(ui, app);
    } else {
        show_file_loaded(ui, app);
    }

    // Handle drag-and-drop
    if let Some(hovered) = ui.input(|i| i.raw.dropped_files.first().cloned()) {
        if let Some(path) = hovered.path {
            if path.extension().and_then(|e| e.to_str()) == Some("pptx") {
                app.input_files = vec![path];
                load_pptx(app);
            }
        }
    }
}

/// Empty state: glassmorphism drop zone with dashed-style border.
fn show_drop_zone(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    let zone = egui::Frame::none()
        .fill(egui::Color32::from_rgba_premultiplied(240, 245, 255, 240))
        .rounding(16.0)
        .stroke(egui::Stroke::new(1.5, BLUE.linear_multiply(0.35)))
        .inner_margin(egui::Margin::symmetric(24.0, 36.0))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("📁").size(40.0),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Drag & drop a PPTX file here")
                        .color(BLUE)
                        .size(16.0)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("or")
                        .color(TEXT_SECONDARY)
                        .size(13.0),
                );
                ui.add_space(6.0);

                // Glass-style browse button
                if glass_button(ui, "Browse Files", BLUE).clicked() {
                    if let Some(files) = rfd::FileDialog::new()
                        .add_filter("PowerPoint", &["pptx"])
                        .pick_files()
                    {
                        app.input_files = files;
                        load_pptx(app);
                    }
                }
            });
        });
    let _ = zone;
}

/// Loaded state: Apple-style file info card.
fn show_file_loaded(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    for file in &app.input_files {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let size = std::fs::metadata(file)
            .map(|m| bytesize::ByteSize(m.len()).to_string())
            .unwrap_or_else(|_| "?".into());

        let file_card = egui::Frame::none()
            .fill(egui::Color32::from_rgb(245, 249, 255))
            .rounding(12.0)
            .stroke(egui::Stroke::new(1.0, BLUE.linear_multiply(0.2)))
            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📄").size(22.0));
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(&*name)
                            .color(TEXT_PRIMARY)
                            .size(14.0)
                            .strong(),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(size)
                            .color(TEXT_SECONDARY)
                            .size(12.0),
                    );
                });
            });
        let _ = file_card;
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        if outline_button(ui, "Change File").clicked() {
            app.state = AppState::Idle;
            app.media_files.clear();
            app.input_files.clear();
            if let Some(files) = rfd::FileDialog::new()
                .add_filter("PowerPoint", &["pptx"])
                .pick_files()
            {
                app.input_files = files;
                load_pptx(app);
            }
        }
        ui.add_space(6.0);
        if app.state != AppState::Compressing {
            if text_button(ui, "Clear").clicked() {
                app.state = AppState::Idle;
                app.media_files.clear();
                app.input_files.clear();
                app._temp_dir = None;
                app.extract_dir = None;
            }
        }
    });
}

/// Glass-style filled button.
pub fn glass_button(ui: &mut egui::Ui, label: &str, color: egui::Color32) -> egui::Response {
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

/// Outline-style button (for secondary actions).
pub fn outline_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label)
            .color(BLUE)
            .size(12.0),
    )
    .fill(egui::Color32::TRANSPARENT)
    .stroke(egui::Stroke::new(1.0, BLUE.linear_multiply(0.4)))
    .rounding(8.0)
    .min_size(egui::vec2(0.0, 30.0));
    ui.add(btn)
}

/// Subtle text-only button.
pub fn text_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label)
            .color(TEXT_SECONDARY)
            .size(12.0),
    )
    .fill(egui::Color32::TRANSPARENT)
    .rounding(6.0)
    .min_size(egui::vec2(0.0, 28.0));
    ui.add(btn)
}

fn load_pptx(app: &mut PptxCompressorApp) {
    if let Some(pptx_path) = app.input_files.first() {
        app.state = AppState::Loading;

        match tempfile::tempdir() {
            Ok(temp_dir) => {
                let extract_path = temp_dir.path().to_path_buf();
                match extract_pptx(pptx_path, &extract_path) {
                    Ok(media) => {
                        app.media_files = media;
                        app.extract_dir = Some(extract_path);
                        app._temp_dir = Some(temp_dir);
                        app.state = AppState::Ready;
                    }
                    Err(e) => {
                        app.error_messages.push(format!("Failed to extract PPTX: {}", e));
                        app.state = AppState::Error;
                    }
                }
            }
            Err(e) => {
                app.error_messages.push(format!("Failed to create temp directory: {}", e));
                app.state = AppState::Error;
            }
        }
    }
}
