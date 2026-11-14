//! OOXML 引用完整性：基于关系模型的引用同步与悬空关系清理。
//!
//! 动机（v1.2.4）：旧实现把 `.rels` 里的旧文件名当普通子串全局替换
//! （`clip.mov` 会误伤 `clip.mov.bak`、`image1.mov` 会误伤 `image10.mov`），
//! `[Content_Types].xml` 又硬编码 `/ppt/media/` 前缀，导致 notesSlides /
//! slideLayouts / slideMasters / handoutMaster / theme 以及自定义目录下的
//! 媒体引用漏更新；删除媒体或缩略图后还可能留下指向不存在部件的悬空
//! 关系，PowerPoint 会提示「需要修复」。
//!
//! 本模块只做**带边界检查的字节扫描**，不整体重写 XML：OOXML 的 `.rels`
//! 与 `[Content_Types].xml` 由工具生成、结构受限，逐标签定位属性可原样
//! 保留命名空间、属性顺序与空白，也不需要引入 XML 解析依赖。所有解析
//! 在畸形输入上都是「跳过该标签」而非「改写整份文档」。

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

/// 一次引用重写的结果，供测试与活动日志使用。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RewriteReport {
    /// 被改写的属性值数量（`.rels` 的 Target 或 CT 的 PartName）。
    pub rewritten: usize,
    /// 补齐的 `<Default Extension=.../>` 数量。
    pub added_defaults: usize,
}

/// 悬空关系清理结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// 扫描过的 `.rels` 文件数。
    pub scanned_rels: usize,
    /// 删除的悬空 `<Relationship/>` 条数。
    pub removed_relationships: usize,
    /// 删除的失效 `<Override/>` 条数。
    pub removed_overrides: usize,
    /// 人类可读的清理明细，供活动日志按需截断展示。
    pub details: Vec<String>,
}

impl PruneReport {
    pub fn is_empty(&self) -> bool {
        self.removed_relationships == 0 && self.removed_overrides == 0
    }
}

/// 常见媒体扩展名对应的 OOXML ContentType。未知扩展返回 `None`，
/// 此时保留原声明、不猜测，避免写入错误的内容类型。
pub fn content_type_for_ext(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "svg" => Some("image/svg+xml"),
        "mp4" | "m4v" => Some("video/mp4"),
        "wmv" => Some("video/x-ms-wmv"),
        "avi" => Some("video/x-msvideo"),
        "mov" => Some("video/quicktime"),
        "webm" => Some("video/webm"),
        "mpg" | "mpeg" => Some("video/mpeg"),
        "asf" => Some("video/x-ms-asf"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "ogg" => Some("audio/ogg"),
        _ => None,
    }
}

fn content_type_for_name(name: &str) -> Option<&'static str> {
    let ext = Path::new(name).extension().and_then(|e| e.to_str())?;
    content_type_for_ext(ext)
}

