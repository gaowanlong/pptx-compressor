//! Video compression via FFmpeg subprocess.
//!
//! FFmpeg is auto-downloaded on first use (one-time ~30MB download).

use crate::core::CompressionSettings;
use crate::utils::ffmpeg::{download_ffmpeg, find_ffmpeg};
use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Compress a video file using FFmpeg with H.264 encoding.
/// Automatically downloads FFmpeg on first use if not found on PATH or bundled.
pub fn compress_video(path: &Path, settings: &CompressionSettings) -> Result<(), String> {
    let ffmpeg = match find_ffmpeg() {
        Some(f) => f,
        None => {
            // Auto-download on first use (one-time ~30MB download)
            download_ffmpeg(|_, _| {}).map_err(|e| {
                format!("FFmpeg auto-download failed: {e}")
            })?;
            // Check again after download
            find_ffmpeg().ok_or("FFmpeg download completed but binary not found")?
        }
    };

    let tmp_path = path.with_extension("tmp.mp4");

    // video_quality is 0-100 (higher = better). Convert to CRF for FFmpeg.
    // quality 0 → CRF 51 (worst), quality 100 → CRF 0 (best/lossless).
    let crf = 51 - (settings.video_quality as u32 * 51 / 100).min(51) as u8;

    let mut args: Vec<String> = vec![
        "-y".into(),                  // Overwrite without asking
        "-i".into(),
        path.to_string_lossy().into_owned(),
        "-c:v".into(),
        "libx264".into(),
        "-crf".into(),
        crf.to_string(),
        "-preset".into(),
        "medium".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-movflags".into(),
        "+faststart".into(),
    ];

    // Add resolution scaling if enabled
    if settings.video_max_resolution_enabled {
        // scale=-2:'min(ih,MAX_HEIGHT)' ensures width is even and height ≤ max
        let filter = format!(
            "scale=-2:'min(ih,{})'",
            settings.video_max_height
        );
        args.push("-vf".into());
        args.push(filter);
    }

    args.push(tmp_path.to_string_lossy().into_owned());

    let mut cmd = Command::new(&ffmpeg);
    cmd.args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // On Windows, hide the console window that FFmpeg spawns
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output()
        .map_err(|e| format!("Failed to run FFmpeg: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Clean up temp file on failure
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("FFmpeg failed: {}", stderr.lines().last().unwrap_or("unknown error")));
    }

    // Check if compressed file exists and is smaller
    if !tmp_path.exists() {
        return Err("FFmpeg produced no output".into());
    }

    // Replace original
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed to replace original video: {}", e))?;

    Ok(())
}
