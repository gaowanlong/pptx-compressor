//! End-to-end pipeline tests over real temporary files (no GUI needed).

use pptx_compressor::core::pipeline::run_pipeline;
use pptx_compressor::core::{CompressMessage, CompressionSettings, MediaInfo, MediaStatus, MediaType};
use image::ImageEncoder as _;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use tempfile::tempdir;
use zip::ZipArchive;

fn make_jpeg(path: &Path, quality: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(320, 240);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([
            ((x * 7 + y * 11) % 256) as u8,
            ((x * 13 + y * 3) % 256) as u8,
            ((x * 5 + y * 17) % 256) as u8,
        ]);
    }
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    encoder
        .write_image(img.as_raw(), 320, 240, image::ExtendedColorType::Rgb8)
        .unwrap();
    let data = buf.into_inner();
    fs::write(path, &data).unwrap();
    data
}

fn write_package_skeleton(root: &Path, media_name: &str, content_type: &str) {
    fs::create_dir_all(root.join("ppt/media")).unwrap();
    fs::create_dir_all(root.join("ppt/slides/_rels")).unwrap();
    fs::create_dir_all(root.join("docProps")).unwrap();
    fs::create_dir_all(root.join("_rels")).unwrap();

    let ct = format!(
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/ppt/media/{}" ContentType="{}"/><Override PartName="/docProps/thumbnail.jpeg" ContentType="image/jpeg"/></Types>"#,
        media_name, content_type
    );
    fs::write(root.join("[Content_Types].xml"), ct).unwrap();

    let root_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="thumbnail" Target="docProps/thumbnail.jpeg"/><Relationship Id="rId2" Type="officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    fs::write(root.join("_rels/.rels"), root_rels).unwrap();

    let slide_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="media" Target="../media/{}"/></Relationships>"#,
        media_name
    );
    fs::write(root.join("ppt/slides/_rels/slide1.xml.rels"), slide_rels).unwrap();
    fs::write(root.join("docProps/thumbnail.jpeg"), b"fake-thumbnail").unwrap();
}

fn run(root: &Path, media: &mut [MediaInfo], settings: &CompressionSettings) -> (PathBuf, Vec<CompressMessage>) {
    let output = root.parent().unwrap().join("out.pptx");
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    run_pipeline(root, &output, media, settings, 1_000_000, tx, cancel).unwrap();
    (output, rx.try_iter().collect())
}

fn zip_bytes(path: &Path, name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).unwrap();
    buf
}

fn zip_text(path: &Path, name: &str) -> String {
    String::from_utf8(zip_bytes(path, name)).unwrap()
}

#[test]
fn larger_candidate_never_replaces_original() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    write_package_skeleton(&root, "small.jpg", "image/jpeg");
    let original = make_jpeg(&root.join("ppt/media/small.jpg"), 20);

    // 重编码 q20 JPEG 只会变大，用来验证「变大不替换」。
    let settings = CompressionSettings {
        image_quality: 95,
        ..CompressionSettings::default()
    };
    let mut media = vec![MediaInfo::new(
        "ppt/media/small.jpg".into(),
        root.join("ppt/media/small.jpg"),
        MediaType::Jpeg,
    )];
    let (out, messages) = run(&root, &mut media, &settings);

    assert_eq!(fs::read(root.join("ppt/media/small.jpg")).unwrap(), original);
    assert!(messages.iter().any(|m| matches!(
        m,
        CompressMessage::StatusUpdate(_, MediaStatus::Skipped, _)
    )));
    assert!(messages.iter().any(|m| matches!(m, CompressMessage::Finished(..))));
    assert_eq!(zip_bytes(&out, "ppt/media/small.jpg"), original);
}

