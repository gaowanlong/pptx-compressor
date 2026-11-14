//! v1.2.4 OOXML 引用完整性：真实临时文件 / ZIP 端到端回归。
//! 不依赖 GUI 与 FFmpeg。

use image::ImageEncoder as _;
use pptx_compressor::core::extractor::{extract_pptx, rewrite_media_references};
use pptx_compressor::core::ooxml::prune_dangling_relationships;
use pptx_compressor::core::pipeline::run_pipeline;
use pptx_compressor::core::{CompressMessage, CompressionSettings};
use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use tempfile::tempdir;
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, ZipArchive};

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
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .write_image(img.as_raw(), 320, 240, image::ExtendedColorType::Rgb8)
        .unwrap();
    let data = buf.into_inner();
    fs::write(path, &data).unwrap();
    data
}

fn add_stored<W: Write + Seek>(zip: &mut ZipWriter<W>, name: &str, data: &[u8]) {
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file(name, opts).unwrap();
    zip.write_all(data).unwrap();
}

fn zip_text(pptx: &Path, name: &str) -> String {
    let mut archive = ZipArchive::new(fs::File::open(pptx).unwrap()).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

fn zip_bytes(pptx: &Path, name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(fs::File::open(pptx).unwrap()).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).unwrap();
    buf
}

// ── 引用同步：覆盖 notesSlides / layouts / masters / handoutMaster / theme ──

