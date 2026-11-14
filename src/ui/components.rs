//! 界面通用组件：统一圆角、间距、颜色与交互反馈。
//!
//! 所有面板共用这些小部件，保证密度一致、主次分明。

use crate::app::{
    ACCENT, LINE, MUTED, TEXT, TEXT_2, WHITE,
};
use std::path::Path;

/// 体积格式化口径下沉到 `core::estimate`，保证界面、测试与后续 CLI 一致。
pub fn fmt_size(bytes: u64) -> String {
    crate::core::estimate::fmt_size(bytes)
}

/// 段落标题 + 浅色副标题 + 右侧次要操作。
pub fn section_head(
    ui: &mut egui::Ui,
    title: &str,
    sub: &str,
    add_right: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).color(TEXT).size(13.0).strong());
        if !sub.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(sub).color(MUTED).size(11.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add_right(ui);
        });
    });
    ui.add_space(10.0);
}

/// 细分隔线。
pub fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.center().y),
            egui::pos2(rect.right(), rect.center().y),
        ],
        egui::Stroke::new(1.0, LINE),
    );
}

/// 带边框、圆角的内容容器。
pub fn card(
    ui: &mut egui::Ui,
    fill: egui::Color32,
    border: egui::Color32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::none()
        .fill(fill)
        .rounding(10.0)
        .stroke(egui::Stroke::new(1.0, border))
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| add_contents(ui));
}

/// 小标签。
pub fn badge(ui: &mut egui::Ui, text: &str, fg: egui::Color32, bg: egui::Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(fg).size(10.5).strong());
        });
}

pub fn version_badge(ui: &mut egui::Ui, version: &str) {
    let text = format!("v{}", version);
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(247, 248, 250))
        .rounding(6.0)
        .stroke(egui::Stroke::new(1.0, LINE))
        .inner_margin(egui::Margin::symmetric(7.0, 2.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(MUTED).size(10.5));
        });
}

/// 标题栏状态胶囊（带状态圆点）。
pub fn status_pill(ui: &mut egui::Ui, text: &str, fg: egui::Color32, bg: egui::Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(999.0)
        .stroke(egui::Stroke::new(1.0, fg.linear_multiply(0.22)))
        .inner_margin(egui::Margin::symmetric(11.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.5, fg);
                ui.add_space(5.0);
                ui.label(egui::RichText::new(text).color(fg).size(11.5).strong());
            });
        });
}

/// 固定尺寸的主操作按钮；禁用态仍占位，保证位置不变。
pub fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let fill = if enabled {
        ACCENT
    } else {
        egui::Color32::from_rgb(199, 211, 245)
    };
    let btn = egui::Button::new(
        egui::RichText::new(label).color(WHITE).size(12.5).strong(),
    )
    .fill(fill)
    .stroke(egui::Stroke::NONE)
    .rounding(9.0)
    .min_size(egui::vec2(150.0, 40.0));
    ui.add_enabled(enabled, btn)
}

pub fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label).color(TEXT_2).size(12.0),
    )
    .fill(WHITE)
    .stroke(egui::Stroke::new(1.0, LINE))
    .rounding(9.0)
    .min_size(egui::vec2(0.0, 40.0));
    ui.add(btn)
}

pub fn small_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label).color(TEXT_2).size(11.5),
    )
    .fill(WHITE)
    .stroke(egui::Stroke::new(1.0, LINE))
    .rounding(8.0)
    .min_size(egui::vec2(0.0, 28.0));
    ui.add(btn)
}

pub fn text_link(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(label).color(ACCENT).size(11.5),
    )
    .fill(egui::Color32::TRANSPARENT)
    .stroke(egui::Stroke::NONE)
    .rounding(6.0);
    ui.add(btn)
}

/// iOS 风格开关，点击整块区域切换。
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(36.0, 21.0), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let bg = if *on {
        ACCENT
    } else {
        egui::Color32::from_rgb(215, 219, 227)
    };
    ui.painter().rect(rect, egui::Rounding::same(11.0), bg, egui::Stroke::NONE);
    let knob_x = if *on {
        rect.right() - 11.0
    } else {
        rect.left() + 11.0
    };
    ui.painter().circle_filled(
        egui::pos2(knob_x, rect.center().y),
        7.5,
        WHITE,
    );
    response
}

pub fn progress_bar(ui: &mut egui::Ui, fraction: f32) {
    let width = ui.available_width().max(60.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::Rounding::same(3.0),
        egui::Color32::from_rgb(233, 236, 242),
        egui::Stroke::NONE,
    );
    let fill = (rect.width() * fraction.clamp(0.0, 1.0)).max(0.0);
    if fill > 0.0 {
        ui.painter().rect(
            egui::Rect::from_min_size(rect.min, egui::vec2(fill.max(5.0), rect.height())),
            egui::Rounding::same(3.0),
            ACCENT,
            egui::Stroke::NONE,
        );
    }
}

/// 底部提示条（风险告知 / 环境说明）。
pub fn notice(ui: &mut egui::Ui, text: &str, fg: egui::Color32, bg: egui::Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(9.0)
        .stroke(egui::Stroke::new(1.0, fg.linear_multiply(0.20)))
        .inner_margin(egui::Margin::symmetric(11.0, 9.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text).color(fg).size(10.5),
            );
        });
}

pub fn open_folder(path: &Path) -> std::io::Result<()> {
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