/// 找出所有以 `<Name` 开头的标签，返回其在原串中的字节范围（含结尾 `>`）。
///
/// 标签名后必须紧跟空白、`/`、`>`，因此 `<Relationships .../>` 不会被
/// 误认成 `<Relationship .../>`。属性值里出现的 `>` 由 `tag_end` 跳过。
fn scan_tags(xml: &str, name: &str) -> Vec<(usize, usize)> {
    let prefix = format!("<{name}");
    let bytes = xml.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < xml.len() {
        let Some(rel) = xml[cursor..].find(&prefix) else { break };
        let start = cursor + rel;
        let after = start + prefix.len();
        if after < bytes.len()
            && !matches!(bytes[after], b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>')
        {
            cursor = after;
            continue;
        }
        let Some(end_rel) = tag_end(&xml[start..]) else { break };
        let end = start + end_rel;
        out.push((start, end));
        cursor = end;
    }
    out
}

/// 找到标签结束的 `>`（引号内的 `>` 不算），返回相对长度（含 `>`）。
fn tag_end(tag_from_start: &str) -> Option<usize> {
    let bytes = tag_from_start.as_bytes();
    let mut quote: u8 = 0;
    let mut i = 1usize; // 跳过 '<'
    while i < bytes.len() {
        let b = bytes[i];
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'"' || b == b'\'' {
            quote = b;
        } else if b == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// 取属性值及其在 `tag` 内的字节范围（不含引号）。
///
/// 要求属性名位于空白或 `<` 之后，避免 `xTarget=` 命中 `Target=`。
fn attr<'a>(tag: &'a str, name: &str) -> Option<(usize, usize, &'a str)> {
    let needle = format!("{name}=");
    let bytes = tag.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = tag[from..].find(&needle) {
        let pos = from + rel;
        let boundary_ok = pos == 0
            || matches!(bytes[pos - 1], b' ' | b'\t' | b'\r' | b'\n' | b'<');
        let value_start = pos + needle.len();
        if boundary_ok && value_start < bytes.len() {
            let quote = bytes[value_start];
            if quote == b'"' || quote == b'\'' {
                let vs = value_start + 1;
                if let Some(rel_end) = tag[vs..].find(quote as char) {
                    let ve = vs + rel_end;
                    if tag.is_char_boundary(vs) && tag.is_char_boundary(ve) {
                        return Some((vs, ve, &tag[vs..ve]));
                    }
                }
            }
        }
        from = pos + needle.len();
        if from >= tag.len() {
            break;
        }
    }
    None
}

/// 外部目标（绝对 URL / mailto / 盘符 / UNC）不是包内部件，不参与改写与清理。
fn is_external(target: &str) -> bool {
    let t = target.trim();
    if t.is_empty() {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("file://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
        || lower.starts_with("data:")
        || lower.starts_with("news:")
        || t.contains("://")
        || t.starts_with("\\\\")
        || (t.len() >= 2 && t.as_bytes()[1] == b':')
}

/// 返回 target 中「文件名部分」的字节范围（剥离 `#fragment` 与路径前缀）。
fn basename_range(target: &str) -> Option<(usize, usize)> {
    let end = target.find('#').unwrap_or(target.len());
    let path = &target[..end];
    let start = path.rfind(['/', '\\']).map_or(0, |p| p + 1);
    if start >= end {
        return None;
    }
    Some((start, end))
}

/// 把 `.rels` 中每个 `Target` 的**文件名部分**精确替换为新名字。
///
/// 只按 basename 全等匹配，保留 `../media/`、`/ppt/notesSlides/media/`
/// 等任意相对 / 绝对前缀；`TargetMode="External"` 与外部 URL 一律跳过。
pub fn rewrite_rels_targets(
    xml: &str,
    renames: &[(String, String)],
) -> (String, RewriteReport) {
    if renames.is_empty() {
        return (xml.to_string(), RewriteReport::default());
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (start, end) in scan_tags(xml, "Relationship") {
        let tag = &xml[start..end];
        if let Some((_, _, mode)) = attr(tag, "TargetMode") {
            if mode.eq_ignore_ascii_case("External") {
                continue;
            }
        }
        let Some((vs, _, target)) = attr(tag, "Target") else {
            continue;
        };
        if is_external(target) {
            continue;
        }
        let Some((bs, be)) = basename_range(target) else {
            continue;
        };
        let base = &target[bs..be];
        if let Some((_, new)) = renames.iter().find(|(old, _)| old.as_str() == base) {
            edits.push((start + vs + bs, start + vs + be, new.to_owned()));
        }
    }
    let report = RewriteReport {
        rewritten: edits.len(),
        added_defaults: 0,
    };
    (apply_edits(xml, edits), report)
}

/// 同步 `[Content_Types].xml`：Override 的 PartName 按 basename 精确改写
/// （不限 `/ppt/media/` 路径）并修正 ContentType；缺少的 Default 扩展补齐。
pub fn rewrite_content_types(
    xml: &str,
    renames: &[(String, String)],
) -> (String, RewriteReport) {
    if renames.is_empty() {
        return (xml.to_string(), RewriteReport::default());
    }
    let mut defaults: HashSet<String> = HashSet::new();
    for (s, e) in scan_tags(xml, "Default") {
        let tag = &xml[s..e];
        if let Some((_, _, ext)) = attr(tag, "Extension") {
            defaults.insert(ext.to_ascii_lowercase());
        }
    }

    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut rewritten = 0usize;
    for (s, e) in scan_tags(xml, "Override") {
        let tag = &xml[s..e];
        let Some((pvs, _, part)) = attr(tag, "PartName") else {
            continue;
        };
        let Some((bs, be)) = basename_range(part) else {
            continue;
        };
        let base = &part[bs..be];
        let Some((_, new)) = renames.iter().find(|(old, _)| old.as_str() == base) else {
            continue;
        };
        edits.push((s + pvs + bs, s + pvs + be, new.to_owned()));
        rewritten += 1;
        if let Some((cvs, cve, _)) = attr(tag, "ContentType") {
            if let Some(ct) = content_type_for_name(new) {
                edits.push((s + cvs, s + cve, ct.to_string()));
            }
        }
    }

    // 无论是否存在对应 Override，都要保证新扩展名有 Default 声明：
    // 很多包只用 `<Default Extension="wmv"/>` 声明视频，没有 Override。
    let mut to_add: Vec<(String, String)> = Vec::new();
    for (_, new) in renames {
        let Some(ext) = Path::new(new).extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext_lc = ext.to_ascii_lowercase();
        let Some(ct) = content_type_for_ext(&ext_lc) else {
            continue;
        };
        if !defaults.contains(&ext_lc) && !to_add.iter().any(|(e, _)| e == &ext_lc) {
            to_add.push((ext_lc, ct.to_string()));
        }
    }

    let mut out = apply_edits(xml, edits);
    if !to_add.is_empty() {
        // 插在 `<Types ...>` 开标签之后，而不是 XML 声明的 `?>` 之后。
        if let Some(pos) = out.find("<Types").and_then(|s| tag_end(&out[s..]).map(|e| s + e)) {
            let mut snippet = String::new();
            for (ext, ct) in &to_add {
                snippet.push_str(&format!(
                    "<Default Extension=\"{ext}\" ContentType=\"{ct}\"/>"
                ));
            }
            if out.is_char_boundary(pos) {
                out.insert_str(pos, &snippet);
            }
        }
    }
    (
        out,
        RewriteReport {
            rewritten,
            added_defaults: to_add.len(),
        },
    )
}

/// 从后往前应用不重叠的字节区间替换，保证前面的索引不失效。
fn apply_edits(xml: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    let mut out = xml.to_string();
    edits.sort_by_key(|(s, _, _)| *s);
    for (s, e, value) in edits.into_iter().rev() {
        if s <= e && e <= out.len() && out.is_char_boundary(s) && out.is_char_boundary(e) {
            out.replace_range(s..e, &value);
        }
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// OPC 的 Target 允许 `%20` 这类百分号编码，判断部件是否存在前先解码。
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `.rels` 位于 `<part_dir>/_rels/<name>.rels`，关系目标相对 `<part_dir>`
/// 解析；根包 `_rels/.rels` 的 `<part_dir>` 就是包根。
fn rels_source_dir(extract_dir: &Path, rels_path: &Path) -> PathBuf {
    let parent = rels_path.parent().unwrap_or(extract_dir);
    if parent.file_name().and_then(|n| n.to_str()) == Some("_rels") {
        parent.parent().unwrap_or(extract_dir).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

/// 纯词法规范化 `..` / `.`，不触碰文件系统。
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir => out.push(Path::new("/")),
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

/// 把一条关系的 `Target` 解析为包内路径。外部目标或会逃出解压目录的
/// 路径返回 `None`，调用方一律**保守保留**，绝不因为无法判断而误删。
pub fn resolve_relationship_path(
    extract_dir: &Path,
    rels_path: &Path,
    target: &str,
) -> Option<PathBuf> {
    let t = target.trim();
    if is_external(t) {
        return None;
    }
    let without_fragment = t.split('#').next().unwrap_or(t);
    let decoded = percent_decode(without_fragment);
    if decoded.is_empty() {
        return None;
    }
    let source_dir = rels_source_dir(extract_dir, rels_path);
    let joined = if let Some(stripped) = decoded.strip_prefix('/') {
        extract_dir.join(stripped)
    } else {
        source_dir.join(&decoded)
    };
    let normalized = lexical_normalize(&joined);
    if !normalized.starts_with(extract_dir) {
        return None;
    }
    Some(normalized)
}

fn rel_display(extract_dir: &Path, path: &Path) -> String {
    path.strip_prefix(extract_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 删除所有指向**不存在部件**的内部关系与失效的 ContentType Override。
///
/// 覆盖整个包：notesSlides / slideLayouts / slideMasters / handoutMaster /
/// theme / notesMasters / 自定义目录的 `.rels` 都会被扫描。只要目标部件
/// 存在（或是 External 关系）就原样保留，因此不会动任何有效的 `rId`。
pub fn prune_dangling_relationships(extract_dir: &Path) -> io::Result<PruneReport> {
    let mut report = PruneReport::default();

    let mut rels_files: Vec<PathBuf> = WalkDir::new(extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_file()
                && e
                    .path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".rels"))
        })
        .map(|e| e.path().to_path_buf())
        .collect();
    rels_files.sort();

    for rels_path in rels_files {
        let Ok(content) = fs::read_to_string(&rels_path) else {
            // 非 UTF-8 的关系文件无法安全改写：跳过而不是让整条流水线失败。
            report
                .details
                .push(format!("{}：非 UTF-8，跳过引用检查", rel_display(extract_dir, &rels_path)));
            continue;
        };
        report.scanned_rels += 1;
        let mut removals: Vec<(usize, usize)> = Vec::new();
        for (s, e) in scan_tags(&content, "Relationship") {
            let tag = &content[s..e];
            if let Some((_, _, mode)) = attr(tag, "TargetMode") {
                if mode.eq_ignore_ascii_case("External") {
                    continue;
                }
            }
            let Some((_, _, target)) = attr(tag, "Target") else {
                continue;
            };
            let Some(part) = resolve_relationship_path(extract_dir, &rels_path, target) else {
                continue;
            };
            if !part.exists() {
                removals.push((s, e));
                report.details.push(format!(
                    "{}：{}",
                    rel_display(extract_dir, &rels_path),
                    target
                ));
            }
        }
        if removals.is_empty() {
            continue;
        }
        report.removed_relationships += removals.len();
        fs::write(&rels_path, remove_ranges(&content, removals))?;
    }

    let ct_path = extract_dir.join("[Content_Types].xml");
    if let Ok(content) = fs::read_to_string(&ct_path) {
        let mut removals = Vec::new();
        for (s, e) in scan_tags(&content, "Override") {
            let tag = &content[s..e];
            let Some((_, _, part_name)) = attr(tag, "PartName") else {
                continue;
            };
            let decoded = percent_decode(part_name);
            let Some(rel) = decoded.strip_prefix('/') else {
                continue;
            };
            let part = lexical_normalize(&extract_dir.join(rel));
            if !part.starts_with(extract_dir) || part.is_file() {
                continue;
            }
            removals.push((s, e));
            report
                .details
                .push(format!("[Content_Types].xml：{part_name}"));
        }
        if !removals.is_empty() {
            report.removed_overrides += removals.len();
            fs::write(&ct_path, remove_ranges(&content, removals))?;
        }
    }

    Ok(report)
}

fn remove_ranges(xml: &str, mut ranges: Vec<(usize, usize)>) -> String {
    let mut out = xml.to_string();
    ranges.sort_by_key(|(s, _)| *s);
    for (s, e) in ranges.into_iter().rev() {
        if s <= e && e <= out.len() && out.is_char_boundary(s) && out.is_char_boundary(e) {
            out.replace_range(s..e, "");
        }
    }
    out
}
