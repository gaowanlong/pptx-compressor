//! FFmpeg detection and auto-download.
//!
//! Detection order:
//! 1. Bundled with the app (relative to the executable / .app bundle)
//! 2. Development resources directory (when running from `cargo`)
//! 3. System PATH (e.g. Homebrew on macOS)
//!
//! Fallback: auto-download on first use if not found anywhere else.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Platform-specific FFmpeg binary name.
#[cfg(target_os = "windows")]
const FFMPEG_BIN: &str = "ffmpeg.exe";
#[cfg(not(target_os = "windows"))]
const FFMPEG_BIN: &str = "ffmpeg";

/// Known FFmpeg download URL for Windows x64 (essentials build).
/// macOS users get the binary bundled in the .app or via auto-download fallback.
#[cfg(target_os = "windows")]
const FFMPEG_DOWNLOAD_URL: &str =
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
#[cfg(not(target_os = "windows"))]
const FFMPEG_DOWNLOAD_URL: &str =
    "https://evermeet.cx/ffmpeg/get/zip";

/// Enumerate places where the bundled FFmpeg binary might live,
/// in priority order (most-specific first).
fn ffmpeg_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // --- macOS .app bundle ---
            // Layout: .app/Contents/MacOS/pptx-compressor
            //         .app/Contents/Resources/ffmpeg
            #[cfg(target_os = "macos")]
            if exe_dir.ends_with("MacOS") {
                if let Some(contents) = exe_dir.parent() {
                    paths.push(contents.join("Resources").join(FFMPEG_BIN));
                }
            }

            // --- Same directory as executable (Windows distribution) ---
            #[cfg(target_os = "windows")]
            paths.push(exe_dir.join(FFMPEG_BIN));

            // --- Development from Cargo workspace ---
            // Layout: target/<profile>/pptx-compressor
            //         resources/ffmpeg/<platform>/ffmpeg
            if let Some(target_dir) = exe_dir.parent() {
                if let Some(project_dir) = target_dir.parent() {
                    #[cfg(target_os = "macos")]
                    paths.push(
                        project_dir
                            .join("resources")
                            .join("ffmpeg")
                            .join("macos")
                            .join(FFMPEG_BIN),
                    );
                    #[cfg(target_os = "windows")]
                    paths.push(
                        project_dir
                            .join("resources")
                            .join("ffmpeg")
                            .join("windows")
                            .join(FFMPEG_BIN),
                    );
                }
            }
        }
    }

    paths
}

/// Check if FFmpeg is available (bundled, on PATH, or in known dev locations).
/// Returns the path / command to use.
pub fn find_ffmpeg() -> Option<String> {
    // 1. Check candidate paths (bundled .app, dev resources, etc.)
    for path in ffmpeg_candidate_paths() {
        if path.is_file() {
            // Quick sanity: ensure it's executable
            #[cfg(not(target_os = "windows"))]
            {
                use std::os::unix::fs::PermissionsExt;
                if path.metadata().ok().map_or(false, |m| m.permissions().mode() & 0o111 == 0) {
                    continue;
                }
            }
            return Some(path.to_string_lossy().into_owned());
        }
    }

    // 2. Check system PATH (Homebrew, system install, etc.)
    if let Ok(output) = Command::new(FFMPEG_BIN).arg("-version").output() {
        if output.status.success() {
            return Some(FFMPEG_BIN.into());
        }
    }

    None
}

/// Download FFmpeg to a known location using curl.
///
/// On Windows downloads from gyan.dev (essentials .zip), extracts `ffmpeg.exe`.
/// On macOS downloads from evermeet.cx (static build .zip), extracts `ffmpeg`.
pub fn download_ffmpeg(progress: impl Fn(u64, u64)) -> Result<PathBuf, String> {
    let app_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pptx-compressor")
        .join("ffmpeg");

    std::fs::create_dir_all(&app_dir).map_err(|e| format!("Failed to create directory: {e}"))?;

    let zip_path = app_dir.join("ffmpeg.zip");

    // -------- Download via curl --------
    let curl_binary = if cfg!(windows) { "curl.exe" } else { "curl" };

    let mut child = Command::new(curl_binary)
        .args([
            "-L",
            "--fail",
            "-o",
            &zip_path.to_string_lossy(),
            FFMPEG_DOWNLOAD_URL,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch curl: {e}"))?;

    // Poll file size periodically for progress reporting
    let sleep_dur = Duration::from_millis(200);
    let metadata_interval = 10;
    let mut poll_count = 0u32;
    let total_size: u64 = guess_content_length().unwrap_or(0);

    let mut stderr_buf = Vec::new();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = child.stderr.take().map(|mut s| {
                    let _ = s.read_to_end(&mut stderr_buf);
                });
                if !status.success() {
                    let stderr_msg = String::from_utf8_lossy(&stderr_buf);
                    return Err(format!(
                        "curl download failed (exit: {}): {}",
                        status.code().unwrap_or(-1),
                        stderr_msg.trim()
                    ));
                }
                break;
            }
            Ok(None) => {
                poll_count += 1;
                if poll_count % metadata_interval == 0 {
                    if let Ok(meta) = std::fs::metadata(&zip_path) {
                        progress(meta.len(), total_size);
                    }
                }
                std::thread::sleep(sleep_dur);
            }
            Err(e) => {
                return Err(format!("Failed to wait for curl: {e}"));
            }
        }
    }

    let actual_size = std::fs::metadata(&zip_path)
        .map(|m| m.len())
        .unwrap_or(0);
    progress(
        actual_size,
        if total_size > 0 {
            total_size
        } else {
            actual_size
        },
    );

    // -------- Extract via zip crate --------
    let extract_dir = app_dir.join("extracted");
    let zip_file =
        std::fs::File::open(&zip_path).map_err(|e| format!("Failed to open zip: {e}"))?;

    let mut archive =
        zip::ZipArchive::new(zip_file).map_err(|e| format!("Failed to read zip archive: {e}"))?;

    std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let out_path = extract_dir.join(entry.name());
        if entry.name().ends_with('/') {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut outfile =
                std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
        }
    }

    // Find binary in extracted contents
    let binary = find_file_recursive(&extract_dir, FFMPEG_BIN)
        .ok_or(format!("{} not found in archive", FFMPEG_BIN))?;

    let target = app_dir.join(FFMPEG_BIN);
    std::fs::copy(&binary, &target).map_err(|e| e.to_string())?;

    // Also copy ffprobe if found
    let ffprobe = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    if let Some(fp) = find_file_recursive(&extract_dir, ffprobe) {
        let _ = std::fs::copy(&fp, app_dir.join(ffprobe));
    }

    // Clean up
    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_file(&zip_path);

    // Make executable on Unix
    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&target) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            let _ = std::fs::set_permissions(&target, perms);
        }
    }

    Ok(target)
}

/// Try to estimate the content length via a HEAD request using curl.
fn guess_content_length() -> Option<u64> {
    let curl_binary = if cfg!(windows) { "curl.exe" } else { "curl" };
    let output = Command::new(curl_binary)
        .args(["-sI", "-L", FFMPEG_DOWNLOAD_URL])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let headers = String::from_utf8_lossy(&output.stdout);
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            if let Some(val) = line.split(':').nth(1) {
                if let Ok(len) = val.trim().parse::<u64>() {
                    return Some(len);
                }
            }
        }
    }
    None
}

fn find_file_recursive(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy() == name {
            return Some(entry.into_path());
        }
    }
    None
}
