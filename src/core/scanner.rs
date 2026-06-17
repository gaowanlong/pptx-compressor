//! Media file scanner and classification utilities.
//! (Primary scanning logic lives in extractor.rs; this module provides helpers.)

use crate::core::MediaType;
use std::path::Path;

/// Check if a file extension corresponds to a compressible media type.
pub fn is_compressible_extension(ext: &str) -> bool {
    MediaType::from_extension(ext).is_compressible()
}

/// Get a human-readable category label for a file extension.
pub fn category_label(ext: &str) -> &'static str {
    MediaType::from_extension(ext).label()
}

/// Detect image dimensions without fully decoding.
/// Returns (width, height) or None on failure.
pub fn detect_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" => {
            let img = image::open(path).ok()?;
            Some((img.width(), img.height()))
        }
        "gif" => {
            let file = std::fs::File::open(path).ok()?;
            let decoder = gif::DecodeOptions::new()
                .read_info(file)
                .ok()?;
            Some((decoder.width() as u32, decoder.height() as u32))
        }
        _ => None,
    }
}
