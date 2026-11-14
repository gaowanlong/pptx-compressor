//! v1.2.4 OOXML 引用完整性：字符串层纯函数回归测试。
//! 不依赖 GUI / FFmpeg / 真实文件系统。

use pptx_compressor::core::ooxml::{
    content_type_for_ext, resolve_relationship_path, rewrite_content_types, rewrite_rels_targets,
    RewriteReport,
};
use std::path::{Path, PathBuf};

fn rename(old: &str, new: &str) -> Vec<(String, String)> {
    vec![(old.to_string(), new.to_string())]
}

fn rels(inner: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{inner}</Relationships>"#
    )
}

fn types(inner: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{inner}</Types>"#
    )
}

fn relationship(id: &str, target: &str) -> String {
    format!(r#"<Relationship Id="{id}" Type="media" Target="{target}"/>"#)
}

#[test]
fn content_type_mapping_covers_common_media_and_rejects_unknown() {
    assert_eq!(content_type_for_ext("mp4"), Some("video/mp4"));
    assert_eq!(content_type_for_ext("M4V"), Some("video/mp4"));
    assert_eq!(content_type_for_ext("jpg"), Some("image/jpeg"));
    assert_eq!(content_type_for_ext("png"), Some("image/png"));
    assert_eq!(content_type_for_ext("wmv"), Some("video/x-ms-wmv"));
    assert_eq!(content_type_for_ext("mov"), Some("video/quicktime"));
    assert_eq!(content_type_for_ext("bin"), None);
}

#[test]
fn rels_rewrite_matches_basename_exactly_without_hurting_similar_names() {
    let xml = rels(&format!(
        "{}{}{}",
        relationship("rId1", "../media/clip.mov"),
        relationship("rId2", "../media/image10.mov"),
        relationship("rId3", "/ppt/notesSlides/media/clip.mov"),
    ));
    let (out, report) = rewrite_rels_targets(&xml, &rename("clip.mov", "clip.mp4"));

    assert_eq!(report.rewritten, 2);
    assert!(out.contains(r#"Target="../media/clip.mp4""#));
    assert!(out.contains(r#"Target="/ppt/notesSlides/media/clip.mp4""#));
    // 旧实现的全局子串替换会把 image10.mov 误改成 image10.mp4 / 串位。
    assert!(out.contains(r#"Target="../media/image10.mov""#), "{out}");
    assert!(!out.contains("image10.mp4"));
    assert!(!out.contains("clip.mov"));
}

#[test]
fn rels_rewrite_keeps_prefix_single_quotes_and_fragments() {
    let xml = rels(&format!(
        "{}{}{}",
        r#"<Relationship Id="rId1" Type="media" Target='../slideLayouts/media/pic.png'/> "#,
        relationship("rId2", "../media/clip.mov#t=1"),
        relationship("rId3", "../media/sub\\clip.mov"),
    ));
    let (out, _) = rewrite_rels_targets(&xml, &rename("clip.mov", "clip.mp4"));
    assert!(out.contains("Target='../slideLayouts/media/pic.png'"));
    assert!(out.contains(r#"Target="../media/clip.mp4#t=1""#));
    assert!(out.contains(r#"Target="../media/sub\clip.mp4""#));
}

#[test]
fn rels_rewrite_skips_external_and_unrelated_targets() {
    let xml = rels(&format!(
        "{}{}{}{}",
        r#"<Relationship Id="rId1" Type="hyperlink" TargetMode="External" Target="https://example.com/clip.mov"/> "#,
        r#"<Relationship Id="rId2" Type="hyperlink" TargetMode="external" Target="mailto:a@b.com"/> "#,
        relationship("rId3", "../media/clip.mov.bak"),
        relationship("rId4", "../media/other.wmv"),
    ));
    let (out, report) = rewrite_rels_targets(&xml, &rename("clip.mov", "clip.mp4"));
    assert_eq!(report.rewritten, 0);
    assert_eq!(out, xml, "外部链接与不精确匹配的 Target 必须原样保留");
    assert!(out.contains("https://example.com/clip.mov"));
    assert!(out.contains("clip.mov.bak"));
}

#[test]
fn rels_rewrite_is_a_noop_for_empty_renames_and_malformed_xml() {
    let xml = rels(&relationship("rId1", "../media/clip.mov"));
    let (out, report) = rewrite_rels_targets(&xml, &[]);
    assert_eq!(out, xml);
    assert_eq!(report, RewriteReport::default());

    // 未闭合标签：找不到结尾 `>` 就跳过，绝不截断文档。
    let broken = r#"<Relationships><Relationship Id="rId1" Target="../media/clip.mov"#;
    let (out, report) = rewrite_rels_targets(broken, &rename("clip.mov", "clip.mp4"));
    assert_eq!(out, broken);
    assert_eq!(report.rewritten, 0);

    // 根标签 `<Relationships>` 不能被当成一条 Relationship。
    let only_root = r#"<Relationships xmlns="x"></Relationships>"#;
    let (out, report) = rewrite_rels_targets(only_root, &rename("clip.mov", "clip.mp4"));
    assert_eq!(out, only_root);
    assert_eq!(report.rewritten, 0);
}

#[test]
fn content_types_rewrites_overrides_at_any_path_and_fixes_type() {
    let xml = types(&format!(
        "{}{}{}",
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/> "#,
        r#"<Override PartName="/ppt/media/clip.mov" ContentType="video/quicktime"/> "#,
        r#"<Override PartName="/ppt/notesSlides/media/clip.mov" ContentType="video/quicktime"/> "#,
    ));
    let (out, report) = rewrite_content_types(&xml, &rename("clip.mov", "clip.mp4"));

    assert_eq!(report.rewritten, 2);
    assert_eq!(report.added_defaults, 1);
    assert!(out.contains(r#"PartName="/ppt/media/clip.mp4""#));
    assert!(out.contains(r#"PartName="/ppt/notesSlides/media/clip.mp4""#));
    assert_eq!(out.matches(r#"ContentType="video/mp4""#).count(), 3); // 2 Override + 1 Default
    assert_eq!(out.matches(r#"Extension="mp4""#).count(), 1);
    // Default 必须插在 <Types ...> 之内，而不是 XML 声明的 ?> 之后。
    assert!(out.find("<Types").unwrap() < out.find(r#"Extension="mp4""#).unwrap());
    assert!(out.contains("</Types>"));
}

#[test]
fn content_types_adds_default_even_without_any_override() {
    // 很多包只用 <Default Extension="mov"> 声明视频，根本没有 Override。
    let xml = types(r#"<Default Extension="mov" ContentType="video/quicktime"/>"#);
    let (out, report) = rewrite_content_types(&xml, &rename("clip.mov", "clip.mp4"));
    assert_eq!(report.rewritten, 0);
    assert_eq!(report.added_defaults, 1);
    assert!(out.contains(r#"<Default Extension="mp4" ContentType="video/mp4"/>"#));
    assert!(out.ends_with("</Types>"));
}

#[test]
fn content_types_does_not_duplicate_existing_default() {
    let xml = types(&format!(
        "{}{}",
        r#"<Default Extension="mp4" ContentType="video/mp4"/> "#,
        r#"<Override PartName="/ppt/media/clip.mov" ContentType="video/quicktime"/> "#,
    ));
    let (out, report) = rewrite_content_types(&xml, &rename("clip.mov", "clip.mp4"));
    assert_eq!(report.added_defaults, 0);
    assert_eq!(out.matches(r#"Extension="mp4""#).count(), 1);
    assert!(out.contains(r#"PartName="/ppt/media/clip.mp4""#));
}

#[test]
fn content_types_defaults_are_deduplicated_across_multiple_renames() {
    let renames = vec![
        ("a.mov".to_string(), "a.mp4".to_string()),
        ("b.wmv".to_string(), "b.mp4".to_string()),
    ];
    let xml = types(r#"<Default Extension="mov" ContentType="video/quicktime"/>"#);
    let (out, report) = rewrite_content_types(&xml, &renames);
    assert_eq!(report.added_defaults, 1);
    assert_eq!(out.matches(r#"Extension="mp4""#).count(), 1);
}

#[test]
fn content_types_unknown_extension_keeps_the_original_declaration() {
    let xml = types(r#"<Override PartName="/ppt/media/clip.mov" ContentType="video/quicktime"/>"#);
    let (out, report) = rewrite_content_types(&xml, &rename("clip.mov", "clip.weird"));
    assert_eq!(report.rewritten, 1);
    assert_eq!(report.added_defaults, 0);
    assert!(out.contains(r#"PartName="/ppt/media/clip.weird""#));
    // 未知扩展不猜内容类型，也不凭空补 Default。
    assert!(out.contains(r#"ContentType="video/quicktime""#));
    assert!(!out.contains("Extension=\"weird\""));
}

const ROOT: &str = "/tmp/ooxml-pkg";

fn path(rel: &str) -> PathBuf {
    Path::new(ROOT).join(rel)
}

fn resolved(rels_rel: &str, target: &str) -> Option<PathBuf> {
    resolve_relationship_path(Path::new(ROOT), &path(rels_rel), target)
}

#[test]
fn relationship_targets_resolve_relative_to_the_source_part() {
    assert_eq!(
        resolved("ppt/slides/_rels/slide1.xml.rels", "../media/image1.png"),
        Some(path("ppt/media/image1.png"))
    );
    assert_eq!(
        resolved("ppt/notesSlides/_rels/notesSlide1.xml.rels", "media/pic.jpg"),
        Some(path("ppt/notesSlides/media/pic.jpg"))
    );
    assert_eq!(
        resolved("_rels/.rels", "docProps/thumbnail.jpeg"),
        Some(path("docProps/thumbnail.jpeg"))
    );
    assert_eq!(
        resolved("ppt/slideMasters/_rels/slideMaster1.xml.rels", "../theme/theme1.xml"),
        Some(path("ppt/theme/theme1.xml"))
    );
}

#[test]
fn relationship_targets_handle_absolute_paths_decoding_and_fragments() {
    assert_eq!(
        resolved("ppt/slides/_rels/slide1.xml.rels", "/ppt/handoutMasters/handoutMaster1.xml"),
        Some(path("ppt/handoutMasters/handoutMaster1.xml"))
    );
    assert_eq!(
        resolved("ppt/slides/_rels/slide1.xml.rels", "../media/my%20image.png"),
        Some(path("ppt/media/my image.png"))
    );
    assert_eq!(
        resolved("ppt/slides/_rels/slide1.xml.rels", "../media/image1.png#slide=2"),
        Some(path("ppt/media/image1.png"))
    );
    assert_eq!(
        resolved("ppt/slides/_rels/slide1.xml.rels", "../media/%E4%B8%AD%E6%96%87.png"),
        Some(path("ppt/media/中文.png"))
    );
}

#[test]
fn relationship_targets_reject_external_and_escaping_paths() {
    for target in [
        "https://example.com/a.png",
        "mailto:a@b.com",
        "file:///etc/passwd",
        "C:\\Windows\\a.png",
        "../../../etc/passwd",
    ] {
        assert_eq!(
            resolved("ppt/slides/_rels/slide1.xml.rels", target),
            None,
            "应保守保留而非解析：{target}"
        );
    }
    // 纯片段不是包内部件，返回 None 让调用方保留。
    assert_eq!(resolved("ppt/slides/_rels/slide1.xml.rels", "#rId2"), None);
}
