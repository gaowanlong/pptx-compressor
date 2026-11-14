//! v1.2.5 体积构成：真实临时 PPTX / ZIP 端到端回归。
//! 不依赖 GUI 与 FFmpeg。

use image::ImageEncoder as _;
use pptx_compressor::core::estimate::{actual_media_saving, MediaComposition};
use pptx_compressor::core::extractor::extract_pptx;
use pptx_compressor::core::pipeline::run_pipeline;
use pptx_compressor::core::{CompressMessage, CompressionSettings, MediaInfo, MediaType, Preset};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use tempfile::tempdir;
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, ZipArchive};

const CT: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpg" ContentType="image/jpeg"/><Default Extension="png" ContentType="image/png"/><Default Extension="bin" ContentType="application/octet-stream"/></Types>"#;
const ROOT_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
const PRESENTATION: &str = r#"<?xml version="1.0"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#;

fn make_jpeg(quality: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(320, 240);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([
            ((x * 7 + y * 11) % 256) as u8,
            ((x * 13 + y * 3) % 256) as u8,
            ((x * 5 + y * 17) % 256) as u8,
        ]);
    }
    let mut buf = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .write_image(img.as_raw(), 320, 240, image::ExtendedColorType::Rgb8)
        .unwrap();
    buf.into_inner()
}

fn make_png() -> Vec<u8> {
    let mut img = image::RgbImage::new(240, 180);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([
            ((x * 3 + y * 5) % 256) as u8,
            ((x * 11 + y) % 256) as u8,
            ((x + y * 9) % 256) as u8,
        ]);
    }
    let mut buf = Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(img.as_raw(), 240, 180, image::ExtendedColorType::Rgb8)
        .unwrap();
    buf.into_inner()
}

struct Fixture {
    jpeg: Vec<u8>,
    png: Vec<u8>,
    other: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            jpeg: make_jpeg(95),
            png: make_png(),
            other: vec![0xAB; 4_096],
        }
    }
}

/// 写一个真实、可被输入检测与解包流程接受的最小 PPTX（ZIP）。
fn write_pptx(path: &Path, fixture: Option<&Fixture>) -> u64 {
    let file = fs::File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);

    let mut add = |name: &str, data: &[u8]| {
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file(name, opts).unwrap();
        zip.write_all(data).unwrap();
    };
    add("[Content_Types].xml", CT.as_bytes());
    add("_rels/.rels", ROOT_RELS.as_bytes());
    add("ppt/presentation.xml", PRESENTATION.as_bytes());
    if let Some(f) = fixture {
        add("ppt/media/photo.jpg", &f.jpeg);
        add("ppt/media/chart.png", &f.png);
        add("ppt/media/notes.bin", &f.other);
    }
    zip.finish().unwrap();
    fs::metadata(path).unwrap().len()
}

fn unpack(pptx: &Path, dir: &Path) -> Vec<MediaInfo> {
    extract_pptx(pptx, dir).expect("解包应成功")
}

fn run(
    root: &Path,
    output: &Path,
    media: &mut [MediaInfo],
    settings: &CompressionSettings,
    package_size: u64,
) -> Vec<CompressMessage> {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    run_pipeline(root, output, media, settings, package_size, tx, cancel).unwrap();
    rx.try_iter().collect()
}

fn valid_zip(path: &Path) -> ZipArchive<fs::File> {
    ZipArchive::new(fs::File::open(path).unwrap()).unwrap()
}

#[test]
fn composition_matches_actual_bytes_in_a_real_pptx() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::new();
    let pptx = dir.path().join("source.pptx");
    let package_size = write_pptx(&pptx, Some(&fixture));
    let root = dir.path().join("unpacked");
    let media = unpack(&pptx, &root);

    assert_eq!(media.len(), 3);
    let c = MediaComposition::from_media(&media, package_size);

    assert_eq!(c.package_size, package_size);
    assert_eq!(c.package_size, fs::metadata(&pptx).unwrap().len());
    assert_eq!(c.total_count, 3);
    assert_eq!(
        c.media_bytes,
        (fixture.jpeg.len() + fixture.png.len() + fixture.other.len()) as u64
    );
    assert_eq!(c.by_type(MediaType::Jpeg).bytes, fixture.jpeg.len() as u64);
    assert_eq!(c.by_type(MediaType::Png).bytes, fixture.png.len() as u64);
    assert_eq!(c.by_type(MediaType::Other).bytes, fixture.other.len() as u64);
    assert_eq!(c.by_type(MediaType::Video).count, 0);
    assert_eq!(c.optimizable_count, 2);
    assert_eq!(
        c.saving_upper_bound(),
        (fixture.jpeg.len() + fixture.png.len()) as u64
    );
    assert!(!c.headline().contains("预计"));
    assert!(c.breakdown().contains("JPEG"));
    assert!(c.breakdown().contains("PNG"));
}

