//! PPTX ZIP extraction, OOXML reference maintenance and repacking.

use crate::core::MediaInfo;
use std::fs;
use std::io::{self};
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::ZipArchive;

const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024; // 4 GiB
const MAX_SINGLE_RATIO: u64 = 200;

/// Safely resolve a ZIP entry path, trying UTF-8 on raw bytes first.
///
/// ZIP entries without the UTF-8 flag (bit 11) get decoded as CP-437
/// by the `zip` crate, which garbles Chinese characters. We try UTF-8 on
/// the raw bytes first and fall back to the crate's decoded name.
fn resolve_entry_path(entry: &zip::read::ZipFile) -> Option<PathBuf> {
    let raw = entry.name_raw();
    let name = String::from_utf8(raw.to_vec())
        .ok()
        .unwrap_or_else(|| entry.name().to_string());

    if name.contains('\0') {
        return None;
    }

    let path = PathBuf::from(&name);
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => return None,
            Component::ParentDir => depth = depth.checked_sub(1)?,
            Component::Normal(_) => depth += 1,
            Component::CurDir => (),
        }
    }
    Some(path)
}

/// Extract a PPTX (ZIP) file into a temporary directory.
/// Returns the list of discovered media files.
///
/// Guards against zip bombs, symlink entries and absurd entry counts.
pub fn extract_pptx(pptx_path: &Path, extract_dir: &Path) -> io::Result<Vec<MediaInfo>> {
    let file = fs::File::open(pptx_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if archive.len() > MAX_ENTRIES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("PPTX 条目数过多（{}），已拒绝解压", archive.len()),
        ));
    }

    let mut declared_total: u64 = 0;
    let mut written_total: u64 = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Never materialize symlinks from an untrusted archive.
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                continue;
            }
        }

        let declared = entry.size();
        let compressed = entry.compressed_size();
        if compressed > 0 && declared / compressed.max(1) > MAX_SINGLE_RATIO {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("检测到异常压缩比（{}），疑似 ZIP 炸弹", entry.name()),
            ));
        }
        declared_total = declared_total.saturating_add(declared);
        if declared_total > MAX_TOTAL_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PPTX 解压后体积超过 4 GiB 安全上限",
            ));
        }

        let out_path = resolve_entry_path(&entry)
            .map(|p| extract_dir.join(p))
            .unwrap_or_else(|| {
                let name = entry.name().to_string();
                if name.contains("..") || name.contains('\0') {
                    return PathBuf::new();
                }
                extract_dir.join(name.trim_start_matches('/'))
            });

        if out_path.as_os_str().is_empty() {
            continue; // skip dangerous entries
        }

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            let written = io::copy(&mut entry, &mut outfile)?;
            written_total = written_total.saturating_add(written);
            if written_total > MAX_TOTAL_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "解压数据量超过 4 GiB 安全上限",
                ));
            }
        }
    }

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
            let relative = path
                .strip_prefix(extract_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            media_files.push(MediaInfo::new(relative, path, media_type));
        }
    }

    media_files.sort_by_key(|m| std::cmp::Reverse(m.original_size));
    media_files
}

/// Repack the extracted directory into a PPTX (ZIP) file.
///
/// The archive is written to a sibling temp file, validated by reading it
/// back, and only then renamed into place. A crash or a full disk can no
/// longer leave a truncated `.pptx` at the destination.
pub fn repack_pptx(extract_dir: &Path, output_path: &Path) -> io::Result<()> {
    let tmp_path = output_path.with_extension(format!("tmp-{}.pptx", std::process::id()));
    let _ = fs::remove_file(&tmp_path);

    let outfile = fs::File::create(&tmp_path)?;
    let mut zip = zip::ZipWriter::new(outfile);

    let stored_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    let deflate_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9));

    // [Content_Types].xml MUST be the first entry per ECMA-376 OOXML.
    let mut entries: Vec<walkdir::DirEntry> = walkdir::WalkDir::new(extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy();
            let hidden_junk = name.starts_with("._") || name == ".DS_Store";
            let scratch = name.ends_with(".ffmpeg.log") || name.contains(".staged");
            !hidden_junk && !scratch && (e.path().is_file() || e.path().is_dir())
        })
        .collect();
    entries.sort_by(|a, b| {
        let a_rel = a.path().strip_prefix(extract_dir).unwrap_or(a.path());
        let b_rel = b.path().strip_prefix(extract_dir).unwrap_or(b.path());
        let a_is_ct = a_rel == Path::new("[Content_Types].xml");
        let b_is_ct = b_rel == Path::new("[Content_Types].xml");
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
            continue;
        }
        let relative = file_path
            .strip_prefix(extract_dir)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");

        if file_path.is_dir() {
            let dir_name = if relative.ends_with('/') {
                relative
            } else {
                format!("{}/", relative)
            };
            zip.add_directory(dir_name, deflate_options)
                .map_err(io::Error::other)?;
        } else {
            let is_media = relative.starts_with("ppt/media/") && is_compressed_format(file_path);
            let options = if is_media { stored_options } else { deflate_options };
            zip.start_file(relative, options)
                .map_err(io::Error::other)?;
            let mut f = fs::File::open(file_path)?;
            io::copy(&mut f, &mut zip)?;
        }
    }

    let finished = zip.finish().map_err(io::Error::other)?;
    drop(finished);

    if let Err(e) = validate_pptx_zip(&tmp_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    if output_path.exists() {
        let _ = fs::remove_file(output_path);
    }
    if fs::rename(&tmp_path, output_path).is_err() {
        fs::copy(&tmp_path, output_path)?;
        let _ = fs::remove_file(&tmp_path);
    }
    Ok(())
}

