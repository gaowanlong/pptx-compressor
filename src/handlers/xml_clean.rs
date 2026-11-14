//! XML cleanup for PPTX internal XML files.
//!
//! Strips XML comments (`<!-- ... -->`) to reduce file size.
//! Does NOT modify whitespace — PPTX text content (in `<a:t>` elements)
//! may contain significant whitespace that must be preserved.

use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Clean all XML/rels files. Returns a list of non-fatal problems so the
/// pipeline can surface them instead of silently ignoring them.
pub fn clean_xml_files(extract_dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for entry in WalkDir::new(extract_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.path().is_file() {
            continue;
        }
        let ext = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("rels") {
            if let Err(e) = clean_xml_file(entry.path()) {
                problems.push(format!("{}: {}", entry.path().display(), e));
            }
        }
    }
    problems
}

fn clean_xml_file(path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let cleaned = strip_comments(&content);
    if cleaned.len() < content.len() {
        fs::write(path, cleaned).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Strip XML comments. Preserves all other content including whitespace.
///
/// An unterminated `<!--` is kept verbatim: dropping it would silently
/// truncate the XML part and corrupt the presentation.
pub fn strip_comments(xml: &str) -> String {
    let bytes = xml.as_bytes();
    let mut result = String::with_capacity(xml.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" {
            let rest = &bytes[i + 4..];
            if let Some(offset) = find_bytes(rest, b"-->") {
                i = i + 4 + offset + 3;
                continue;
            }
            // Unterminated comment: keep everything as-is.
            result.push_str(&xml[i..]);
            break;
        }
        let ch = xml[i..].chars().next().unwrap_or('\u{FFFD}');
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}
