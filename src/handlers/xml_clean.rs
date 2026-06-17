//! XML cleanup for PPTX internal XML files.
//!
//! Strips XML comments (`<!-- ... -->`) to reduce file size.
//! Does NOT modify whitespace — PPTX text content (in `<a:t>` elements)
//! may contain significant whitespace that must be preserved.

use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Clean all XML files in the extracted PPTX directory.
/// - Strips XML comments
pub fn clean_xml_files(extract_dir: &Path) {
    for entry in WalkDir::new(extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.path().is_file() {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("rels") {
            let _ = clean_xml_file(entry.path());
        }
    }
}

fn clean_xml_file(path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;

    let cleaned = strip_comments(&content);

    // Only write if we actually saved space
    if cleaned.len() < content.len() {
        fs::write(path, cleaned).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Strip XML comments (`<!-- ... -->`). Preserves all other content including whitespace.
fn strip_comments(xml: &str) -> String {
    let mut result = String::with_capacity(xml.len());
    let chars: Vec<char> = xml.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Check for comment start: `<!--`
        if i + 3 < len
            && chars[i] == '<'
            && chars[i + 1] == '!'
            && chars[i + 2] == '-'
            && chars[i + 3] == '-'
        {
            // Skip until `-->`
            i += 4;
            while i + 2 < len {
                if chars[i] == '-' && chars[i + 1] == '-' && chars[i + 2] == '>' {
                    i += 3;
                    break;
                }
                i += 1;
            }
            // If we reached end without finding `-->`, stop
            if i >= len {
                break;
            }
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}