#[test]
fn real_saving_after_pipeline_stays_within_the_upper_bound() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::new();
    let pptx = dir.path().join("source.pptx");
    let package_size = write_pptx(&pptx, Some(&fixture));
    let root = dir.path().join("unpacked");
    let mut media = unpack(&pptx, &root);
    let c = MediaComposition::from_media(&media, package_size);

    let settings = CompressionSettings {
        image_quality: 20,
        ..CompressionSettings::default()
    };
    let out = dir.path().join("out.pptx");
    let messages = run(&root, &out, &mut media, &settings, package_size);

    let saved = actual_media_saving(&media);
    assert!(saved > 0, "高画质源 JPEG 在 q20 下应当真的变小");
    assert!(saved >= c.saving_lower_bound());
    assert!(saved <= c.saving_upper_bound(), "真实节省 {saved} 超过上界 {}", c.saving_upper_bound());
    assert!(messages
        .iter()
        .any(|m| matches!(m, CompressMessage::Finished(..))));
    let mut archive = valid_zip(&out);
    assert_eq!(archive.by_index(0).unwrap().name(), "[Content_Types].xml");
}

#[test]
fn disabling_all_media_gives_zero_bounds_and_zero_real_saving() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::new();
    let pptx = dir.path().join("source.pptx");
    let package_size = write_pptx(&pptx, Some(&fixture));
    let root = dir.path().join("unpacked");
    let mut media = unpack(&pptx, &root);
    for m in &mut media {
        m.enabled = false;
    }
    let c = MediaComposition::from_media(&media, package_size);
    assert_eq!(c.saving_upper_bound(), 0);
    assert!(c.headline().contains("没有可优化"));

    let out = dir.path().join("out.pptx");
    run(
        &root,
        &out,
        &mut media,
        &CompressionSettings::default(),
        package_size,
    );
    assert_eq!(actual_media_saving(&media), 0);
    assert!(!valid_zip(&out).is_empty());
}

#[test]
fn upper_bound_is_identical_for_opposite_quality_presets() {
    // 旧实现的「预计可节省」会随质量滑杆大幅变化；新的可优化空间上界
    // 只取决于真实媒体构成，因此高质量 / 最小体积两档必须给出同一个数字。
    let dir = tempdir().unwrap();
    let fixture = Fixture::new();
    let pptx = dir.path().join("source.pptx");
    let package_size = write_pptx(&pptx, Some(&fixture));

    let mut high = CompressionSettings::default();
    high.apply_preset(Preset::High);
    let mut small = CompressionSettings::default();
    small.apply_preset(Preset::Small);
    assert_ne!(high.image_quality, small.image_quality);

    let (root_a, root_b) = (dir.path().join("a"), dir.path().join("b"));
    let mut media_a = unpack(&pptx, &root_a);
    let mut media_b = unpack(&pptx, &root_b);
    let c_a = MediaComposition::from_media(&media_a, package_size);
    let c_b = MediaComposition::from_media(&media_b, package_size);
    assert_eq!(c_a, c_b);
    assert_eq!(c_a.headline(), c_b.headline());

    let out_a = dir.path().join("a.pptx");
    let out_b = dir.path().join("b.pptx");
    run(&root_a, &out_a, &mut media_a, &high, package_size);
    run(&root_b, &out_b, &mut media_b, &small, package_size);

    let saved_high = actual_media_saving(&media_a);
    let saved_small = actual_media_saving(&media_b);
    assert!(saved_high <= c_a.saving_upper_bound());
    assert!(saved_small <= c_b.saving_upper_bound());
    assert!(saved_small >= saved_high, "更激进的压缩不应比高质量档省得更少");
    assert!(!valid_zip(&out_a).is_empty());
    assert!(!valid_zip(&out_b).is_empty());
}

#[test]
fn package_without_media_reports_zero_bounds_and_still_packs() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("empty-media.pptx");
    let package_size = write_pptx(&pptx, None);
    let root = dir.path().join("unpacked");
    let mut media = unpack(&pptx, &root);
    assert!(media.is_empty());

    let c = MediaComposition::from_media(&media, package_size);
    assert_eq!(c.media_bytes, 0);
    assert_eq!(c.saving_upper_bound(), 0);
    assert_eq!(c.breakdown(), "包内未发现媒体");
    assert!(c.headline().contains("没有可优化"));
    assert_eq!(c.non_media_bytes(), package_size);

    let out: PathBuf = dir.path().join("out.pptx");
    run(
        &root,
        &out,
        &mut media,
        &CompressionSettings::default(),
        package_size,
    );
    let mut archive = valid_zip(&out);
    assert_eq!(archive.by_index(0).unwrap().name(), "[Content_Types].xml");
}
