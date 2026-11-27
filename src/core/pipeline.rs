//! 压缩流水线：编排媒体压缩与 PPTX 重打包。
//!
//! 两阶段设计：
//! 1. **并行生成候选**：handler 只往私有工作目录写文件，绝不碰解压树。
//!    图片 / GIF 用受限线程池并行，视频转码保持串行（FFmpeg 自身已多线程，
//!    并发多个进程会打爆内存）。
//! 2. **串行裁决与安装**：只有这一个地方决定是否替换原件，并同步更新
//!    `MediaInfo`、OOXML 引用与活动日志，保证顺序稳定、可回滚。
//!
//! 附带好处：取消时解压树尚未被修改，直接丢弃工作目录即可，无需逐文件回滚。

use crate::core::{self, CompressMessage, CompressionSettings, MediaInfo, MediaStatus, MediaType};
use crate::handlers::{gif_handler, image, video, xml_clean, CompressCandidate, CANCELLED};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// 图片并行上限。解码大图很吃内存，盲目用满所有核心反而更慢；
/// 鸿蒙 PC 上进一步降到 2 线程（见 `core::platform`）。
fn max_image_threads() -> usize {
    core::platform::max_image_threads()
}

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
    fs::copy(candidate, target).map_err(|e| format!("写入压缩结果失败：{}", e))?;
    if target != original {
        fs::remove_file(original).map_err(|e| format!("删除旧媒体文件失败：{}", e))?;
    }
    Ok(())
}

/// `Sender` 的实现细节不保证跨线程共享引用，统一用 Mutex 包一层。
type SharedSender = Arc<Mutex<Sender<CompressMessage>>>;

fn send(tx: &SharedSender, msg: CompressMessage) {
    if let Ok(guard) = tx.lock() {
        let _ = guard.send(msg);
    }
}

struct WorkItem {
    idx: usize,
    original_path: PathBuf,
    file_name: String,
    media_type: MediaType,
    candidate: PathBuf,
    backup: PathBuf,
    skip_reason: Option<String>,
}

enum WorkResult {
    Candidate(CompressCandidate),
    Failed(String),
    Skipped(String),
    Cancelled,
}

struct WorkOutcome {
    idx: usize,
    result: WorkResult,
}

