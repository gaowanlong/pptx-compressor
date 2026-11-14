//! 右侧从属区上半部：压缩方案（预设 / 质量 / 兼容模式 / 高级项）。
//!
//! 高风险参数收进折叠区，默认只呈现三档预设与两个质量滑块。

use crate::app::{
    PptxCompressorApp, ACCENT, LINE, MUTED, SURFACE, TEXT, TEXT_2, WHITE,
};
use crate::core::Preset;
use crate::ui::components;

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    ui.label(egui::RichText::new("压缩方案").color(TEXT).size(13.0).strong());
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("选一个预设，或按需微调")
            .color(MUTED)
            .size(11.0),
    );
    ui.add_space(12.0);

    show_presets(ui, app);
    ui.add_space(16.0);
    show_slider(
        ui,
        "图片质量",
        &mut app.settings.image_quality,
        "数值越高越清晰；JPEG 重编码后若变大会自动保留原图",
    );
    ui.add_space(14.0);
    let crf = app.settings.crf();
    show_slider(
        ui,
        "视频质量",
        &mut app.settings.video_quality,
        &format!("对应 H.264 CRF {} · 数值越高画质越好", crf),
    );
    ui.add_space(16.0);
    show_toggle_row(ui, app);
    ui.add_space(10.0);
    show_advanced(ui, app);
}

fn show_presets(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    let current = app.settings.preset();

    egui::Frame::none()
        .fill(SURFACE)
        .rounding(10.0)
        .stroke(egui::Stroke::new(1.0, LINE))
        .inner_margin(egui::Margin::symmetric(3.0, 3.0))
        .show(ui, |ui| {
            let width = ((ui.available_width() - 4.0) / 3.0).max(60.0);
            ui.horizontal(|ui| {
                for preset in [Preset::High, Preset::Balanced, Preset::Small] {
                    let selected = current == preset;
                    let btn = egui::Button::new(
                        egui::RichText::new(preset.label())
                            .color(if selected { ACCENT } else { TEXT_2 })
                            .size(11.5)
                            .strong(),
                    )
                    .fill(if selected { WHITE } else { egui::Color32::TRANSPARENT })
                    .stroke(if selected {
                        egui::Stroke::new(1.0, LINE)
                    } else {
                        egui::Stroke::NONE
                    })
                    .rounding(7.0)
                    .min_size(egui::vec2(width, 30.0));
                    if ui.add(btn).clicked() {
                        app.settings.apply_preset(preset);
                    }
                }
            });
        });

    if current == Preset::Custom {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("当前为自定义参数（已微调预设）")
                .color(MUTED)
                .size(10.5),
        );
    }
}

fn show_slider(ui: &mut egui::Ui, label: &str, value: &mut u8, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(TEXT).size(12.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{}", *value))
                    .color(ACCENT)
                    .size(12.0)
                    .strong(),
            );
        });
    });
    ui.add(
        egui::Slider::new(value, 0..=100)
            .show_value(false)
            .trailing_fill(true),
    );
    ui.add_space(4.0);
    ui.label(egui::RichText::new(hint).color(MUTED).size(10.5));
}

fn show_toggle_row(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    components::hairline(ui);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("兼容模式")
                    .color(TEXT)
                    .size(12.0)
                    .strong(),
            );
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new("把非 MP4 视频转为 H.264 MP4，并同步更新包内声明")
                    .color(MUTED)
                    .size(10.5),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            components::toggle_switch(ui, &mut app.settings.video_convert_to_h264);
        });
    });
}

fn show_advanced(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    let marker = if app.show_advanced { "- " } else { "+ " };
    let label = format!("{}高级设置", marker);
    if components::text_link(ui, &label).clicked() {
        app.show_advanced = !app.show_advanced;
    }

    if !app.show_advanced {
        return;
    }

    ui.add_space(8.0);
    components::card(ui, WHITE, LINE, |ui| {
        ui.label(
            egui::RichText::new("GIF 动图")
                .color(TEXT)
                .size(11.5)
                .strong(),
        );
        ui.add_space(4.0);
        ui.checkbox(&mut app.settings.gif_palette_optimize, "优化调色板");
        ui.checkbox(&mut app.settings.gif_frame_drop, "抽取帧以降低体积");
        if app.settings.gif_frame_drop {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("目标帧率").color(TEXT_2).size(11.0));
                ui.add(
                    egui::DragValue::new(&mut app.settings.gif_target_fps)
                        .range(1..=60)
                        .speed(1)
                        .suffix(" fps"),
                );
            });
        }

        ui.add_space(8.0);
        components::hairline(ui);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.checkbox(&mut app.settings.image_max_width_enabled, "限制图片宽度");
            if app.settings.image_max_width_enabled {
                ui.add(
                    egui::DragValue::new(&mut app.settings.image_max_width)
                        .range(320..=7680)
                        .speed(10)
                        .suffix(" px"),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut app.settings.video_max_resolution_enabled,
                "限制视频高度",
            );
            if app.settings.video_max_resolution_enabled {
                ui.add(
                    egui::DragValue::new(&mut app.settings.video_max_height)
                        .range(240..=4320)
                        .speed(10)
                        .suffix(" p"),
                );
            }
        });

        ui.add_space(8.0);
        components::hairline(ui);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("输出目录").color(TEXT_2).size(11.0));
            let text = app
                .output_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "与源文件相同".into());
            ui.label(
                egui::RichText::new(shorten(&text, 26))
                    .color(MUTED)
                    .size(10.5),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.output_dir.is_some() && components::small_button(ui, "重置").clicked() {
                    app.output_dir = None;
                }
                if components::small_button(ui, "选择").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        app.output_dir = Some(dir);
                    }
                }
            });
        });
    });
}

fn shorten(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_owned();
    }
    let head: String = chars[..max_chars.saturating_sub(5)].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{}…{}", head, tail)
}
