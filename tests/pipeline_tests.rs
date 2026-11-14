//! Regression tests for v1.0.1 correctness fixes.
//! No GUI or FFmpeg is required to run this suite.

use pptx_compressor::core::extractor::{
    extract_pptx, repack_pptx, rewrite_media_references, strip_thumbnails,
};
use pptx_compressor::core::pipeline::should_replace;
use pptx_compressor::core::{crf_from_quality, CompressionSettings};
use pptx_compressor::handlers::xml_clean::strip_comments;
use std::fs;
use std::io::Write;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[test]
fn crf_mapping_is_consistent() {
    assert_eq!(crf_from_quality(0), 51);
    assert_eq!(crf_from_quality(18), 42);
    assert_eq!(crf_from_quality(23), 40);
    assert_eq!(crf_from_quality(45), 29);
    assert_eq!(crf_from_quality(65), 18);
    assert_eq!(crf_from_quality(100), 0);
    assert_eq!(CompressionSettings::default().crf(), 29);
}

#[test]
fn balanced_preset_matches_default() {
    let default = CompressionSettings::default();
    let mut preset = CompressionSettings::default();
    preset.preset_medium();
    assert_eq!(preset.image_quality, default.image_quality);
    assert_eq!(preset.video_quality, default.video_quality);
    assert_eq!(preset.crf(), 29);
}

#[test]
fn only_strictly_smaller_candidates_replace() {
    assert!(should_replace(100, 99));
    assert!(!should_replace(100, 100));
    assert!(!should_replace(100, 101));
    assert!(!should_replace(100, 0));
    assert!(!should_replace(0, 0));
}

#[test]
fn comment_stripping_never_truncates_xml() {
    assert_eq!(strip_comments("<a><!-- note --><b/>"), "<a><b/>");
    assert_eq!(
        strip_comments("<a>中文<!--注释-->尾</a>"),
        "<a>中文尾</a>"
    );
    // An unterminated comment must be kept verbatim, not silently dropped.
    assert_eq!(
        strip_comments("<a><!-- broken <b/>尾"),
        "<a><!-- broken <b/>尾"
    );
}

#[test]
fn media_rename_updates_all_ooxml_references() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("ppt/slides/_rels")).unwrap();

    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="wmv" ContentType="video/x-ms-wmv"/><Override PartName="/ppt/media/video1.wmv" ContentType="video/x-ms-wmv"/></Types>"#;
    fs::write(root.join("[Content_Types].xml"), ct).unwrap();
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="video" Target="../media/video1.wmv"/></Relationships>"#;
    fs::write(root.join("ppt/slides/_rels/slide1.xml.rels"), rels).unwrap();

    rewrite_media_references(root, &[("video1.wmv".into(), "video1.mp4".into())]).unwrap();

    let ct_after = fs::read_to_string(root.join("[Content_Types].xml")).unwrap();
    assert!(ct_after.contains("PartName=\"/ppt/media/video1.mp4\""));
    assert!(ct_after.contains("ContentType=\"video/mp4\""));
    assert!(ct_after.contains("Extension=\"mp4\""));
    // The Default must be inserted inside <Types>, not after the XML declaration.
    assert!(ct_after.find("<Types").unwrap() < ct_after.find("Extension=\"mp4\"").unwrap());

    let rels_after =
        fs::read_to_string(root.join("ppt/slides/_rels/slide1.xml.rels")).unwrap();
    assert!(rels_after.contains("../media/video1.mp4"));
    assert!(!rels_after.contains("video1.wmv"));
}

#[test]
fn thumbnail_cleanup_removes_dangling_references() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("docProps")).unwrap();
    fs::create_dir_all(root.join("_rels")).unwrap();
    fs::write(root.join("docProps/thumbnail.jpeg"), b"fake-jpeg").unwrap();

    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="thumbnail" Target="docProps/thumbnail.jpeg"/><Relationship Id="rId2" Type="officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    fs::write(root.join("_rels/.rels"), rels).unwrap();
    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/docProps/thumbnail.jpeg" ContentType="image/jpeg"/><Default Extension="xml" ContentType="application/xml"/></Types>"#;
    fs::write(root.join("[Content_Types].xml"), ct).unwrap();

    let problems = strip_thumbnails(root);
    assert!(problems.is_empty());
    assert!(!root.join("docProps/thumbnail.jpeg").exists());

    let rels_after = fs::read_to_string(root.join("_rels/.rels")).unwrap();
    assert!(!rels_after.contains("thumbnail"));
    assert!(rels_after.contains("rId2"));

    let ct_after = fs::read_to_string(root.join("[Content_Types].xml")).unwrap();
    assert!(!ct_after.contains("thumbnail"));
    assert!(ct_after.contains("Default Extension=\"xml\""));
}

#[test]
fn repack_writes_a_valid_pptx() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src = root.join("src");
    fs::create_dir_all(src.join("ppt/media")).unwrap();
    fs::create_dir_all(src.join("_rels")).unwrap();
    fs::write(src.join("[Content_Types].xml"), b"<Types/>").unwrap();
    fs::write(src.join("ppt/media/image1.png"), b"fake-png").unwrap();
    fs::write(src.join("_rels/.rels"), b"<Relationships/>").unwrap();

    let out = root.join("out.pptx");
    repack_pptx(&src, &out).unwrap();

    let mut archive = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    assert_eq!(archive.by_index(0).unwrap().name(), "[Content_Types].xml");
    assert!(archive.by_name("ppt/media/image1.png").is_ok());
    assert!(archive.by_name("_rels/.rels").is_ok());
}

#[test]
fn extraction_rejects_zip_bombs() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("bomb.pptx");
    {
        let mut zip = ZipWriter::new(fs::File::create(&pptx).unwrap());
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        // Structure must look valid so the test really exercises the ratio guard.
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(b"<Types/>").unwrap();
        zip.start_file("ppt/presentation.xml", options).unwrap();
        zip.write_all(b"<p:presentation/>").unwrap();
        zip.start_file("ppt/media/big.bin", options).unwrap();
        zip.write_all(&vec![0u8; 4 * 1024 * 1024]).unwrap();
        zip.finish().unwrap();
    }
    let out = dir.path().join("extracted");
    fs::create_dir_all(&out).unwrap();
    let err = extract_pptx(&pptx, &out).unwrap_err().to_string();
    assert!(err.contains("压缩比"), "应由炸弹防护拦截：{err}");
}
