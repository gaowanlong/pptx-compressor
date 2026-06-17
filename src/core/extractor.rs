//! PPTX ZIP extraction and repacking.

use crate::core::MediaInfo;
use std::fs;
use std::io::{self};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::ZipArchive;

/// Extract a PPTX (ZIP) file into a temporary directory.
/// Returns the list of discovered media files.
pub fn extract_pptx(pptx_path: &Path, extract_dir: &Path) -> io::Result<Vec<MediaInfo>> {
    let file = fs::File::open(pptx_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Use enclosed_name for safe path extraction; fall back to name()
        let out_path = if let Some(safe) = entry.enclosed_name() {
            extract_dir.join(safe)
        } else {
            let name = entry.name().to_string();
            // Skip entries with suspicious paths
            if name.contains("..") {
                continue;
            }
            extract_dir.join(&name)
        };

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            io::copy(&mut entry, &mut outfile)?;
        }
    }

    // Scan for media files
    Ok(scan_media(extract_dir))
}

/// Scan the extracted directory for media files under ppt/media/.
fn scan_media(extract_dir: &Path) -> Vec<MediaInfo> {
    use crate::core::MediaType;
    let mut media_files = Vec::new();
    let media_dir = extract_dir.join("ppt").join("media");

    if !media_dir.exists() {
        return media_files;
    }

    if let Ok(entries) = fs::read_dir(&media_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();

            let media_type = MediaType::from_extension(&ext);

            // Relative path inside the PPTX
            let relative = path
                .strip_prefix(extract_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            media_files.push(MediaInfo::new(relative, path, media_type));
        }
    }

    // Sort by size descending (compress big files first for visible progress)
    media_files.sort_by(|a, b| b.original_size.cmp(&a.original_size));
    media_files
}

/// Repack the extracted directory back into a PPTX (ZIP) file.
/// Media files use STORED (no extra compression on already-compressed data).
/// Everything else uses DEFLATE level 9.
pub fn repack_pptx(extract_dir: &Path, output_path: &Path) -> io::Result<()> {
    let outfile = fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(outfile);

    let stored_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);

    let deflate_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9));

    // Collect and sort for deterministic output.
    // [Content_Types].xml MUST be the first entry per ECMA-376 OOXML standard.
    let mut entries: Vec<walkdir::DirEntry> = walkdir::WalkDir::new(extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() || e.path().is_dir())
        .collect();
    entries.sort_by(|a, b| {
        let a_rel = a.path().strip_prefix(extract_dir).unwrap_or(a.path());
        let b_rel = b.path().strip_prefix(extract_dir).unwrap_or(b.path());
        // [Content_Types].xml must be first per ECMA-376
        let a_is_ct = a_rel == std::path::Path::new("[Content_Types].xml");
        let b_is_ct = b_rel == std::path::Path::new("[Content_Types].xml");
        if a_is_ct && !b_is_ct {
            std::cmp::Ordering::Less
        } else if !a_is_ct && b_is_ct {
            std::cmp::Ordering::Greater
        } else {
            a_rel.cmp(b_rel)
        }
    });

    for entry in &entries {
        let file_path = entry.path();

        if file_path == extract_dir {
            continue; // skip the root itself
        }

        let relative = file_path
            .strip_prefix(extract_dir)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            .to_string_lossy()
            .replace('\\', "/");

        if file_path.is_dir() {
            // Add directory entry (required by some OOXML consumers)
            let dir_name = if relative.ends_with('/') {
                relative.to_string()
            } else {
                format!("{}/", relative)
            };
            zip.add_directory(dir_name, deflate_options)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        } else {
            // Choose compression: media files are already compressed formats
            let is_media = relative.starts_with("ppt/media/")
                && is_compressed_format(file_path);
            let options = if is_media {
                stored_options
            } else {
                deflate_options
            };

            zip.start_file(relative, options)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            let mut f = fs::File::open(file_path)?;
            io::copy(&mut f, &mut zip)?;
        }
    }

    zip.finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(())
}

/// Check if a file is an already-compressed format (don't deflate again).
fn is_compressed_format(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "mp4" | "wmv" | "mov" | "webm" | "m4v" | "mp3"
            | "wav" | "ogg" | "zip"
    )
}
