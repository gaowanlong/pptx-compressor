//! 界面状态与用户偏好的回归测试（不依赖 GUI 渲染）。

use pptx_compressor::app::{AppState, PersistedState};
use pptx_compressor::core::{crf_from_quality, CompressionSettings, MediaType, Preset};

#[test]
fn preset_selection_is_stable_and_does_not_leak_scaling() {
    let mut settings = CompressionSettings::default();
    assert_eq!(settings.preset(), Preset::Balanced);

    settings.apply_preset(Preset::Small);
    assert_eq!(settings.preset(), Preset::Small);
    assert!(settings.image_max_width_enabled);
    assert_eq!(settings.image_max_width, 1024);
    assert!(settings.video_max_resolution_enabled);
    assert_eq!(settings.video_max_height, 720);

    // 从「最小体积」切回「均衡」时，旧的缩放开关必须被清掉，
    // 否则分段控件显示为均衡、实际仍按 1024px / 720p 压缩。
    settings.apply_preset(Preset::Balanced);
    assert_eq!(settings.preset(), Preset::Balanced);
    assert!(!settings.image_max_width_enabled);
    assert_eq!(settings.image_max_width, 1920);
    assert!(!settings.video_max_resolution_enabled);
    assert_eq!(settings.video_max_height, 1080);
    assert_eq!(settings.crf(), 29);

    settings.apply_preset(Preset::High);
    assert_eq!(settings.preset(), Preset::High);
    assert_eq!(settings.crf(), 18);

    settings.image_quality = 77;
    assert_eq!(settings.preset(), Preset::Custom);
}

#[test]
fn crf_mapping_matches_ui_hint() {
    assert_eq!(crf_from_quality(0), 51);
    assert_eq!(crf_from_quality(100), 0);
    assert_eq!(crf_from_quality(45), 29);
    assert_eq!(crf_from_quality(65), 18);
    assert_eq!(crf_from_quality(18), 42);
}

#[test]
fn persisted_preferences_roundtrip() {
    let mut state = PersistedState::default();
    state.settings.apply_preset(Preset::Small);
    state.show_advanced = true;
    state.output_dir = Some(std::path::PathBuf::from("/tmp/pptx-out"));
    state.ffmpeg_override = Some("/usr/local/bin/ffmpeg".into());

    let raw = serde_json::to_string(&state).expect("serialize");
    let restored: PersistedState = serde_json::from_str(&raw).expect("deserialize");

    assert_eq!(restored.settings.preset(), Preset::Small);
    assert_eq!(restored.settings.video_quality, 18);
    assert!(restored.show_advanced);
    assert_eq!(
        restored.output_dir,
        Some(std::path::PathBuf::from("/tmp/pptx-out"))
    );
    assert_eq!(restored.ffmpeg_override.as_deref(), Some("/usr/local/bin/ffmpeg"));
}

#[test]
fn legacy_preference_file_still_loads() {
    // v1.1.0 之前没有偏好文件；未来新增字段时旧 JSON 也不能让应用崩溃。
    let legacy = r#"{"show_advanced":true}"#;
    let restored: PersistedState = serde_json::from_str(legacy).expect("legacy deserialize");
    assert!(restored.show_advanced);
    assert_eq!(restored.settings.preset(), Preset::Balanced);
    assert!(restored.output_dir.is_none());
    assert!(restored.ffmpeg_override.is_none());
}

#[test]
fn reset_clears_input_warnings_but_keeps_preferences() {
    use pptx_compressor::app::PptxCompressorApp;

    let mut app = PptxCompressorApp::default();
    app.settings.apply_preset(Preset::Small);
    app.output_dir = Some(std::path::PathBuf::from("/tmp/pptx-out"));
    app.input_files = vec![std::path::PathBuf::from("/tmp/demo.pptx")];
    app.input_warnings = vec!["包含数字签名，压缩后签名会失效".into()];
    app.activity_log.push("已解析文稿".into());
    app.source_size = 48_600_000;

    app.reset_for_new_file();

    assert_eq!(app.source_size, 0, "源文稿体积应随文件一起清空");
    assert!(app.input_warnings.is_empty());
    assert!(app.input_files.is_empty());
    assert!(app.activity_log.is_empty());
    assert_eq!(app.settings.preset(), Preset::Small, "用户偏好不应被重置");
    assert_eq!(
        app.output_dir,
        Some(std::path::PathBuf::from("/tmp/pptx-out")),
        "输出目录不应被重置"
    );
}

#[test]
fn chinese_status_and_media_labels_are_wired() {
    assert_eq!(AppState::Idle.badge_text(0), "等待文件");
    assert_eq!(AppState::Compressing.badge_text(42), "压缩中 42%");
    assert_eq!(AppState::Done.badge_text(100), "已完成");
    assert_eq!(AppState::Error.badge_text(0), "处理失败");
    assert_eq!(MediaType::Video.label_zh(), "视频");
    assert_eq!(MediaType::Other.label_zh(), "其他");
    assert_eq!(Preset::Custom.label(), "自定义");
}
