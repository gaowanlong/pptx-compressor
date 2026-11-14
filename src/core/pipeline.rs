//! Compression pipeline: orchestrates media compression and PPTX repacking.
//!
//! Handlers never modify the extracted tree. They write candidates into a
//! private work directory; this module is the single place that decides whether
//! a candidate may replace the original. That makes "compressed is larger",
//! encode failures and user cancels non-destructive.

use crate::core::{CompressMessage, CompressionSettings, MediaInfo, MediaStatus, MediaType};
use crate::handlers::{gif_handler, image, video, xml_clean, CANCELLED};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// A candidate replaces the original only when it is non-empty and smaller.
pub fn should_replace(original_size: u64, new_size: u64) -> bool {
    new_size > 0 && new_size < original_size
}

fn is_mp4_container(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str(),
        "mp4" | "m4v"
    )
}

fn candidate_path(work_dir: &Path, idx: usize, original: &Path, kind: MediaType) -> PathBuf {
    let stem = original
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("media");
    let ext = match kind {
        MediaType::Video => "mp4",
        MediaType::Gif => "gif",
        MediaType::Jpeg => "jpg",
        MediaType::Png => "png",
        MediaType::Other => "bin",
    };
    work_dir.join(format!("{:04}-{}.{}", idx, stem, ext))
}

fn backup_path(work_dir: &Path, idx: usize) -> PathBuf {
    work_dir.join(format!("{:04}-original.bin", idx))
}

fn unique_target(original: &Path, new_ext: Option<&str>) -> PathBuf {
    let Some(ext) = new_ext else {
        return original.to_path_buf();
    };
    let target = original.with_extension(ext);
    if !target.exists() {
        return target;
    }
    let parent = original.parent().unwrap_or_else(|| Path::new("."));
    let stem = original.file_stem().and_then(|s| s.to_str()).unwrap_or("media");
    for i in 2..1000 {
        let p = parent.join(format!("{}_c{}.{}", stem, i, ext));
        if !p.exists() {
            return p;
        }
    }
    target
}

fn install_candidate(candidate: &Path, original: &Path, target: &Path) -> Result<(), String> {
    fs::copy(candidate, target).map_err(|e| format!("Failed to install candidate: {}", e))?;
    if target != original {
        fs::remove_file(original).map_err(|e| format!("Failed to remove old media: {}", e))?;
    }
    Ok(())
}

fn send(tx: &Sender<CompressMessage>, msg: CompressMessage) {
    let _ = tx.send(msg);
}