/// 在后台线程里跑完整流水线。
///
/// 1. 并行把每个启用的媒体压成候选文件（视频串行）。
/// 2. 串行裁决：仅当候选严格更小时才替换，替换前已有备份可回滚。
/// 3. 视频容器改名时同步全部 OOXML 声明（含非 `ppt/media/` 路径）。
/// 4. 剥 XML 注释与预览缩略图（连同关系），并清理其余悬空关系。
/// 5. 原子重打包并回读校验。
pub fn run_pipeline(
    extract_dir: &Path,
    output_path: &Path,
    media_files: &mut [MediaInfo],
    settings: &CompressionSettings,
    original_pptx_size: u64,
    tx: Sender<CompressMessage>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let work_dir = core::platform::tempdir().map_err(|e| format!("创建工作目录失败：{}", e))?;
    let tx: SharedSender = Arc::new(Mutex::new(tx));
    let cancel: &AtomicBool = cancel.as_ref();

    let items: Vec<WorkItem> = media_files
        .iter()
        .enumerate()
        .filter(|(_, m)| m.enabled && m.media_type.is_compressible())
        .map(|(idx, m)| {
            let original_path = m.extracted_path.clone();
            let file_name = original_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let skip_reason = if !core::platform::video_transcode_available()
                && m.media_type == MediaType::Video
            {
                Some(format!(
                    "已跳过 {}：当前平台（鸿蒙电脑）不提供 FFmpeg 子进程，视频保持原样",
                    file_name
                ))
            } else if m.media_type == MediaType::Video
                && !settings.video_convert_to_h264
                && !is_mp4_container(&original_path)
            {
                // 不开兼容模式就不能重写非 MP4 容器：FFmpeg 只会产出 MP4，
                // 与文件扩展名不符会让 PowerPoint 报损坏。
                Some(format!("已跳过 {}：处理非 MP4 视频需开启兼容模式", file_name))
            } else {
                None
            };
            WorkItem {
                idx,
                candidate: candidate_path(work_dir.path(), idx, &original_path, m.media_type),
                backup: backup_path(work_dir.path(), idx),
                original_path,
                file_name,
                media_type: m.media_type,
                skip_reason,
            }
        })
        .collect();
    let total = items.len();

    let progress = Arc::new(AtomicUsize::new(0));
    let process = |item: &WorkItem| -> WorkOutcome {
        if cancel.load(Ordering::Relaxed) {
            return WorkOutcome { idx: item.idx, result: WorkResult::Cancelled };
        }
        send(&tx, CompressMessage::StatusUpdate(item.idx, MediaStatus::Processing, 0));

        let result = if let Some(reason) = &item.skip_reason {
            WorkResult::Skipped(reason.clone())
        } else if let Err(e) = fs::copy(&item.original_path, &item.backup) {
            WorkResult::Failed(format!("备份 {} 失败：{}", item.file_name, e))
        } else {
            let outcome = match item.media_type {
                MediaType::Jpeg => image::compress_jpeg(&item.original_path, &item.candidate, settings),
                MediaType::Png => image::compress_png(&item.original_path, &item.candidate, settings),
                MediaType::Gif => gif_handler::compress_gif(&item.original_path, &item.candidate, settings),
                MediaType::Video => video::compress_video(
                    &item.original_path,
                    &item.candidate,
                    settings,
                    cancel,
                ),
                // `is_compressible()` 已排除 Other；保留分支只为 match 完整，
                // 指向空候选会自然走「变小才替换」的 Skipped 路径。
                MediaType::Other => Ok(CompressCandidate {
                    path: item.candidate.clone(),
                    new_extension: None,
                }),
            };
            match outcome {
                Ok(c) => WorkResult::Candidate(c),
                Err(e) if e == CANCELLED => WorkResult::Cancelled,
                Err(e) => WorkResult::Failed(e),
            }
        };

        let done = progress.fetch_add(1, Ordering::Relaxed) + 1;
        send(&tx, CompressMessage::Progress(done, total, item.file_name.clone()));
        WorkOutcome { idx: item.idx, result }
    };

    // 图片 / GIF 并行；视频转码串行，避免多个 FFmpeg 进程同时吃满内存。
    let (parallel_items, video_items): (Vec<WorkItem>, Vec<WorkItem>) = items
        .into_iter()
        .partition(|w| w.media_type != MediaType::Video);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(max_image_threads().min(parallel_items.len().max(1)))
        .build()
        .map_err(|e| format!("创建压缩线程池失败：{}", e))?;

    let mut outcomes: Vec<WorkOutcome> = pool.install(|| {
        parallel_items
            .par_iter()
            .map(&process)
            .collect()
    });
    outcomes.extend(video_items.iter().map(process));
    outcomes.sort_by_key(|o| o.idx);

    let mut renames: Vec<(String, String)> = Vec::new();
    for outcome in outcomes {
        if cancel.load(Ordering::Relaxed) {
            send(&tx, CompressMessage::Cancelled);
            return Ok(());
        }

        let idx = outcome.idx;
        let original_path = media_files[idx].extracted_path.clone();
        let file_name = original_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let original_size = media_files[idx].original_size;
        let backup = backup_path(work_dir.path(), idx);

        match outcome.result {
            WorkResult::Cancelled => {
                send(&tx, CompressMessage::Cancelled);
                return Ok(());
            }
            WorkResult::Skipped(reason) => {
                media_files[idx].compressed_size = original_size;
                send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Skipped, original_size));
                if !reason.is_empty() {
                    send(&tx, CompressMessage::Error(reason));
                }
            }
            WorkResult::Failed(e) => {
                let _ = fs::remove_file(candidate_path(work_dir.path(), idx, &original_path, media_files[idx].media_type));
                media_files[idx].compressed_size = original_size;
                send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Failed, original_size));
                send(&tx, CompressMessage::Error(format!("压缩 {} 失败：{}", file_name, e)));
            }
            WorkResult::Candidate(candidate) => {
                let new_size = fs::metadata(&candidate.path).map(|m| m.len()).unwrap_or(0);
                if !should_replace(original_size, new_size) {
                    let _ = fs::remove_file(&candidate.path);
                    media_files[idx].compressed_size = original_size;
                    send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Skipped, original_size));
                    continue;
                }

                let target = unique_target(&original_path, candidate.new_extension.as_deref());
                if let Err(e) = install_candidate(&candidate.path, &original_path, &target) {
                    // 从备份回滚，避免输出里出现半写入或变大的媒体文件。
                    let _ = fs::copy(&backup, &original_path);
                    media_files[idx].compressed_size = original_size;
                    send(&tx, CompressMessage::StatusUpdate(idx, MediaStatus::Failed, original_size));
                    send(&tx, CompressMessage::Error(format!("替换 {} 失败：{}", file_name, e)));
                    continue;
                }

                if target != original_path {
                    renames.push((
                        file_name.clone(),
                        target
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    ));
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
        }
    }

    if cancel.load(Ordering::Relaxed) {
        send(&tx, CompressMessage::Cancelled);
        return Ok(());
    }

    if !renames.is_empty() {
        crate::core::extractor::rewrite_media_references(extract_dir, &renames)
            .map_err(|e| format!("更新 OOXML 媒体引用失败：{}", e))?;
    }

    for problem in xml_clean::clean_xml_files(extract_dir) {
        send(&tx, CompressMessage::Error(format!("XML 清理：{}", problem)));
    }
    for problem in crate::core::extractor::strip_thumbnails(extract_dir) {
        send(&tx, CompressMessage::Error(format!("缩略图清理：{}", problem)));
    }

    // 打包前收尾：删除指向不存在部件的 .rels 关系与失效的 Override。
    // 失败只记一条非致命日志，绝不因为「体检没做成」而阻断已经压好的包。
    match crate::core::ooxml::prune_dangling_relationships(extract_dir) {
        Ok(report) if !report.is_empty() => send(
            &tx,
            CompressMessage::Error(format!(
                "引用完整性：已清理 {} 条悬空关系、{} 条失效内容类型声明（非致命）",
                report.removed_relationships, report.removed_overrides
            )),
        ),
        Ok(_) => {}
        Err(e) => send(
            &tx,
            CompressMessage::Error(format!("引用完整性检查未完成：{}（非致命）", e)),
        ),
    }

    send(&tx, CompressMessage::Progress(total, total, "正在重新打包…".into()));
    crate::core::extractor::repack_pptx(extract_dir, output_path)
        .map_err(|e| format!("重新打包 PPTX 失败：{}", e))?;

    let output_size = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    send(&tx, CompressMessage::Finished(original_pptx_size, output_size));
    Ok(())
}
