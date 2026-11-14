//! JPEG and PNG compression handlers.
//!
//! Handlers only produce a candidate file; the pipeline decides whether it is
//! small enough to replace the original. Decoding is bounded so a malformed or
//! hostile image cannot exhaust memory.

use crate::core::CompressionSettings;
use crate::handlers::CompressCandidate;
use image::imageops::FilterType;
use image::{ImageEncoder as _, ImageReader, Limits};
use std::path::Path;

const MAX_DIMENSION: u32 = 20_000;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

fn decode_bounded(path: &Path) -> Result<image::DynamicImage, String> {
    let mut reader = ImageReader::open(path)
        .map_err(|e| format!("Failed to open image {}: {}", path.display(), e))?
        .with_guessed_format()
        .map_err(|e| format!("Failed to detect image format {}: {}", path.display(), e))?;

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);

    reader
        .decode()
        .map_err(|e| format!("Failed to decode image {}: {}", path.display(), e))
}

fn resize_if_needed(img: image::DynamicImage, settings: &CompressionSettings) -> image::DynamicImage {
    if settings.image_max_width_enabled && img.width() > settings.image_max_width {
        let ratio = settings.image_max_width as f64 / img.width() as f64;
        // A very wide image can round the height down to zero; never emit a 0px image.
        let new_height = ((img.height() as f64 * ratio) as u32).max(1);
        img.resize_exact(settings.image_max_width, new_height, FilterType::Lanczos3)
    } else {
        img
    }
}

/// Re-encode a JPEG at the configured quality into `candidate`.
pub fn compress_jpeg(
    path: &Path,
    candidate: &Path,
    settings: &CompressionSettings,
) -> Result<CompressCandidate, String> {
    let img = resize_if_needed(decode_bounded(path)?, settings);

    // JPEG has no alpha channel; encoding a 4-channel image produces files
    // that PowerPoint may refuse to render.
    let rgb = img.to_rgb8();
    let mut buffer = std::io::Cursor::new(Vec::new());
    let encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, settings.image_quality);
    encoder
        .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("Failed to encode JPEG: {}", e))?;

    std::fs::write(candidate, buffer.into_inner())
        .map_err(|e| format!("Failed to write JPEG candidate: {}", e))?;

    Ok(CompressCandidate { path: candidate.to_path_buf(), new_extension: None })
}

/// Re-encode a PNG with maximum compression into `candidate`.
pub fn compress_png(
    path: &Path,
    candidate: &Path,
    settings: &CompressionSettings,
) -> Result<CompressCandidate, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};

    let img = resize_if_needed(decode_bounded(path)?, settings);

    let mut buffer = std::io::Cursor::new(Vec::new());
    let encoder =
        PngEncoder::new_with_quality(&mut buffer, CompressionType::Best, FilterType::Adaptive);
    encoder
        .write_image(img.as_bytes(), img.width(), img.height(), img.color().into())
        .map_err(|e| format!("Failed to encode PNG: {}", e))?;

    std::fs::write(candidate, buffer.into_inner())
        .map_err(|e| format!("Failed to write PNG candidate: {}", e))?;

    Ok(CompressCandidate { path: candidate.to_path_buf(), new_extension: None })
}
