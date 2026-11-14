//! 输入安全检测回归测试：加密 / IRM / OLE / 签名 / 宏 / 结构损坏。
//! 不依赖 GUI 与 FFmpeg。

use pptx_compressor::core::extractor::{extract_pptx, extract_pptx_inspected};
use pptx_compressor::core::inspection::{classify, inspect_pptx, Severity};
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const CT: &[u8] = b"<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/></Types>";
const PRESENTATION: &[u8] = b"<?xml version=\"1.0\"?><p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"/>";

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn classify_clean_package_has_no_findings() {
    let findings = classify(&names(&["[content_types].xml", "ppt/presentation.xml"]), "");
    assert!(findings.is_empty(), "正常文稿不应产生任何提示：{findings:?}");
}

#[test]
fn classify_detects_encryption_and_irm() {
    let findings = classify(
        &names(&["[content_types].xml", "ppt/presentation.xml", "encryptioninfo"]),
        "",
    );
    assert!(findings.iter().any(|f| f.code == "encrypted"));
    assert!(findings.iter().any(|f| f.severity == Severity::Block));

    let irm = classify(
        &names(&[
            "[content_types].xml",
            "ppt/presentation.xml",
            "\u{6}DataSpaces/TransformInfo/primary",
        ]),
        "",
    );
    assert!(irm.iter().any(|f| f.code == "encrypted"));
}

#[test]
fn classify_structural_problems_are_actionable() {
    let missing_ct = classify(&names(&["ppt/presentation.xml"]), "");
    assert!(missing_ct.iter().any(|f| f.code == "no_content_types"));
    assert!(missing_ct.iter().any(|f| f.severity == Severity::Block));

    let no_presentation = classify(&names(&["[content_types].xml"]), "");
    assert!(no_presentation.iter().any(|f| f.code == "no_presentation"));
    assert!(no_presentation
        .iter()
        .all(|f| f.severity == Severity::Warn));

    let empty = classify(&[], "");
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0].code, "empty");
}

#[test]
fn classify_collects_all_risk_signals() {
    let findings = classify(
        &names(&[
            "[content_types].xml",
            "ppt/presentation.xml",
            "_xmlsignatures/origin.sigs",
            "ppt/vbaproject.bin",
            "ppt/embeddings/oleobject1.bin",
        ]),
        "<p:presentation><p:modifyVerifier cryptProviderType=\"rsaAES\"/></p:presentation>",
    );
    let codes: Vec<_> = findings.iter().map(|f| f.code).collect();
    for expected in [
        "signature",
        "macro",
        "embedded_object",
        "modify_protected",
    ] {
        assert!(codes.contains(&expected), "缺少 {expected}：{codes:?}");
    }
    assert!(findings.iter().all(|f| f.severity == Severity::Warn));
}

#[test]
fn valid_package_extracts_and_reports_no_warnings() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("ok.pptx");
    write_zip(
        &pptx,
        &[
            ("[Content_Types].xml", CT),
            ("ppt/presentation.xml", PRESENTATION),
            ("ppt/media/image1.jpg", b"fake-jpeg"),
        ],
    );
    let out = dir.path().join("extracted");
    fs::create_dir_all(&out).unwrap();

    let (media, inspection) = extract_pptx_inspected(&pptx, &out).unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(media[0].relative_path, "ppt/media/image1.jpg");
    assert!(!inspection.is_blocked());
    assert!(inspection.warning_messages().is_empty());
    assert!(out.join("[Content_Types].xml").exists());
}

#[test]
fn encrypted_package_is_rejected_before_any_file_is_written() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("encrypted.pptx");
    write_zip(
        &pptx,
        &[
            ("[Content_Types].xml", CT),
            ("ppt/presentation.xml", PRESENTATION),
            ("EncryptionInfo", b"encrypted"),
            ("EncryptedPackage", b"encrypted"),
        ],
    );
    let out = dir.path().join("extracted");
    fs::create_dir_all(&out).unwrap();

    let err = extract_pptx(&pptx, &out).unwrap_err().to_string();
    assert!(err.contains("加密"), "错误信息应说明原因：{err}");
    assert!(err.contains("另存"), "错误信息应给出建议：{err}");
    assert_eq!(
        fs::read_dir(&out).unwrap().count(),
        0,
        "被阻断的文件不能写入解压目录"
    );

    let inspection = inspect_pptx(&pptx).unwrap();
    assert!(inspection.codes().contains(&"encrypted"));
}

#[test]
fn ole_container_gives_actionable_error_instead_of_zip_parse_error() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("legacy.pptx");
    fs::write(
        &pptx,
        [&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1][..], &[0u8; 64][..]].concat(),
    )
    .unwrap();

    let inspection = inspect_pptx(&pptx).unwrap();
    assert!(inspection.is_blocked());
    assert!(inspection.codes().contains(&"ole_container"));
    let message = inspection.blocker_message().unwrap();
    assert!(message.contains("旧版 .ppt"), "{message}");

    let out = dir.path().join("extracted");
    fs::create_dir_all(&out).unwrap();
    assert!(extract_pptx(&pptx, &out).is_err());
}

#[test]
fn warnings_are_returned_alongside_extraction() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("risky.pptx");
    write_zip(
        &pptx,
        &[
            ("[Content_Types].xml", CT),
            (
                "ppt/presentation.xml",
                b"<p:presentation><p:modifyVerifier/></p:presentation>",
            ),
            ("_xmlsignatures/origin.sigs", b"sig"),
            ("ppt/vbaProject.bin", b"macro"),
            ("ppt/embeddings/oleObject1.bin", b"ole"),
        ],
    );
    let out = dir.path().join("extracted");
    fs::create_dir_all(&out).unwrap();

    let (_, inspection) = extract_pptx_inspected(&pptx, &out).unwrap();
    assert!(!inspection.is_blocked());
    let warnings = inspection.warning_messages();
    assert_eq!(warnings.len(), 4, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("签名")));
    assert!(warnings.iter().any(|w| w.contains("宏")));
    assert!(warnings.iter().any(|w| w.contains("嵌入对象")));
    assert!(warnings.iter().any(|w| w.contains("修改密码")));
}

#[test]
fn zip_bomb_with_valid_skeleton_is_still_rejected_by_ratio_guard() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("bomb.pptx");
    {
        let mut zip = ZipWriter::new(fs::File::create(&pptx).unwrap());
        let deflated = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        zip.start_file("[Content_Types].xml", deflated).unwrap();
        zip.write_all(CT).unwrap();
        zip.start_file("ppt/presentation.xml", deflated).unwrap();
        zip.write_all(PRESENTATION).unwrap();
        zip.start_file("ppt/media/big.bin", deflated).unwrap();
        zip.write_all(&vec![0u8; 4 * 1024 * 1024]).unwrap();
        zip.finish().unwrap();
    }
    let out = dir.path().join("extracted");
    fs::create_dir_all(&out).unwrap();

    let err = extract_pptx(&pptx, &out).unwrap_err().to_string();
    assert!(err.contains("压缩比"), "应由炸弹防护拦截：{err}");
}
