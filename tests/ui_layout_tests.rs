//! 无窗口 UI 冒烟测试：在 egui 测试上下文里渲染所有面板的六种状态。
//!
//! 编译通过不代表布局不会在运行时 panic（例如空数据、零宽度、
//! `ui.output_mut` 与 `Grid` 列数不匹配），这里逐帧跑一遍真实绘制路径。

use pptx_compressor::app::{AppState, PptxCompressorApp};
use pptx_compressor::core::{MediaInfo, MediaStatus, MediaType};
use pptx_compressor::ui::{file_panel, media_list, progress, settings};
use std::path::PathBuf;

fn app_with_media(state: AppState) -> PptxCompressorApp {
    let mut media = MediaInfo::new(
        "ppt/media/image1.jpg".into(),
        PathBuf::from("/tmp/示例文稿.pptx"),
        MediaType::Jpeg,
    );
    media.original_size = 2_000_000;
    media.compressed_size = 800_000;
    media.status = match state {
        AppState::Done => MediaStatus::Done,
        AppState::Error => MediaStatus::Failed,
        AppState::Compressing => MediaStatus::Processing,
        _ => MediaStatus::Pending,
    };

    PptxCompressorApp {
        state,
        input_files: vec![PathBuf::from("/tmp/示例文稿.pptx")],
        media_files: vec![media],
        progress_current: 1,
        progress_total: 4,
        progress_file: "image1.jpg".into(),
        result_original: 10_000_000,
        result_compressed: 6_000_000,
        output_path: Some(PathBuf::from("/tmp/示例文稿_compressed.pptx")),
        ffmpeg_available: true,
        activity_log: vec!["压缩 image1.jpg 失败：示例日志".into()],
        ..PptxCompressorApp::default()
    }
}

#[test]
fn every_panel_renders_in_all_six_states() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1080.0, 740.0),
        )),
        ..Default::default()
    };

    for state in [
        AppState::Idle,
        AppState::Loading,
        AppState::Ready,
        AppState::Compressing,
        AppState::Done,
        AppState::Error,
    ] {
        let mut app = app_with_media(state);
        let _output = ctx.run(input.clone(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                settings::show(ui, &mut app);
                media_list::show(ui, &mut app);
                progress::show(ui, &mut app);
                progress::show_notes(ui, &mut app);
                file_panel::show(ui, &mut app);
            });
        });
    }
}

#[test]
fn empty_media_list_and_advanced_panel_render() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        ..Default::default()
    };

    let mut app = PptxCompressorApp {
        show_advanced: true,
        show_only_compressible: true,
        font_ok: false,
        ffmpeg_available: false,
        ..PptxCompressorApp::default()
    };

    let _output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            settings::show(ui, &mut app);
            media_list::show(ui, &mut app);
            progress::show(ui, &mut app);
            progress::show_notes(ui, &mut app);
            file_panel::show(ui, &mut app);
        });
    });
}
