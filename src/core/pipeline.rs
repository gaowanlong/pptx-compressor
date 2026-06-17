//! Compression pipeline: orchestrates media compression and PPTX repacking.

use crate::core::{CompressMessage, CompressionSettings, MediaInfo, MediaStatus};
use crate::handlers::{gif_handler, image, video, xml_clean};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// Run the full compression pipeline in a background thread.
///
/// 1. Compress each enabled media file according to its type.
/// 2. Update `[Content_Types].xml` if any file format changed.
/// 3. Clean up XML files.
/// 4. Repack the extracted directory into a new PPTX.
///
/// Progress is reported through `tx`. Set `cancel` to `true` to abort early.
pub fn run_pipeline(
    extract_dir: &Path,
    output_path: &Path,
    media_files: &mut Vec<MediaInfo>,
    settings: &CompressionSettings,
    original_pptx_size: u64,
    tx: Sender<CompressMessage>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let compressible: Vec<usize> = media_files
        .iter()
        .enumerate()
        .filter(|(_, m)| m.enabled && m.media_type.is_compressible())
        .map(|(i, _)| i)
        .collect();

    let total = compressible.len();

    for (completed, &idx) in compressible.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            let _ = tx.send(CompressMessage::Error("Cancelled by user".into()));
            return Err("Cancelled".into());
        }

        let file_name = media_files[idx]
            .extracted_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let _ = tx.send(CompressMessage::Progress(
            completed,
            total,
            file_name.clone(),
        ));
        let _ = tx.send(CompressMessage::StatusUpdate(idx, MediaStatus::Processing, 0));

        let result = match media_files[idx].media_type {
            crate::core::MediaType::Jpeg => {
                image::compress_jpeg(&media_files[idx].extracted_path, settings)
            }
            crate::core::MediaType::Png => {
                image::compress_png(&media_files[idx].extracted_path, settings)
            }
            crate::core::MediaType::Gif => {
                gif_handler::compress_gif(&media_files[idx].extracted_path, settings)
            }
            crate::core::MediaType::Video => {
                video::compress_video(&media_files[idx].extracted_path, settings)
            }
            crate::core::MediaType::Other => Ok(()),
        };

        match result {
            Ok(()) => {
                let new_size = std::fs::metadata(&media_files[idx].extracted_path)
                    .map(|m| m.len())
                    .unwrap_or(media_files[idx].original_size);

                if new_size >= media_files[idx].original_size {
                    // Compressed file is larger — skip this file
                    media_files[idx].compressed_size = media_files[idx].original_size;
                    let _ = tx.send(CompressMessage::StatusUpdate(
                        idx,
                        MediaStatus::Skipped,
                        media_files[idx].original_size,
                    ));
                } else {
                    media_files[idx].compressed_size = new_size;
                    let _ = tx.send(CompressMessage::StatusUpdate(
                        idx,
                        MediaStatus::Done,
                        new_size,
                    ));
                }
            }
            Err(e) => {
                media_files[idx].compressed_size = media_files[idx].original_size;
                let _ = tx.send(CompressMessage::StatusUpdate(
                    idx,
                    MediaStatus::Failed,
                    media_files[idx].original_size,
                ));
                let _ = tx.send(CompressMessage::Error(format!(
                    "Failed to compress {}: {}",
                    file_name, e
                )));
            }
        }
    }

    // Note: update_content_types is intentionally NOT called here.
    // The current pipeline never converts PNG→JPEG (both formats are
    // re-encoded in-place), so there are no content-type changes to apply.
    // The string-based update_content_types function also corrupts
    // inline-formatted [Content_Types].xml (single-line XML).

    // Clean XML files (strip comments)
    xml_clean::clean_xml_files(extract_dir);

    // Strip preview thumbnails (docProps/thumbnail.*)
    crate::core::extractor::strip_thumbnails(extract_dir);

    // Repack into PPTX
    let _ = tx.send(CompressMessage::Progress(
        total,
        total,
        "Repacking PPTX...".into(),
    ));

    crate::core::extractor::repack_pptx(extract_dir, output_path)
        .map_err(|e| format!("Failed to repack PPTX: {}", e))?;

    // Report overall PPTX file sizes
    let output_size = std::fs::metadata(output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let _ = tx.send(CompressMessage::Finished(original_pptx_size, output_size));

    Ok(())
}
