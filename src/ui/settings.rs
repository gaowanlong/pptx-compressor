//! Compression settings panel — Apple-style controls with blue theme.

use crate::app::{PptxCompressorApp, BLUE, TEXT_PRIMARY, TEXT_SECONDARY};

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    // Reserve consistent height to match Progress card side-by-side
    ui.set_min_height(210.0);

    // ── Preset pills ────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Preset")
                .color(TEXT_PRIMARY)
                .size(13.0)
                .strong(),
        );
        ui.add_space(10.0);

        if preset_pill(ui, "High Quality", true).clicked() {
            app.settings.preset_high();
        }
        ui.add_space(4.0);
        if preset_pill(ui, "Balanced", false).clicked() {
            app.settings.preset_medium();
        }
        ui.add_space(4.0);
        if preset_pill(ui, "Small Size", false).clicked() {
            app.settings.preset_low();
        }
    });

    ui.add_space(14.0);

    // ── Image Quality ───────────────────────────────────
    let img_quality = app.settings.image_quality;
    slider_row(
        ui,
        "Image Quality",
        &mut app.settings.image_quality,
        10..=100,
        img_quality,
    );

    ui.add_space(8.0);

    // ── Video CRF ───────────────────────────────────────
    let crf = app.settings.video_crf;
    slider_row(
        ui,
        "Video Quality",
        &mut app.settings.video_crf,
        0..=51,
        crf,
    );
    // Show CRF label below
    ui.add_space(-4.0);
    ui.horizontal(|ui| {
        ui.add_space(110.0); // indent to match slider position
        ui.label(
            egui::RichText::new(crf_quality(crf))
                .color(TEXT_SECONDARY)
                .size(11.0),
        );
    });

    ui.add_space(12.0);

    // ── GIF settings ────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("GIF")
                .color(TEXT_PRIMARY)
                .size(13.0)
                .strong(),
        );
        ui.add_space(8.0);

        if glass_toggle(ui, "Palette", app.settings.gif_palette_optimize).clicked() {
            app.settings.gif_palette_optimize = !app.settings.gif_palette_optimize;
        }
        ui.add_space(4.0);
        if glass_toggle(ui, "Frame Drop", app.settings.gif_frame_drop).clicked() {
            app.settings.gif_frame_drop = !app.settings.gif_frame_drop;
        }
        if app.settings.gif_frame_drop {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("FPS")
                    .color(TEXT_SECONDARY)
                    .size(12.0),
            );
            ui.add(
                egui::DragValue::new(&mut app.settings.gif_target_fps)
                    .range(1..=60)
                    .speed(1)
                    .suffix(" fps"),
            );
        }
    });

    ui.add_space(10.0);

    // ── Advanced toggle ─────────────────────────────────
    let expand_icon = if app.show_advanced { "▼" } else { "▶" };
    if link_button(ui, &format!("{}  Advanced Options", expand_icon)).clicked() {
        app.show_advanced = !app.show_advanced;
    }

    if app.show_advanced {
        ui.add_space(8.0);
        let adv_panel = egui::Frame::none()
            .fill(egui::Color32::from_rgb(247, 248, 250))
            .rounding(10.0)
            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut app.settings.image_max_width_enabled, "Limit image width");
                    if app.settings.image_max_width_enabled {
                        ui.add(
                            egui::DragValue::new(&mut app.settings.image_max_width)
                                .range(320..=7680)
                                .speed(10)
                                .suffix(" px"),
                        );
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut app.settings.video_max_resolution_enabled, "Limit video height");
                    if app.settings.video_max_resolution_enabled {
                        ui.add(
                            egui::DragValue::new(&mut app.settings.video_max_height)
                                .range(240..=4320)
                                .speed(10)
                                .suffix(" p"),
                        );
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Output dir")
                            .color(TEXT_SECONDARY)
                            .size(12.0),
                    );
                    if let Some(ref dir) = app.output_dir {
                        ui.colored_label(TEXT_SECONDARY, dir.to_string_lossy().as_ref());
                    } else {
                        ui.colored_label(TEXT_SECONDARY, "(same as input)");
                    }
                    if ui.button("Choose").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            app.output_dir = Some(dir);
                        }
                    }
                    if app.output_dir.is_some() && ui.button("Reset").clicked() {
                        app.output_dir = None;
                    }
                });
            });
        let _ = adv_panel;
    }
}

/// Slider row with label, slider, and value display.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u8,
    range: std::ops::RangeInclusive<u8>,
    display_value: u8,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .color(TEXT_PRIMARY)
                .size(13.0),
        );
        ui.add_space(8.0);
        let slider = egui::Slider::new(value, range)
            .show_value(false)
            .trailing_fill(true);
        ui.add(slider);
        ui.label(
            egui::RichText::new(format!("{}", display_value))
                .color(BLUE)
                .size(13.0)
                .strong(),
        );
    });
}

/// Preset pill button — subtle outline for unselected, filled for selected.
fn preset_pill(ui: &mut egui::Ui, label: &str, _selected: bool) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label)
            .color(egui::Color32::WHITE)
            .size(12.0)
            .strong(),
    )
    .fill(BLUE)
    .rounding(20.0)
    .min_size(egui::vec2(0.0, 28.0));
    ui.add(btn)
}

/// Glass-style toggle.
fn glass_toggle(ui: &mut egui::Ui, label: &str, is_on: bool) -> egui::Response {
    let bg = if is_on {
        BLUE
    } else {
        egui::Color32::from_rgb(220, 222, 227)
    };
    let text_color = if is_on {
        egui::Color32::WHITE
    } else {
        TEXT_SECONDARY
    };
    let btn = egui::Button::new(
        egui::RichText::new(label)
            .color(text_color)
            .size(12.0),
    )
    .fill(bg)
    .rounding(14.0)
    .min_size(egui::vec2(0.0, 26.0));
    ui.add(btn)
}

/// Text-styled link button.
fn link_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    egui::Frame::none()
        .fill(egui::Color32::TRANSPARENT)
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(4.0, 2.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .color(BLUE)
                    .size(12.0),
            )
        })
        .inner
}

fn crf_quality(crf: u8) -> &'static str {
    match crf {
        0..=17 => "Very high quality, larger file",
        18..=23 => "High quality",
        24..=28 => "Balanced",
        29..=35 => "Lower quality, smaller file",
        36..=51 => "Very low quality",
        _ => "",
    }
}
