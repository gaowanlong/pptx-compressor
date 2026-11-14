//! 视频压缩（FFmpeg 子进程）。
//!
//! 本版不再随包分发 FFmpeg，也不会在运行时偷偷下载二进制。FFmpeg 由用户
//! 本机提供：手动指定路径 → 随包副本（若维护者用合规构建打包）→ 用户数据
//! 目录 → 系统 PATH。找不到时视频项会明确失败并在活动日志里说明原因。
//!
//! LGPL 构建通常不含 libx264，因此这里会在运行时探测可用的 H.264 编码器，
//! 在 libx264（CRF 质量模型）与 libopenh264（码率模型）之间自动切换。

use crate::core::{crf_from_quality, CompressionSettings};
use crate::handlers::{CompressCandidate, CANCELLED};
use crate::utils::ffmpeg::{encoder_support, locate_current, EncoderSupport};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// libopenh264 不支持 CRF，把 0–100 的质量滑杆映射成目标码率（kbit/s）。
pub fn bitrate_kbps(quality: u8) -> u32 {
    200 + u32::from(quality) * 40
}

/// 构造 FFmpeg 命令行。抽成纯函数便于在没有 FFmpeg 的机器上做回归测试。
pub fn build_video_args(
    input: &Path,
    output: &Path,
    settings: &CompressionSettings,
    encoder: &str,
) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-c:v".into(),
        encoder.into(),
    ];

    match encoder {
        "libx264" => {
            args.extend([
                "-crf".into(),
                crf_from_quality(settings.video_quality).to_string(),
                "-preset".into(),
                "medium".into(),
                "-profile:v".into(),
                "high".into(),
                "-refs".into(),
                "3".into(),
            ]);
        }
        "libopenh264" => {
            args.extend([
                "-b:v".into(),
                format!("{}k", bitrate_kbps(settings.video_quality)),
            ]);
        }
        other => return Err(format!("不支持的 H.264 编码器：{other}")),
    }

    args.extend([
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-movflags".into(),
        "+faststart".into(),
    ]);

    if settings.video_max_resolution_enabled {
        args.push("-vf".into());
        args.push(format!(
            "scale=-2:'min(ih,{})',format=yuv420p",
            settings.video_max_height
        ));
    } else {
        // yuv420p 保证 PowerPoint 能播放；H.264 level 交给编码器决定，
        // 避免 4K 素材被错误标成 Level 4.0。
        args.push("-pix_fmt".into());
        args.push("yuv420p".into());
    }

    args.push(output.to_string_lossy().into_owned());
    Ok(args)
}

fn choose_encoder(support: EncoderSupport) -> Result<&'static str, String> {
    support.preferred().ok_or_else(|| {
        "当前 FFmpeg 缺少 H.264 编码器（libx264 / libopenh264），无法压缩视频。\
         请改用带有 H.264 编码器的 FFmpeg 构建。"
            .into()
    })
}

/// 把视频重编码成 H.264/AAC MP4 写入 `candidate`。
///
/// 子进程被轮询，用户取消时可以真正终止长任务而不是等 FFmpeg 跑完。
pub fn compress_video(
    path: &Path,
    candidate: &Path,
    settings: &CompressionSettings,
    cancel: &AtomicBool,
) -> Result<CompressCandidate, String> {
    let located = locate_current().ok_or_else(|| {
        "未找到 FFmpeg，视频压缩已跳过。图片、GIF 与 XML 清理不受影响。\
         请在右下角「环境说明」里选择本机 FFmpeg，或安装后重启应用。"
            .to_string()
    })?;
    let encoder = choose_encoder(encoder_support(&located.command))?;
    let args = build_video_args(path, candidate, settings, encoder)?;

    // stderr 重定向到文件：无人读取的管道缓冲区写满会让 FFmpeg 死锁。
    let log_path = candidate.with_extension("ffmpeg.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("创建 FFmpeg 日志失败：{}", e))?;

    let mut cmd = Command::new(&located.command);
    cmd.args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file));

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
