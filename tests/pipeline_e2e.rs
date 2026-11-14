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

fn run(root: &Path, media: &mut Vec<MediaInfo>, settings: &CompressionSettings) -> (PathBuf, Vec<CompressMessage>) {
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

    let mut settings = CompressionSettings::default();
    settings.image_quality = 95; // re-encoding a q20 JPEG will be larger
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

    let mut settings = CompressionSettings::default();
    settings.image_quality = 20;
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
#[ignore = "requires the bundled FFmpeg binary"]
fn video_conversion_renames_and_updates_declarations() {
    let ffmpeg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources/ffmpeg/macos/ffmpeg");
    if !ffmpeg.exists() {
        eprintln!("Bundled FFmpeg not present, skipping");
        return;
    }

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
}