#[test]
fn rename_is_synced_across_every_part_and_arbitrary_media_paths() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let rels_dirs = [
        "ppt/slides/_rels",
        "ppt/notesSlides/_rels",
        "ppt/slideLayouts/_rels",
        "ppt/slideMasters/_rels",
        "ppt/handoutMasters/_rels",
        "ppt/theme/_rels",
        "custom/_rels",
    ];
    for d in rels_dirs {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    fs::create_dir_all(root.join("ppt/media")).unwrap();
    fs::write(root.join("ppt/media/clip.mov"), b"fake-video").unwrap();
    fs::write(root.join("ppt/media/image10.mov"), b"fake-video-10").unwrap();

    let targets = [
        ("slide1.xml.rels", "ppt/slides/_rels", "../media/clip.mov"),
        ("notesSlide1.xml.rels", "ppt/notesSlides/_rels", "../notesSlides/media/clip.mov"),
        ("slideLayout1.xml.rels", "ppt/slideLayouts/_rels", "../media/clip.mov"),
        ("slideMaster1.xml.rels", "ppt/slideMasters/_rels", "../media/clip.mov"),
        ("handoutMaster1.xml.rels", "ppt/handoutMasters/_rels", "../media/clip.mov"),
        ("theme1.xml.rels", "ppt/theme/_rels", "../media/clip.mov"),
        ("thing.xml.rels", "custom/_rels", "/ppt/media/clip.mov"),
    ];
    for (file, dir_name, target) in targets {
        let xml = format!(
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="media" Target="{target}"/><Relationship Id="rId2" Type="media" Target="../media/image10.mov"/></Relationships>"#
        );
        fs::write(root.join(dir_name).join(file), xml).unwrap();
    }

    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="mov" ContentType="video/quicktime"/><Override PartName="/ppt/media/clip.mov" ContentType="video/quicktime"/><Override PartName="/ppt/notesSlides/media/clip.mov" ContentType="video/quicktime"/><Override PartName="/ppt/theme/media/clip.mov" ContentType="video/quicktime"/></Types>"#;
    fs::write(root.join("[Content_Types].xml"), ct).unwrap();

    rewrite_media_references(root, &[("clip.mov".into(), "clip.mp4".into())]).unwrap();

    for (file, dir_name, _) in targets {
        let after = fs::read_to_string(root.join(dir_name).join(file)).unwrap();
        assert!(after.contains("clip.mp4"), "{dir_name}/{file} 未同步：{after}");
        assert!(!after.contains("clip.mov"), "{dir_name}/{file} 仍含旧名：{after}");
        assert!(
            after.contains("image10.mov"),
            "{dir_name}/{file} 相似文件名被误伤：{after}"
        );
        assert!(!after.contains("image10.mp4"));
    }

    let ct_after = fs::read_to_string(root.join("[Content_Types].xml")).unwrap();
    for p in [
        "/ppt/media/clip.mp4",
        "/ppt/notesSlides/media/clip.mp4",
        "/ppt/theme/media/clip.mp4",
    ] {
        assert!(ct_after.contains(&format!("PartName=\"{p}\"")), "{p} 未同步");
    }
    assert_eq!(ct_after.matches(r#"ContentType="video/mp4""#).count(), 4);
    assert_eq!(ct_after.matches(r#"Extension="mp4""#).count(), 1);
}

// ── 悬空关系清理 ────────────────────────────────────────────────────────

fn seed_package(root: &Path) {
    for d in [
        "ppt/media",
        "ppt/slides/_rels",
        "ppt/notesSlides/_rels",
        "ppt/notesMasters",
        "ppt/slideLayouts/_rels",
        "ppt/slideMasters/_rels",
        "ppt/handoutMasters/_rels",
        "ppt/theme",
    ] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    for f in [
        "ppt/presentation.xml",
        "ppt/media/image1.png",
        "ppt/notesMasters/notesMaster1.xml",
        "ppt/slideMasters/slideMaster1.xml",
        "ppt/handoutMasters/handoutMaster1.xml",
        "ppt/theme/theme1.xml",
    ] {
        fs::write(root.join(f), b"<p/>").unwrap();
    }

    let rels: &[(&str, &str)] = &[
        (
            "ppt/slides/_rels/slide1.xml.rels",
            r#"<Relationship Id="rId1" Type="image" Target="../media/image1.png"/><Relationship Id="rId2" Type="image" Target="../media/gone.png"/><Relationship Id="rId3" Type="hyperlink" TargetMode="External" Target="https://example.com/clip.mov"/>"#,
        ),
        (
            "ppt/notesSlides/_rels/notesSlide1.xml.rels",
            r#"<Relationship Id="rId1" Type="notesMaster" Target="../notesMasters/notesMaster1.xml"/>"#,
        ),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            r#"<Relationship Id="rId1" Type="slideMaster" Target="../slideMasters/slideMaster1.xml"/>"#,
        ),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            r#"<Relationship Id="rId1" Type="theme" Target="../theme/theme1.xml"/><Relationship Id="rId2" Type="image" Target="../media/missing.jpg"/>"#,
        ),
        (
            "ppt/handoutMasters/_rels/handoutMaster1.xml.rels",
            r#"<Relationship Id="rId1" Type="image" Target="handoutMaster1.xml"/>"#,
        ),
    ];
    for (file, inner) in rels {
        let xml = format!(
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{inner}</Relationships>"#
        );
        fs::write(root.join(file), xml).unwrap();
    }

    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/media/image1.png" ContentType="image/png"/><Override PartName="/ppt/media/gone.png" ContentType="image/png"/><Override PartName="/docProps/thumbnail.jpeg" ContentType="image/jpeg"/></Types>"#;
    fs::write(root.join("[Content_Types].xml"), ct).unwrap();
}

#[test]
fn prune_deletes_only_dangling_relations_and_keeps_all_valid_ooxml_parts() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    seed_package(root);

    let report = prune_dangling_relationships(root).unwrap();
    assert_eq!(report.scanned_rels, 5);
    assert_eq!(report.removed_relationships, 2, "只应删两处悬空关系");
    assert_eq!(report.removed_overrides, 2, "只应删两条失效 Override");
    assert_eq!(report.details.len(), 4);
    assert!(!report.is_empty());

    let slides = fs::read_to_string(root.join("ppt/slides/_rels/slide1.xml.rels")).unwrap();
    assert!(slides.contains("../media/image1.png"));
    assert!(!slides.contains("gone.png"));
    // External 关系即使指向包外 URL 也必须保留。
    assert!(slides.contains("https://example.com/clip.mov"));

    for (file, needle) in [
        ("ppt/notesSlides/_rels/notesSlide1.xml.rels", "../notesMasters/notesMaster1.xml"),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", "../slideMasters/slideMaster1.xml"),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", "../theme/theme1.xml"),
        ("ppt/handoutMasters/_rels/handoutMaster1.xml.rels", "handoutMaster1.xml"),
    ] {
        let content = fs::read_to_string(root.join(file)).unwrap();
        assert!(content.contains(needle), "{file} 的合法关系被误删：{content}");
    }
    let masters = fs::read_to_string(root.join("ppt/slideMasters/_rels/slideMaster1.xml.rels")).unwrap();
    assert!(!masters.contains("missing.jpg"));

    let ct = fs::read_to_string(root.join("[Content_Types].xml")).unwrap();
    assert!(ct.contains(r#"PartName="/ppt/media/image1.png""#));
    assert!(!ct.contains("gone.png"));
    assert!(!ct.contains("thumbnail"));
    assert!(ct.contains(r#"<Default Extension="xml""#));

    // 幂等：再跑一次不应再删任何东西。
    let again = prune_dangling_relationships(root).unwrap();
    assert!(again.is_empty());
}

#[test]
fn prune_is_safe_on_empty_trees_non_utf8_rels_and_escaping_targets() {
    let dir = tempdir().unwrap();
    let empty = dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(prune_dangling_relationships(&empty).unwrap().is_empty());

    let root = dir.path().join("pkg");
    fs::create_dir_all(root.join("ppt/slides/_rels")).unwrap();
    fs::write(
        root.join("ppt/slides/_rels/binary.xml.rels"),
        [0xFF, 0xFE, 0x00, 0x80],
    )
    .unwrap();
    fs::write(
        root.join("ppt/slides/_rels/weird.xml.rels"),
        r##"<Relationships><Relationship Id="rId1" Target="../../../etc/passwd"/><Relationship Id="rId2" Target="#rId1"/><Relationship Id="rId3" TargetMode="External" Target="mailto:a@b.com"/></Relationships>"##,
    )
    .unwrap();

    let report = prune_dangling_relationships(&root).unwrap();
    assert!(report.is_empty(), "无法判断的目标应保守保留：{report:?}");
    let weird = fs::read_to_string(root.join("ppt/slides/_rels/weird.xml.rels")).unwrap();
    assert!(weird.contains("../../../etc/passwd"));
    assert!(weird.contains("#rId1"));
    assert!(weird.contains("mailto:a@b.com"));
}

// ── 完整流水线：非 ppt/media 路径的媒体参与压缩且引用完好 ────────────────

fn build_pptx(path: &Path, jpeg: &[u8]) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    add_stored(
        &mut zip,
        "[Content_Types].xml",
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/ppt/notesSlides/media/photo.jpg" ContentType="image/jpeg"/><Override PartName="/docProps/thumbnail.jpeg" ContentType="image/jpeg"/><Override PartName="/ppt/media/gone.png" ContentType="image/png"/></Types>"#.as_bytes(),
    );
    add_stored(
        &mut zip,
        "_rels/.rels",
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="thumbnail" Target="docProps/thumbnail.jpeg"/></Relationships>"#.as_bytes(),
    );
    add_stored(&mut zip, "ppt/presentation.xml", b"<p:presentation/>");
    add_stored(&mut zip, "ppt/notesSlides/notesSlide1.xml", b"<p:notes/>");
    add_stored(&mut zip, "ppt/notesSlides/media/photo.jpg", jpeg);
    add_stored(
        &mut zip,
        "ppt/notesSlides/_rels/notesSlide1.xml.rels",
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="image" Target="media/photo.jpg"/><Relationship Id="rId2" Type="image" Target="media/missing.jpg"/></Relationships>"#.as_bytes(),
    );
    add_stored(&mut zip, "ppt/theme/theme1.xml", b"<a:theme/>");
    add_stored(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="theme" Target="../theme/theme1.xml"/><Relationship Id="rId2" Type="image" Target="../media/gone.png"/></Relationships>"#.as_bytes(),
    );
    add_stored(&mut zip, "docProps/thumbnail.jpeg", b"fake-thumbnail");
    zip.finish().unwrap();
}

#[test]
fn pipeline_compresses_media_outside_ppt_media_and_leaves_no_dangling_refs() {
    let dir = tempdir().unwrap();
    let scratch = dir.path().join("high-quality.jpg");
    let jpeg = make_jpeg(&scratch, 98);

    let pptx = dir.path().join("in.pptx");
    build_pptx(&pptx, &jpeg);

    let extract = dir.path().join("pkg");
    fs::create_dir_all(&extract).unwrap();
    let mut media = extract_pptx(&pptx, &extract).unwrap();
    assert_eq!(media.len(), 1, "应且只应发现 notesSlides 下的 JPEG");
    assert_eq!(media[0].relative_path, "ppt/notesSlides/media/photo.jpg");

    let output = dir.path().join("out.pptx");
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let settings = CompressionSettings {
        image_quality: 20,
        ..CompressionSettings::default()
    };
    run_pipeline(
        &extract,
        &output,
        &mut media,
        &settings,
        jpeg.len() as u64,
        tx,
        cancel,
    )
    .unwrap();
    let messages: Vec<CompressMessage> = rx.try_iter().collect();
    assert!(messages.iter().any(|m| matches!(m, CompressMessage::Finished(..))));

    let mut archive = ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    assert_eq!(archive.by_index(0).unwrap().name(), "[Content_Types].xml");
    assert!(archive.by_name("ppt/notesSlides/media/photo.jpg").is_ok());
    assert!(archive.by_name("docProps/thumbnail.jpeg").is_err());
    assert!(archive.by_name("ppt/media/gone.png").is_err());
    // 非 ppt/media 路径的已压缩媒体同样用 Stored，不再二次 deflate。
    {
        let entry = archive.by_name("ppt/notesSlides/media/photo.jpg").unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Stored);
    }

    let compressed = zip_bytes(&output, "ppt/notesSlides/media/photo.jpg");
    assert!(compressed.len() < jpeg.len(), "非 ppt/media 媒体应被真正压缩");

    let notes_rels = zip_text(&output, "ppt/notesSlides/_rels/notesSlide1.xml.rels");
    assert!(notes_rels.contains("media/photo.jpg"));
    assert!(!notes_rels.contains("missing.jpg"));
    let master_rels = zip_text(&output, "ppt/slideMasters/_rels/slideMaster1.xml.rels");
    assert!(master_rels.contains("../theme/theme1.xml"));
    assert!(!master_rels.contains("gone.png"));
    let root_rels = zip_text(&output, "_rels/.rels");
    assert!(root_rels.contains("ppt/presentation.xml"));
    assert!(!root_rels.contains("thumbnail"));

    let ct = zip_text(&output, "[Content_Types].xml");
    assert!(ct.contains(r#"PartName="/ppt/notesSlides/media/photo.jpg""#));
    assert!(!ct.contains("gone.png"));
    assert!(!ct.contains("thumbnail"));
}

#[test]
fn repack_keeps_media_in_custom_paths_stored() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    fs::create_dir_all(root.join("ppt/notesSlides/media")).unwrap();
    fs::write(root.join("[Content_Types].xml"), b"<Types/>").unwrap();
    fs::write(root.join("ppt/notesSlides/media/pic.png"), b"fake-png").unwrap();

    let out: PathBuf = dir.path().join("out.pptx");
    pptx_compressor::core::extractor::repack_pptx(&root, &out).unwrap();
    let mut archive = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    let entry = archive.by_name("ppt/notesSlides/media/pic.png").unwrap();
    assert_eq!(entry.compression(), CompressionMethod::Stored);
}
