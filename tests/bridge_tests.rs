//! 鸿蒙桥接协议回归测试。
//!
//! 在 macOS/Windows 主机上模拟鸿蒙的能力边界（无 FFmpeg 子进程、沙箱临时目录），
//! 走完 ArkTS 会调用的全部三类任务：inspect → compress → batch。
//! 这样无需真机也能验证「数据格式一致 + 核心功能覆盖」。

use image::ImageEncoder as _;
use pptx_compressor::core::bridge::{self, BridgeEvent, PROTOCOL_VERSION};
use pptx_compressor::core::platform::{self, SandboxPaths};
use pptx_compressor::core::state::{PersistedState, STORAGE_KEY};
use pptx_compressor::core::CompressionSettings;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="jpeg" ContentType="image/jpeg"/>
  <Default Extension="jpg" ContentType="image/jpeg"/>
  <Default Extension="mp4" ContentType="video/mp4"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
</Types>"#;

fn make_jpeg(quality: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(240, 180);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([
            ((x * 5 + y * 7) % 256) as u8,
            ((x * 11 + y * 3) % 256) as u8,
            ((x * 13 + y * 2) % 256) as u8,
        ]);
    }
    let mut buf = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .write_image(img.as_raw(), 240, 180, image::ExtendedColorType::Rgb8)
        .unwrap();
    buf.into_inner()
}

fn make_pptx(path: &Path, quality: u8) {
    let file = fs::File::create(path).unwrap();
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let parts: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes().to_vec()),
        (
            "_rels/.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="officeDocument" Target="ppt/presentation.xml"/></Relationships>"#.to_vec(),
        ),
        (
            "ppt/presentation.xml",
            br#"<?xml version="1.0"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#.to_vec(),
        ),
        ("ppt/media/image1.jpg", make_jpeg(quality)),
        ("ppt/media/clip.mp4", vec![0x5Au8; 4096]),
    ];
    for (name, data) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(&data).unwrap();
    }
    writer.finish().unwrap();
}

fn wait_job(job_id: u64) -> Vec<BridgeEvent> {
    let started = Instant::now();
    let mut collected = Vec::new();
    loop {
        let reply = bridge::poll_events(job_id);
        collected.extend(reply.events);
        if reply.done {
            return collected;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "桥接任务超时未完成"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn start(request: &str) -> u64 {
    bridge::start_job(request)
        .expect("启动桥接任务失败")
        .job_id
}

fn is_valid_pptx(path: &str) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return false;
    };
    let has_types = archive.by_name("[Content_Types].xml").is_ok();
    let mut readable = true;
    for i in 0..archive.len() {
        if let Ok(mut entry) = archive.by_index(i) {
            let mut sink = Vec::new();
            if entry.read_to_end(&mut sink).is_err() {
                readable = false;
            }
        }
    }
    has_types && readable
}

#[test]
fn protocol_json_shape_matches_desktop() {
    assert_eq!(STORAGE_KEY, "pptx-compressor.state.v1");
    let raw = serde_json::to_string(&PersistedState::default()).unwrap();
    assert!(raw.contains("\"image_quality\":75"));
    assert!(raw.contains("\"video_quality\":45"));
    assert!(raw.contains("\"ffmpeg_override\":null"));

    // 旧版本偏好缺字段仍可解析（serde(default) 语义不能退化）。
    let legacy = r#"{"settings":{"image_quality":60},"output_dir":null}"#;
    let restored = PersistedState::from_json(legacy);
    assert_eq!(restored.settings.image_quality, 60);
    assert_eq!(restored.settings.video_quality, 45);

    let event = BridgeEvent::JobFinished {
        index: 0,
        original: 100,
        compressed: 60,
        output_path: "/tmp/a.pptx".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"job_finished\""));
    assert!(json.contains("\"output_path\":\"/tmp/a.pptx\""));
    assert_eq!(bridge::capabilities().protocol_version, PROTOCOL_VERSION);
}

#[test]
fn unknown_job_and_bad_request_are_structured_errors() {
    let reply = bridge::poll_events(999_999);
    assert!(reply.done);
    assert!(matches!(reply.events.first(), Some(BridgeEvent::Fatal { .. })));
    assert!(!bridge::cancel_job(999_999));
    assert!(bridge::start_job("{not json").is_err());
}