/// Run the full compression pipeline in a background thread.
///
/// 1. Compress each enabled media file into a candidate.
/// 2. Replace only strictly smaller candidates, keeping a backup for recovery.
/// 3. Keep OOXML declarations in sync when a video container changed.
/// 4. Strip XML comments and preview thumbnails (with their relationships).
/// 5. Atomically repack and validate the output PPTX.
pub fn run_pipeline(
    extract_dir: &Path,
    output_path: &Path,
    media_files: &mut Vec<MediaInfo>,
    settings: &CompressionSettings,
    original_pptx_size: u64,
    tx: Sender<CompressMessage>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let work_dir = tempfile::tempdir().map_err(|e| format!("Failed to create work dir: {}", e))?;

    let compressible: Vec<usize> = media_files
        .iter()
        .enumerate()
        .filter(|(_, m)| m.enabled && m.media_type.is_compressible())
        .map(|(i, _)| i)
        .collect();
    let total = compressible.len();
    let mut renames: Vec<(String, String)> = Vec::new();

    for (completed, &idx) in compressible.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            send(&tx, CompressMessage::Cancelled);
            return Ok(());
        }

        let original_path = media_files[idx].extracted_path.clone();
        let file_name = original_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let original_size = media_files[idx].original_size;
        send(&tx, CompressMessage::Progress(completed + 1, total, file_name.clone()));
        send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Processing, 0));

        // Without H.264 conversion we must not rewrite a non-MP4 container:
        // FFmpeg always emits MP4, which would disagree with the file name.
        if media_files[idx].media_type == MediaType::Video
            && !settings.video_convert_to_h264
            && !is_mp4_container(&original_path)
        {
            media_files[idx].compressed_size = original_size;
            send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Skipped, original_size));
            send(&tx, CompressMessage::Error(format!(
                "Skipped {}: enable H.264 conversion to process non-MP4 video",
                file_name
            )));
            continue;
        }

        let candidate = candidate_path(work_dir.path(), idx, &original_path, media_files[idx].media_type);
        let backup = backup_path(work_dir.path(), idx);
        if let Err(e) = fs::copy(&original_path, &backup) {
            media_files[idx].compressed_size = original_size;
            send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Failed, original_size));
            send(&tx, CompressMessage::Error(format!("Failed to back up {}: {}", file_name, e)));
            continue;
        }

        let result = match media_files[idx].media_type {
            MediaType::Jpeg => image::compress_jpeg(&original_path, &candidate, settings),
            MediaType::Png => image::compress_png(&original_path, &candidate, settings),
            MediaType::Gif => gif_handler::compress_gif(&original_path, &candidate, settings),
            MediaType::Video => {
                video::compress_video(&original_path, &candidate, settings, cancel.as_ref())
            }
            MediaType::Other => {
                send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Skipped, original_size));
                continue;
            }
        };

        let candidate = match result {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_file(&candidate);
                if e == CANCELLED {
                    send(&tx, CompressMessage::Cancelled);
                    return Ok(());
                }
                media_files[idx].compressed_size = original_size;
                send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Failed, original_size));
                send(&tx, CompressMessage::Error(format!("Failed to compress {}: {}", file_name, e)));
                continue;
            }
        };

        let new_size = fs::metadata(&candidate.path).map(|m| m.len()).unwrap_or(0);
        if !should_replace(original_size, new_size) {
            let _ = fs::remove_file(&candidate.path);
            media_files[idx].compressed_size = original_size;
            send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Skipped, original_size));
            continue;
        }

        let target = unique_target(&original_path, candidate.new_extension.as_deref());
        if let Err(e) = install_candidate(&candidate.path, &original_path, &target) {
            // Roll back from the backup so the output never contains a
            // half-written or enlarged media file.
            let _ = fs::copy(&backup, &original_path);
            media_files[idx].compressed_size = original_size;
            send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Failed, original_size));
            send(&tx, CompressMessage::Error(format!("Failed to replace {}: {}", file_name, e)));
            continue;
        }

        if target != original_path {
            let new_name = target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            renames.push((file_name, new_name));
        }
        media_files[idx].extracted_path = target.clone();
        media_files[idx].relative_path = target
            .strip_prefix(extract_dir)
            .unwrap_or(&target)
            .to_string_lossy()
            .replace('\\', "/");
        media_files[idx].compressed_size = new_size;
        send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Done, new_size));
    }

    if cancel.load(Ordering::Relaxed) {
        send(&tx, CompressMessage::Cancelled);
        return Ok(());
    }

    if !renames.is_empty() {
        crate::core::extractor::rewrite_media_references(extract_dir, &renames)
            .map_err(|e| format!("Failed to update OOXML media references: {}", e))?;
    }

    for problem in xml_clean::clean_xml_files(extract_dir) {
        send(&tx, CompressMessage::Error(format!("XML cleanup: {}", problem)));
    }
    for problem in crate::core::extractor::strip_thumbnails(extract_dir) {
        send(&tx, CompressMessage::Error(format!("Thumbnail cleanup: {}", problem)));
    }

    send(&tx, CompressMessage::Progress(total, total, "Repacking PPTX...".into()));
    crate::core::extractor::repack_pptx(extract_dir, output_path)
        .map_err(|e| format!("Failed to repack PPTX: {}", e))?;

    let output_size = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    send(&tx, CompressMessage::Finished(original_pptx_size, output_size));
    Ok(())
}
