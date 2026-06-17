//! Utilities for updating [Content_Types].xml inside a PPTX.
//!
//! NOTE: The current compression pipeline never converts between image
//! formats (PNG stays PNG, JPEG stays JPEG), so this module's functionality
//! is **not needed** at runtime and is NOT called from the pipeline.
//! The code is kept for reference and potential future use.

use std::fs;
use std::path::Path;

/// Update `[Content_Types].xml` to reflect any PNG→JPEG conversions.
///
/// Scans `ppt/media/` for .jpeg/.jpg files and ensures their overrides
/// exist in `[Content_Types].xml`, while removing stale PNG overrides
/// for files that no longer exist on disk.
///
/// This function correctly handles both multi-line and single-line
/// (inline) formatted [Content_Types].xml files.
pub fn update_content_types(extract_dir: &Path) -> Result<(), String> {
    let ct_path = extract_dir.join("[Content_Types].xml");
    if !ct_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&ct_path).map_err(|e| e.to_string())?;
    let media_dir = extract_dir.join("ppt").join("media");

    // --- Step 1: Remove stale PNG overrides ---
    // Match pattern: <Override PartName="/ppt/media/filename.png" ContentType="image/png"/>
    // Must be an Override for a file in ppt/media/ with PNG content type.
    let mut result = content.clone();

    // Collect ranges [start, end) of Override tags to remove.
    // We scan for each occurrence of "<Override" and check if it's a PNG media override
    // for a file that no longer exists on disk.
    let mut ranges_to_remove: Vec<(usize, usize)> = Vec::new();
    let mut search_pos = 0;
    while let Some(start) = result[search_pos..].find("<Override") {
        let abs_start = search_pos + start;
        // Find the end of this <Override .../> tag
        let after = &result[abs_start..];
        let end = after.find("/>").map(|e| abs_start + e + 2);
        
        if let Some(abs_end) = end {
            let tag = &result[abs_start..abs_end];
            
            // Check: this must be for a file in ppt/media/ with PNG content type
            if tag.contains("image/png") && tag.contains("PartName=\"") {
                // Extract all PartName values from this tag
                let mut part_search = 0;
                while let Some(pn_start) = tag[part_search..].find("PartName=\"") {
                    let pn_abs = part_search + pn_start + 10; // len of 'PartName="'
                    let value_rest = &tag[pn_abs..];
                    if let Some(pn_end) = value_rest.find('"') {
                        let part_name = &value_rest[..pn_end];
                        // Only care about files in ppt/media/
                        if part_name.starts_with("/ppt/media/") {
                            let file_name = part_name.rsplit('/').next().unwrap_or("");
                            let file_path = media_dir.join(file_name);
                            if !file_path.exists() {
                                // This PNG override references a file that doesn't exist
                                // → remove the entire <Override/> tag
                                ranges_to_remove.push((abs_start, abs_end));
                                break; // one tag, one removal
                            }
                        }
                    }
                    part_search += pn_start + 1;
                }
            }
            search_pos = abs_end;
        } else {
            break;
        }
    }

    // Remove from end to start to preserve position indices
    ranges_to_remove.sort_by(|a, b| b.0.cmp(&a.0));
    for (start, end) in &ranges_to_remove {
        result.replace_range(*start..*end, "");
    }

    // --- Step 2: Add JPEG overrides for new JPEG files ---
    if let Ok(entries) = fs::read_dir(&media_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if ext == "jpg" || ext == "jpeg" {
                let file_name = path.file_name().unwrap().to_string_lossy();
                let part_name = format!("/ppt/media/{}", file_name);
                
                // Check if this file already has an Override (or is covered by a Default)
                if !result.contains(&part_name) {
                    let override_entry = format!(
                        "\n<Override PartName=\"{}\" ContentType=\"image/jpeg\"/>",
                        part_name
                    );
                    // Insert before closing </Types>
                    if let Some(types_close) = result.rfind("</Types>") {
                        result.insert_str(types_close, &override_entry);
                    }
                }
            }
        }
    }

    // Only write if modified
    if result != content {
        fs::write(&ct_path, result).map_err(|e| e.to_string())?;
    }

    Ok(())
}
