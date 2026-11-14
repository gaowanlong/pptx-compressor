//! Video compression via FFmpeg subprocess.
//!
//! FFmpeg is bundled with the release packages. When it is missing (e.g. a
//! development build) the app tries a one-time download to the user data dir.

use crate::core::CompressionSettings;
use crate::handlers::{CompressCandidate, CANCELLED};
use crate::utils::ffmpeg::{download_ffmpeg, find_ffmpeg};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn locate_ffmpeg() -> Result<String, String> {
    if let Some(path) = find_ffmpeg() {
        return Ok(path);
    }
    download_ffmpeg(|_, _| {}).map_err(|e| format!("FFmpeg 自动下载失败：{e}"))?;
    find_ffmpeg().ok_or_else(|| {
        "FFmpeg download finished but the binary is still unavailable. \
         Install FFmpeg manually or use an official release package."
            .to_string()
    })
}

/// Encode a video to H.264/AAC MP4 into `candidate`.
///
/// The child process is polled so a user cancel can actually terminate a long
/// encode instead of waiting for FFmpeg to finish.
pub fn compress_video(
    path: &Path,
    candidate: &Path,
    settings: &CompressionSettings,
    cancel: &AtomicBool,
) -> Result<CompressCandidate, String> {
    let ffmpeg = locate_ffmpeg()?;
    let crf = crate::core::crf_from_quality(settings.video_quality);

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        path.to_string_lossy().into_owned(),
        "-c:v".into(),
        "libx264".into(),
        "-crf".into(),
        crf.to_string(),
        "-preset".into(),
        "medium".into(),
        "-profile:v".into(),
        "high".into(),
        "-refs".into(),
        "3".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-movflags".into(),
        "+faststart".into(),
    ];

    if settings.video_max_resolution_enabled {
        args.push("-vf".into());
        args.push(format!(
            "scale=-2:'min(ih,{})',format=yuv420p",
            settings.video_max_height
        ));
    } else {
        // yuv420p + High profile keeps playback working in PowerPoint.
        // The H.264 level is left to x264 so 4K input is not mislabelled 4.0.
        args.push("-pix_fmt".into());
        args.push("yuv420p".into());
    }

    args.push(candidate.to_string_lossy().into_owned());

    // Redirect stderr to a file: a piped stderr that nobody drains can fill
    // the OS pipe buffer and deadlock FFmpeg on verbose encodes.
    let log_path = candidate.with_extension("ffmpeg.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("创建 FFmpeg 日志失败：{}", e))?;

    let mut cmd = Command::new(&ffmpeg);
    cmd.args(&args).stdout(Stdio::null()).stderr(Stdio::from(log_file));

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("启动 FFmpeg 失败：{}", e))?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(candidate);
            let _ = std::fs::remove_file(&log_path);
            return Err(CANCELLED.to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(150)),
            Err(e) => return Err(format!("等待 FFmpeg 结束失败：{}", e)),
        }
    };

    if !status.success() {
        let stderr = std::fs::read_to_string(&log_path).unwrap_or_default();
        let _ = std::fs::remove_file(candidate);
        let _ = std::fs::remove_file(&log_path);
        return Err(format!(
            "FFmpeg 编码失败：{}",
            stderr.lines().last().unwrap_or("未知错误")
        ));
    }
    let _ = std::fs::remove_file(&log_path);

    let len = std::fs::metadata(candidate).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        let _ = std::fs::remove_file(candidate);
        return Err("FFmpeg 未生成输出文件".into());
    }

    let old_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let new_extension = if old_ext == "mp4" || old_ext == "m4v" {
        None
    } else {
        Some("mp4".to_string())
    };

    Ok(CompressCandidate { path: candidate.to_path_buf(), new_extension })
}
