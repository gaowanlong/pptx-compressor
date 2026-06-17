//! PPTX Compressor — entry point.
//!
//! Loads the app icon from embedded assets, configures Chinese font support,
//! and applies an Apple-native glassmorphism theme with blue primary color.

// Hide the console window on Windows (GUI-only application).
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod core;
mod handlers;
mod ui;
mod utils;

use app::PptxCompressorApp;

fn main() -> eframe::Result {
    let icon_data = load_icon();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([860.0, 740.0])
            .with_min_inner_size([640.0, 520.0])
            .with_title("PPTX Compressor")
            .with_icon(icon_data),
        ..Default::default()
    };

    eframe::run_native(
        "PPTX Compressor",
        native_options,
        Box::new(|cc| {
            setup_glass_theme(&cc.egui_ctx);
            setup_chinese_font(&cc.egui_ctx);
            Ok(Box::new(PptxCompressorApp::default()))
        }),
    )
}

/// Load the app icon from the embedded 256×256 PNG asset.
fn load_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon_256.png");
    if let Ok(img) = image::load_from_memory(icon_bytes) {
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        egui::IconData {
            rgba: rgba.into_raw(),
            width,
            height,
        }
    } else {
        egui::IconData {
            rgba: vec![0, 0, 0, 0],
            width: 1,
            height: 1,
        }
    }
}

/// Apple-native glassmorphism theme with blue primary color.
fn setup_glass_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // ── Spacing (Apple HIG: generous) ──────────────────────
    style.spacing.item_spacing = egui::vec2(14.0, 10.0);
    style.spacing.button_padding = egui::vec2(20.0, 8.0);
    style.spacing.indent = 24.0;

    // ── Color palette ──────────────────────────────────────
    let blue = egui::Color32::from_rgb(0, 122, 255);       // Apple Blue
    let blue_dark = egui::Color32::from_rgb(0, 85, 204);   // darker
    let blue_light = egui::Color32::from_rgb(64, 156, 255); // lighter
    let text_primary = egui::Color32::from_rgb(29, 29, 31);
    let bg = egui::Color32::from_rgb(242, 242, 244);       // warm light gray
    let card_bg = egui::Color32::from_rgba_premultiplied(255, 255, 255, 245);
    let card_border = egui::Color32::from_rgb(232, 232, 237);

    style.visuals = egui::Visuals {
        window_fill: bg,
        panel_fill: bg,

        widgets: egui::style::Widgets {
            noninteractive: egui::style::WidgetVisuals {
                bg_fill: card_bg,
                weak_bg_fill: egui::Color32::from_rgb(235, 235, 240),
                bg_stroke: egui::Stroke::new(1.0, card_border),
                rounding: egui::Rounding::same(10.0),
                fg_stroke: egui::Stroke::new(1.0, text_primary),
                expansion: 0.0,
            },
            inactive: egui::style::WidgetVisuals {
                bg_fill: blue,
                weak_bg_fill: blue_light,
                bg_stroke: egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                rounding: egui::Rounding::same(10.0),
                fg_stroke: egui::Stroke::new(1.0, egui::Color32::WHITE),
                expansion: 0.0,
            },
            hovered: egui::style::WidgetVisuals {
                bg_fill: blue_light,
                weak_bg_fill: egui::Color32::from_rgb(80, 168, 255),
                bg_stroke: egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                rounding: egui::Rounding::same(10.0),
                fg_stroke: egui::Stroke::new(1.5, egui::Color32::WHITE),
                expansion: 1.0,
            },
            active: egui::style::WidgetVisuals {
                bg_fill: blue_dark,
                weak_bg_fill: egui::Color32::from_rgb(0, 70, 180),
                bg_stroke: egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                rounding: egui::Rounding::same(8.0),
                fg_stroke: egui::Stroke::new(2.0, egui::Color32::WHITE),
                expansion: 1.0,
            },
            open: egui::style::WidgetVisuals {
                bg_fill: egui::Color32::from_rgb(0, 105, 220),
                weak_bg_fill: egui::Color32::from_rgb(0, 95, 210),
                bg_stroke: egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                rounding: egui::Rounding::same(8.0),
                fg_stroke: egui::Stroke::new(1.0, egui::Color32::WHITE),
                expansion: 0.0,
            },
        },

        override_text_color: Some(text_primary),
        selection: egui::style::Selection {
            bg_fill: blue.linear_multiply(0.25),
            stroke: egui::Stroke::new(1.0, blue),
        },
        hyperlink_color: blue,
        faint_bg_color: bg,
        dark_mode: false,
        ..Default::default()
    };

    ctx.set_style(style);
}

/// Add a Chinese font to egui's font list for CJK rendering.
///
/// Platform-specific font paths:
/// - macOS: STHeiti (苹方) / Songti (宋体)
/// - Windows: Microsoft YaHei (微软雅黑)
/// - Linux: Noto Sans CJK / WenQuanYi Micro Hei
fn setup_chinese_font(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    let font_candidates: &[&str] = &[
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Songti.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ];

    #[cfg(target_os = "windows")]
    let font_candidates: &[&str] = &[
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simfang.ttf",
        "C:\\Windows\\Fonts\\SIMLI.TTF",
    ];

    #[cfg(target_os = "linux")]
    let font_candidates: &[&str] = &[
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    ];

    let font_data = font_candidates
        .iter()
        .find_map(|path| std::fs::read(path).ok())
        .map(|data| egui::FontData::from_owned(data));

    if let Some(data) = font_data {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("CJK".to_owned(), data);

        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "CJK".to_owned());
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.push("CJK".to_owned());
        }

        ctx.set_fonts(fonts);
    }
}
