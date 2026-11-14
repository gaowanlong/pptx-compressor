//! JPEG and PNG compression handlers.
//!
//! Handlers only produce a candidate file; the pipeline decides whether it is
//! small enough to replace the original. Decoding is bounded so a malformed or
//! hostile image cannot exhaust memory.
//!
//! Re-encoding drops all source metadata (EXIF orientation, ICC profile, PNG
//! text/color chunks). After encoding we copy those bytes back verbatim from
//! the source via [`crate::handlers::metadata`], so a rotated photo stays
//! rotated the same way and a profiled image keeps its colors. Injection is
//! best-effort: if either file is malformed we simply keep the clean candidate.

use crate::core::CompressionSettings;
use crate::handlers::metadata::{
    inject_jpeg_segments, inject_png_chunks, jpeg_metadata_segments, png_metadata_chunks,
};
use crate::handlers::CompressCandidate;
use image::imageops::FilterType;
use image::{ImageEncoder as _, ImageReader, Limits};
use std::fs;
use std::io::Cursor;
use std::path::Path;

const MAX_DIMENSION: u32 = 20_000;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

fn decode_bounded(data: &[u8], label: &str) -> Result<image::DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("识别图片格式 {} 失败：{}", label, e))?;

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);

    reader
        .decode()
        .map_err(|e| format!("解码图片 {} 失败：{}", label, e))
}

fn should_resize(img: &image::DynamicImage, settings: &CompressionSettings) -> bool {
    settings.image_max_width_enabled && img.width() > settings.image_max_width
}

fn resize_if_needed(img: image::DynamicImage, settings: &CompressionSettings) -> image::DynamicImage {
    if should_resize(&img, settings) {
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
    let label = path.display().to_string();
    let source = fs::read(path).map_err(|e| format!("读取 JPEG {} 失败：{}", label, e))?;
    let img = resize_if_needed(decode_bounded(&source, &label)?, settings);

    // JPEG has no alpha channel; encoding a 4-channel image produces files
    // that PowerPoint may refuse to render.
    let rgb = img.to_rgb8();
    let mut buffer = Cursor::new(Vec::new());
    let encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, settings.image_quality);
    encoder
        .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("JPEG 编码失败：{}", e))?;

    let mut bytes = buffer.into_inner();
    let segments = jpeg_metadata_segments(&source);
    if let Some(with_metadata) = inject_jpeg_segments(&bytes, &segments) {
        bytes = with_metadata;
    }

    fs::write(candidate, bytes).map_err(|e| format!("写入 JPEG 候选文件失败：{}", e))?;

    Ok(CompressCandidate { path: candidate.to_path_buf(), new_extension: None })
}

/// Re-encode a PNG with maximum compression into `candidate`.
pub fn compress_png(
    path: &Path,
    candidate: &Path,
    settings: &CompressionSettings,
) -> Result<CompressCandidate, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};

    let label = path.display().to_string();
    let source = fs::read(path).map_err(|e| format!("读取 PNG {} 失败：{}", label, e))?;
    let decoded = decode_bounded(&source, &label)?;
    // A resize invalidates the physical pixel size (pHYs / DPI) chunk.
    let keep_phys = !should_resize(&decoded, settings);
    let img = resize_if_needed(decoded, settings);

    let mut buffer = Cursor::new(Vec::new());
    let encoder =
        PngEncoder::new_with_quality(&mut buffer, CompressionType::Best, FilterType::Adaptive);
    encoder
        .write_image(img.as_bytes(), img.width(), img.height(), img.color().into())
        .map_err(|e| format!("PNG 编码失败：{}", e))?;

    let mut bytes = buffer.into_inner();
    let chunks = png_metadata_chunks(&source, keep_phys);
    if let Some(with_metadata) = inject_png_chunks(&bytes, &chunks) {
        bytes = with_metadata;
    }

    fs::write(candidate, bytes).map_err(|e| format!("写入 PNG 候选文件失败：{}", e))?;

    Ok(CompressCandidate { path: candidate.to_path_buf(), new_extension: None })
}