#[test]
fn smaller_candidate_replaces_and_output_is_valid() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    write_package_skeleton(&root, "big.jpg", "image/jpeg");
    let original = make_jpeg(&root.join("ppt/media/big.jpg"), 95);

    let settings = CompressionSettings {
        image_quality: 20,
        ..CompressionSettings::default()
    };
    let mut media = vec![MediaInfo::new(
        "ppt/media/big.jpg".into(),
        root.join("ppt/media/big.jpg"),
        MediaType::Jpeg,
    )];
    let (out, messages) = run(&root, &mut media, &settings);

    assert!(fs::metadata(root.join("ppt/media/big.jpg")).unwrap().len() < original.len() as u64);
    assert!(messages.iter().any(|m| matches!(
        m,
        CompressMessage::StatusUpdate(_, MediaStatus::Done, _)
    )));

    let mut archive = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    assert_eq!(archive.by_index(0).unwrap().name(), "[Content_Types].xml");
    assert!(archive.by_name("docProps/thumbnail.jpeg").is_err());
    assert!(!zip_text(&out, "[Content_Types].xml").contains("thumbnail"));
    assert!(!zip_text(&out, "_rels/.rels").contains("thumbnail"));
}

#[test]
fn parallel_media_processing_is_complete_and_deterministic() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    write_package_skeleton(&root, "m0.jpg", "image/jpeg");

    let mut originals = Vec::new();
    let mut media = Vec::new();
    for i in 0..6 {
        let name = format!("m{i}.jpg");
        let path = root.join("ppt/media").join(&name);
        // 偶数：高质量源会被默认 q75 压小；奇数：q20 源重编码只会变大。
        let quality = if i % 2 == 0 { 98 } else { 20 };
        originals.push(make_jpeg(&path, quality));
        media.push(MediaInfo::new(
            format!("ppt/media/{name}"),
            path,
            MediaType::Jpeg,
        ));
    }

    let (out, messages) = run(&root, &mut media, &CompressionSettings::default());

    // 每个媒体都收到最终状态，且按索引升序：并行只发生在候选生成阶段，
    // 裁决与日志仍在串行阶段完成，因此顺序不受线程调度影响。
    let finals: Vec<usize> = messages
        .iter()
        .filter_map(|m| match m {
            CompressMessage::StatusUpdate(
                idx,
                MediaStatus::Done | MediaStatus::Skipped | MediaStatus::Failed,
                _,
            ) => Some(*idx),
            _ => None,
        })
        .collect();
    assert_eq!(finals, (0..6).collect::<Vec<_>>(), "最终状态数量或顺序不对");

    let progresses: Vec<usize> = messages
        .iter()
        .filter_map(|m| match m {
            CompressMessage::Progress(current, _, _) => Some(*current),
            _ => None,
        })
        .collect();
    assert!(progresses.len() >= 6, "进度消息不足：{progresses:?}");
    assert!(progresses.contains(&6), "进度没有推进到全部任务完成：{progresses:?}");

    for (i, original) in originals.iter().enumerate() {
        let bytes = zip_bytes(&out, &format!("ppt/media/m{i}.jpg"));
        if i % 2 == 0 {
            assert_ne!(&bytes, original, "m{i}.jpg 应当被更小的候选替换");
        } else {
            assert_eq!(&bytes, original, "m{i}.jpg 变大后必须保留原件");
        }
    }
    assert!(messages.iter().any(|m| matches!(m, CompressMessage::Finished(..))));
}

/// 两个视频用例都会改写会话级的 FFmpeg 路径，串行执行避免互相干扰。
#[cfg(unix)]
static FFMPEG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 一个「假的 FFmpeg」：实现 -version / -encoders 与编码调用，
/// 用来在没有真实 FFmpeg 的机器上验证整条视频改名与 OOXML 同步链路。
#[cfg(unix)]
fn write_mock_ffmpeg(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("ffmpeg");
    let script = r#"#!/bin/sh
case "$1" in
  -version)
    echo 'ffmpeg version 99.0-mock'
    echo 'configuration: --disable-gpl --disable-nonfree --enable-libopenh264 --enable-libx264'
    exit 0 ;;
  -hide_banner)
    echo ' V..... libx264      H.264 / AVC (codec h264)'
    echo ' V..... libopenh264  OpenH264 (codec h264)'
    exit 0 ;;
