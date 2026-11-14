//! 无窗口 UI 冒烟测试：在 egui 测试上下文里渲染所有面板的六种状态。
//!
//! 编译通过不代表布局不会在运行时 panic（例如空数据、零宽度、
//! `ui.output_mut` 与 `Grid` 列数不匹配），这里逐帧跑一遍真实绘制路径。

use pptx_compressor::app::{AppState, PptxCompressorApp};
use pptx_compressor::core::queue::BatchQueue;
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
fn input_warning_notices_render() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        ..Default::default()
    };

    let mut app = app_with_media(AppState::Ready);
    app.input_warnings = vec![
        "包含数字签名，压缩后签名会失效，建议先另存副本".into(),
        "包含 VBA 宏，建议另存为普通 .pptx 后再压缩".into(),
        "包含 OLE 嵌入对象，压缩后请先在 PowerPoint 中验证".into(),
        "演示文稿设置了修改密码或只读保护".into(),
    ];

    let _output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            file_panel::show(ui, &mut app);
        });
    });
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

/// 批量队列视图在就绪 / 压缩中 / 完成 / 失败四态下都要能完整绘制，
/// 覆盖「已完成 / 失败 / 处理中」混合状态与输入告警卡片。
#[test]
fn batch_queue_renders_in_all_states() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1080.0, 740.0),
        )),
        ..Default::default()
    };
    let paths: Vec<PathBuf> = (0..3)
        .map(|i| PathBuf::from(format!("/tmp/批量文稿{i}.pptx")))
        .collect();

    for state in [
        AppState::Ready,
        AppState::Compressing,
        AppState::Done,
        AppState::Error,
    ] {
        let mut queue = BatchQueue::from_paths(&paths);
        queue.mark_done(
            0,
            2_000_000,
            1_200_000,
            PathBuf::from("/tmp/批量文稿0_compressed.pptx"),
        );
        queue.mark_failed(1, "这是旧版 .ppt 或加密的复合文档".into());
        queue.mark_running(2);
        queue.set_media_count(2, 6);
        queue.set_warnings(2, vec!["包含数字签名，压缩后签名会失效".into()]);

        let mut app = PptxCompressorApp {
            state,
            input_files: paths.clone(),
            queue: Some(queue),
            progress_current: 1,
            progress_total: 3,
            progress_file: "第 2/3 个 · 3/6 · image1.jpg".into(),
            batch_current_media: (3, 6),
            result_original: 2_000_000,
            result_compressed: 1_200_000,
            output_path: Some(PathBuf::from("/tmp/批量文稿0_compressed.pptx")),
            activity_log: vec!["压缩 image1.jpg 失败：示例日志".into()],
            ..PptxCompressorApp::default()
        };

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

/// 队列全部待处理、无日志、字体缺失、FFmpeg 不可用的边界组合。
#[test]
fn batch_queue_pending_rows_render_without_any_result() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        ..Default::default()
    };
    let paths = vec![
        PathBuf::from("/tmp/甲.pptx"),
        PathBuf::from("/tmp/乙.pptx"),
    ];
    let mut app = PptxCompressorApp {
        state: AppState::Ready,
        input_files: paths.clone(),
        queue: Some(BatchQueue::from_paths(&paths)),
        progress_total: 2,
        font_ok: false,
        ffmpeg_available: false,
        show_advanced: true,
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