#[test]
fn harmony_sandbox_flow_inspect_compress_batch() {
    let root = tempdir().unwrap();
    let sandbox = SandboxPaths {
        data_dir: root.path().join("files"),
        cache_dir: root.path().join("cache"),
        temp_dir: root.path().join("cache/tmp"),
    };
    platform::init_sandbox(sandbox).unwrap();
    // 模拟鸿蒙 HAP：没有可执行的外部 FFmpeg。
    platform::set_video_transcode_available(false);
    assert!(!bridge::capabilities().video_transcode);

    let input_a = root.path().join("deck-a.pptx");
    let input_b = root.path().join("deck-b.pptx");
    make_pptx(&input_a, 95);
    make_pptx(&input_b, 92);
    let out_dir = root.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();

    // ── 1. inspect：解包、安全检测、媒体清单与能力降级提示 ──
    let inspect = format!(
        r#"{{"mode":"inspect","path":{}}}"#,
        serde_json::to_string(&input_a.to_string_lossy()).unwrap()
    );
    let events = wait_job(start(&inspect));
    let inspected = events
        .iter()
        .find_map(|e| match e {
            BridgeEvent::Inspected { .. } => Some(e.clone()),
            _ => None,
        })
        .expect("缺少 Inspected 事件");
    let (session_id, media, warnings, composition) = match inspected {
        BridgeEvent::Inspected {
            session_id,
            media,
            warnings,
            composition,
            ..
        } => (session_id, media, warnings, composition),
        _ => unreachable!(),
    };
    assert!(media.iter().any(|m| m.kind == pptx_compressor::core::MediaType::Jpeg));
    let video = media
        .iter()
        .find(|m| m.kind == pptx_compressor::core::MediaType::Video)
        .expect("应识别到视频项");
    assert!(!video.enabled, "鸿蒙侧视频项必须默认禁用");
    assert!(warnings.iter().any(|w| w.contains("FFmpeg")));
    assert_eq!(composition.total_count, media.len());
    assert!(
        composition.optimizable_bytes <= composition.media_bytes,
        "可优化上界不能超过媒体总体积"
    );

    // ── 2. compress：即便 UI 强行打开视频开关，内核也必须挡住 ──
    let enabled: Vec<bool> = media.iter().map(|_| true).collect();
    let settings = serde_json::to_string(&CompressionSettings::default()).unwrap();
    let compress = format!(
        r#"{{"mode":"compress","session_id":{},"output_dir":{},"settings":{},"enabled":{}}}"#,
        session_id,
        serde_json::to_string(&out_dir.to_string_lossy()).unwrap(),
        settings,
        serde_json::to_string(&enabled).unwrap(),
    );
    let events = wait_job(start(&compress));
    let output_path: PathBuf = events
        .iter()
        .find_map(|e| match e {
            BridgeEvent::JobFinished { output_path, .. } => Some(PathBuf::from(output_path)),
            _ => None,
        })
        .expect("缺少 JobFinished 事件");
    assert!(output_path.exists(), "未生成压缩结果");
    assert!(is_valid_pptx(output_path.to_str().unwrap()), "输出 PPTX 不可读");
    assert!(events.iter().any(|e| matches!(e, BridgeEvent::Finished { .. })));
    assert!(
        events.iter().all(|e| !matches!(e, BridgeEvent::Fatal { .. })),
        "压缩过程不应出现致命错误"
    );

    // ── 3. batch：两个文件独立处理并给出汇总 ──
    let inputs = vec![
        input_a.to_string_lossy().into_owned(),
        input_b.to_string_lossy().into_owned(),
    ];
    let batch = format!(
        r#"{{"mode":"batch","inputs":{},"output_dir":{},"settings":{}}}"#,
        serde_json::to_string(&inputs).unwrap(),
        serde_json::to_string(&out_dir.to_string_lossy()).unwrap(),
        settings,
    );
    let events = wait_job(start(&batch));
    let finished_count = events
        .iter()
        .filter(|e| matches!(e, BridgeEvent::JobFinished { .. }))
        .count();
    assert_eq!(finished_count, 2, "批量队列应产出两个成功结果");
    let summary = events
        .iter()
        .find_map(|e| match e {
            BridgeEvent::Finished { summary } => Some(*summary),
            _ => None,
        })
        .expect("缺少 Finished 汇总");
    assert_eq!(summary.total, 2);
    assert_eq!(summary.succeeded, 2);
    assert_eq!(summary.failed, 0);
    assert!(summary.total_compressed > 0);

    // 会话已在 compress 时被取走，这里只验证回收调用不会 panic。
    let _ = bridge::drop_session(session_id);
    platform::set_video_transcode_available(true);
}