esac
last=""
for arg in "$@"; do last="$arg"; done
printf 'MOCK-MP4-PAYLOAD' > "$last"
exit 0
"#;
    fs::write(&path, script).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn video_conversion_with_mock_ffmpeg_renames_and_updates_declarations() {
    let _guard = FFMPEG_TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let mock = write_mock_ffmpeg(dir.path());
    pptx_compressor::utils::ffmpeg::set_preferred(Some(mock.to_string_lossy().into_owned()));

    let root = dir.path().join("pkg");
    write_package_skeleton(&root, "clip.mov", "video/quicktime");
    let clip = root.join("ppt/media/clip.mov");
    fs::write(&clip, vec![0xABu8; 4096]).unwrap();

    let mut media = vec![MediaInfo::new(
        "ppt/media/clip.mov".into(),
        clip.clone(),
        MediaType::Video,
    )];
    let (out, messages) = run(&root, &mut media, &CompressionSettings::default());

    assert!(
        messages.iter().any(
            |m| matches!(m, CompressMessage::StatusUpdate(_, MediaStatus::Done, _))
        ),
        "视频未被替换：{:?}",
        messages
    );
    assert!(root.join("ppt/media/clip.mp4").exists());
    assert!(!clip.exists());
    assert_eq!(zip_bytes(&out, "ppt/media/clip.mp4"), b"MOCK-MP4-PAYLOAD");

    let ct = zip_text(&out, "[Content_Types].xml");
    assert!(ct.contains("PartName=\"/ppt/media/clip.mp4\""));
    assert!(ct.contains("ContentType=\"video/mp4\""));
    assert!(ct.contains("Extension=\"mp4\""));
    let rels = zip_text(&out, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("../media/clip.mp4"));
    assert!(!rels.contains("clip.mov"));

    pptx_compressor::utils::ffmpeg::set_preferred(None);
}

#[test]
#[ignore = "requires a user-supplied FFmpeg with an H.264 encoder"]
fn video_conversion_renames_and_updates_declarations() {
    #[cfg(unix)]
    let _guard = FFMPEG_TEST_LOCK.lock().unwrap();
    // v1.2.0 起不再随包分发 FFmpeg：用例改为使用本机 FFmpeg，
    // 没装时明确跳过（不会伪装成通过），见 FFMPEG_SETUP.md。
    let Some(ffmpeg) = pptx_compressor::utils::ffmpeg::find_ffmpeg() else {
        eprintln!("本机未找到 FFmpeg，跳过视频端到端用例（参见 FFMPEG_SETUP.md）");
        return;
    };
    pptx_compressor::utils::ffmpeg::set_preferred(Some(ffmpeg.clone()));

    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    write_package_skeleton(&root, "clip.mov", "video/quicktime");
    let clip = root.join("ppt/media/clip.mov");
    let status = std::process::Command::new(&ffmpeg)
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=320x240:rate=15",
            "-c:v",
            "mpeg4",
            "-q:v",
            "2",
        ])
        .arg(&clip)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "failed to generate the test video");

    let settings = CompressionSettings::default();
    let mut media = vec![MediaInfo::new(
        "ppt/media/clip.mov".into(),
        clip.clone(),
        MediaType::Video,
    )];
    let (out, messages) = run(&root, &mut media, &settings);
    assert!(
        messages.iter().any(|m| matches!(
            m,
            CompressMessage::StatusUpdate(_, MediaStatus::Done, _)
        )),
        "video was not replaced: {:?}",
        messages
    );

    assert!(root.join("ppt/media/clip.mp4").exists());
    assert!(!clip.exists());

    let mut archive = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    assert!(archive.by_name("ppt/media/clip.mp4").is_ok());
    assert!(archive.by_name("ppt/media/clip.mov").is_err());

    let ct = zip_text(&out, "[Content_Types].xml");
    assert!(ct.contains("PartName=\"/ppt/media/clip.mp4\""));
    assert!(ct.contains("ContentType=\"video/mp4\""));
    assert!(ct.contains("Extension=\"mp4\""));

    let rels = zip_text(&out, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("../media/clip.mp4"));
    assert!(!rels.contains("clip.mov"));

    pptx_compressor::utils::ffmpeg::set_preferred(None);
}
