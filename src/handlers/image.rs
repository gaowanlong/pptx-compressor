//! JPEG and PNG compression handlers.

use crate::core::CompressionSettings;
use image::imageops::FilterType;
use image::ImageEncoder as _;
use std::path::Path;

/// Compress a JPEG file by re-encoding at the specified quality.
/// Optionally resizes if the image exceeds the configured max width.
pub fn compress_jpeg(path: &Path, settings: &CompressionSettings) -> Result<(), String> {
    let img = image::open(path).map_err(|e| format!("Failed to open JPEG {}: {}", path.display(), e))?;

    let img = if settings.image_max_width_enabled && img.width() > settings.image_max_width {
        let ratio = settings.image_max_width as f64 / img.width() as f64;
        let new_height = (img.height() as f64 * ratio) as u32;
        img.resize_exact(
            settings.image_max_width,
            new_height,
            FilterType::Lanczos3,
        )
    } else {
        img
    };

    // Encode JPEG to memory buffer
    let mut buffer = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut buffer,
        settings.image_quality,
    );
    encoder
        .write_image(
            img.as_bytes(),
            img.width(),
            img.height(),
            img.color().into(),
        )
        .map_err(|e| format!("Failed to encode JPEG: {}", e))?;

    // Write compressed data to file (replaces original)
    std::fs::write(path, buffer.into_inner())
        .map_err(|e| format!("Failed to write JPEG: {}", e))?;

    Ok(())
}

/// Compress a PNG file by re-encoding with maximum compression.
/// Optionally resizes if the image exceeds the configured max width.
/// Uses the `image` crate's PNG encoder with Best compression and Adaptive filtering.
/// Falls back to keeping the original if re-encoding does not reduce file size.
pub fn compress_png(path: &Path, settings: &CompressionSettings) -> Result<(), String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder as _;

    let img = image::open(path).map_err(|e| format!("Failed to open PNG {}: {}", path.display(), e))?;

    let img = if settings.image_max_width_enabled && img.width() > settings.image_max_width {
        let ratio = settings.image_max_width as f64 / img.width() as f64;
        let new_height = (img.height() as f64 * ratio) as u32;
        img.resize_exact(
            settings.image_max_width,
            new_height,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    let mut buffer = std::io::Cursor::new(Vec::new());
    let encoder = PngEncoder::new_with_quality(&mut buffer, CompressionType::Best, FilterType::Adaptive);
    encoder
        .write_image(
            img.as_bytes(),
            img.width(),
            img.height(),
            img.color().into(),
        )
        .map_err(|e| format!("Failed to encode PNG: {}", e))?;

    let new_data = buffer.into_inner();
    let original_len = std::fs::metadata(path)
        .map(|m| m.len() as usize)
        .unwrap_or(usize::MAX);

    // Only write back if we actually reduced the file size
    if new_data.len() < original_len {
        std::fs::write(path, &new_data)
            .map_err(|e| format!("Failed to write optimized PNG: {}", e))?;
    }

    Ok(())
}
