//! PPTX 压缩器 — 程序入口。
//!
//! 负责窗口、图标、纯白克制主题与中文字体。业务逻辑全部来自库 crate，
//! 避免 `main.rs` 与 `lib.rs` 重复编译同一批模块。

// Windows 下隐藏控制台窗口（纯 GUI 应用）。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use pptx_compressor::app::{ACCENT, ACCENT_DARK, ACCENT_SOFT, LINE, SURFACE, TEXT, TEXT_2, WHITE};
use pptx_compressor::app::PptxCompressorApp;

fn main() -> eframe::Result {
    let icon_data = load_icon();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 740.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("PPTX 压缩器")
            .with_icon(icon_data),
        ..Default::default()
    };

    eframe::run_native(
        "pptx-compressor",
        native_options,
        Box::new(|cc| {
            setup_theme(&cc.egui_ctx);
            let font_ok = setup_chinese_font(&cc.egui_ctx);
            Ok(Box::new(PptxCompressorApp::new(cc, font_ok)))
        }),
    )
}

/// 从内嵌资源加载 256×256 应用图标。
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

/// 纯白底、细线、单一强调色。次要控件保持白底，主按钮单独着色。
fn setup_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    style.spacing.item_spacing = egui::vec2(12.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.indent = 20.0;

    let border = egui::Stroke::new(1.0, LINE);
    let widgets = egui::style::Widgets {
        noninteractive: egui::style::WidgetVisuals {
            bg_fill: WHITE,
            weak_bg_fill: SURFACE,
            bg_stroke: border,
            rounding: egui::Rounding::same(8.0),
            fg_stroke: egui::Stroke::new(1.0, TEXT_2),
            expansion: 0.0,
        },
        inactive: egui::style::WidgetVisuals {
            bg_fill: WHITE,
            weak_bg_fill: WHITE,
            bg_stroke: border,
            rounding: egui::Rounding::same(8.0),
            fg_stroke: egui::Stroke::new(1.0, TEXT),
            expansion: 0.0,
        },
        hovered: egui::style::WidgetVisuals {
            bg_fill: SURFACE,
            weak_bg_fill: SURFACE,
            bg_stroke: egui::Stroke::new(1.0, ACCENT),
            rounding: egui::Rounding::same(8.0),
            fg_stroke: egui::Stroke::new(1.0, ACCENT_DARK),
            expansion: 0.0,
        },
        active: egui::style::WidgetVisuals {
            bg_fill: ACCENT_SOFT,
            weak_bg_fill: ACCENT_SOFT,
            bg_stroke: egui::Stroke::new(1.0, ACCENT),
            rounding: egui::Rounding::same(8.0),
            fg_stroke: egui::Stroke::new(1.0, ACCENT_DARK),
            expansion: 0.0,
        },
        open: egui::style::WidgetVisuals {
            bg_fill: ACCENT_SOFT,
            weak_bg_fill: ACCENT_SOFT,
            bg_stroke: egui::Stroke::new(1.0, ACCENT),
            rounding: egui::Rounding::same(8.0),
            fg_stroke: egui::Stroke::new(1.0, ACCENT_DARK),
            expansion: 0.0,
        },
    };

    style.visuals = egui::Visuals {
        window_fill: WHITE,
        panel_fill: WHITE,
        widgets,
        selection: egui::style::Selection {
            bg_fill: ACCENT.linear_multiply(0.25),
            stroke: egui::Stroke::new(1.0, ACCENT),
        },
        hyperlink_color: ACCENT,
        faint_bg_color: SURFACE,
        dark_mode: false,
        ..Default::default()
    };

    ctx.set_style(style);
}

/// 加载系统中文字体作为 egui 的首选字形来源。
///
/// 返回 false 时界面会显示「缺少中文字体」提示，而不是静默显示方块。
/// TTC 字体集合通过 `FontData::index` 显式指定 face（默认 0 为常规字重）。
fn setup_chinese_font(ctx: &egui::Context) -> bool {
    #[cfg(target_os = "macos")]
    let candidates: &[(&str, u32)] = &[
        ("/System/Library/Fonts/PingFang.ttc", 0),
        ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
        ("/System/Library/Fonts/STHeiti Light.ttc", 0),
        ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
        ("/System/Library/Fonts/Songti.ttc", 0),
    ];

    #[cfg(target_os = "windows")]
    let candidates: &[(&str, u32)] = &[
        ("C:\\Windows\\Fonts\\msyh.ttc", 0),
        ("C:\\Windows\\Fonts\\msyhbd.ttc", 0),
        ("C:\\Windows\\Fonts\\simsun.ttc", 0),
        ("C:\\Windows\\Fonts\\simfang.ttf", 0),
        ("C:\\Windows\\Fonts\\msyhl.ttc", 0),
    ];

    #[cfg(target_os = "linux")]
    let candidates: &[(&str, u32)] = &[
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
        ("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf", 0),
    ];

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let candidates: &[(&str, u32)] = &[];

    let Some((bytes, index)) = candidates
        .iter()
        .find_map(|(path, index)| std::fs::read(path).ok().map(|data| (data, *index)))
    else {
        return false;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "CJK".to_owned(),
        egui::FontData {
            font: std::borrow::Cow::Owned(bytes),
            index,
            tweak: Default::default(),
        },
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "CJK".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.push("CJK".to_owned());
    }
    ctx.set_fonts(fonts);
    true
}