fn validate_pptx_zip(path: &Path) -> io::Result<()> {
    let f = fs::File::open(path)?;
    let mut archive =
        ZipArchive::new(f).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    archive
        .by_name("[Content_Types].xml")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(())
}

/// Remove the XML tag that contains `needle` (an attribute such as
/// `Target="docProps/thumbnail.jpeg"`). Used on single-line OOXML parts.
fn remove_tags_containing(content: &str, needle: &str, start: &str, end: &str) -> String {
    let mut result = content.to_string();
    while let Some(pos) = result.find(needle) {
        let Some(tag_start) = result[..pos].rfind(start) else { break };
        let Some(rel_end) = result[pos + needle.len()..].find(end) else { break };
        let tag_end = pos + needle.len() + rel_end + end.len();
        if tag_end > result.len() {
            break;
        }
        result.replace_range(tag_start..tag_end, "");
    }
    result
}

fn replace_content_type(tag: &str, content_type: &str) -> String {
    const KEY: &str = "ContentType=\"";
    let mut result = tag.to_string();
    if let Some(pos) = result.find(KEY) {
        let value_start = pos + KEY.len();
        if let Some(rel_end) = result[value_start..].find('"') {
            result.replace_range(value_start..value_start + rel_end, content_type);
            return result;
        }
    }
    result.replacen("/>", &format!(" {}\"{}\"/>", KEY.trim_end_matches('"'), content_type), 1)
}

/// Fix the ContentType of an Override tag after a media file was renamed.
fn fix_content_type(s: &str, file_name: &str) -> String {
    let needle = format!("PartName=\"/ppt/media/{}\"", file_name);
    let Some(pos) = s.find(&needle) else {
        return s.to_string();
    };
    let Some(start) = s[..pos].rfind("<Override") else {
        return s.to_string();
    };
    let Some(rel_end) = s[pos..].find("/>") else {
        return s.to_string();
    };
    let end = pos + rel_end + 2;
    let mut result = s.to_string();
    let fixed = replace_content_type(&s[start..end], "video/mp4");
    result.replace_range(start..end, &fixed);
    result
}

/// Keep `[Content_Types].xml` and every `.rels` file in sync after a video
/// was converted from e.g. `media1.wmv` to `media1.mp4`.
pub fn rewrite_media_references(
    extract_dir: &Path,
    renames: &[(String, String)],
) -> io::Result<()> {
    if renames.is_empty() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_rels = name.ends_with(".rels");
        let is_ct = name == "[Content_Types].xml";
        if !is_rels && !is_ct {
            continue;
        }

        let original = fs::read_to_string(path)?;
        let mut updated = original.clone();
        for (old, new) in renames {
            if is_rels {
                updated = updated.replace(old.as_str(), new.as_str());
            } else {
                updated = updated.replace(
                    &format!("/ppt/media/{}", old),
                    &format!("/ppt/media/{}", new),
                );
                updated = fix_content_type(&updated, new);
            }
        }
        if is_ct
            && renames.iter().any(|(_, n)| n.ends_with(".mp4"))
            && !updated.contains("Extension=\"mp4\"")
        {
            // Insert inside <Types ...>, not after the XML declaration's `?>`.
            let insert_at = updated
                .find("<Types")
                .and_then(|p| updated[p..].find('>').map(|q| p + q + 1));
            if let Some(pos) = insert_at {
                updated.insert_str(
                    pos,
                    "<Default Extension=\"mp4\" ContentType=\"video/mp4\"/>",
                );
            }
        }
        if updated != original {
            fs::write(path, updated)?;
        }
    }
    Ok(())
}

/// Strip preview thumbnails and *all* references to them. Deleting the file
/// alone leaves a dangling relationship that makes PowerPoint offer to repair
/// the presentation.
pub fn strip_thumbnails(extract_dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let thumb_dir = extract_dir.join("docProps");
    let mut removed: Vec<String> = Vec::new();

    if let Ok(entries) = fs::read_dir(&thumb_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_lowercase().starts_with("thumbnail") && entry.path().is_file() {
                match fs::remove_file(entry.path()) {
                    Ok(()) => removed.push(name),
                    Err(e) => problems.push(format!("{}: {}", name, e)),
                }
            }
        }
    }
    if removed.is_empty() {
        return problems;
    }

    let root_rels = extract_dir.join("_rels").join(".rels");
    if let Ok(content) = fs::read_to_string(&root_rels) {
        let mut updated = content.clone();
        for name in &removed {
            updated = remove_tags_containing(
                &updated,
                &format!("Target=\"docProps/{}\"", name),
                "<Relationship",
                "/>",
            );
        }
        if updated != content {
            if let Err(e) = fs::write(&root_rels, updated) {
                problems.push(format!("_rels/.rels: {}", e));
            }
        }
    }

    let ct_path = extract_dir.join("[Content_Types].xml");
    if let Ok(content) = fs::read_to_string(&ct_path) {
        let mut updated = content.clone();
        for name in &removed {
            updated = remove_tags_containing(
                &updated,
                &format!("PartName=\"/docProps/{}\"", name),
                "<Override",
                "/>",
            );
        }
        if updated != content {
            if let Err(e) = fs::write(&ct_path, updated) {
                problems.push(format!("[Content_Types].xml: {}", e));
            }
        }
    }

    problems
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
        "jpg" | "jpeg" | "png" | "gif" | "mp4" | "m4v" | "wmv" | "avi" | "mov" | "webm"
            | "mpg" | "mpeg" | "asf" | "mp3" | "wav" | "ogg" | "zip"
    )
}
