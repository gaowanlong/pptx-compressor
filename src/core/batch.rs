//! 批量处理编排：把多个 PPTX 串行地「解包 → 压缩 → 重打包」。
//!
//! 设计取舍：文件之间保持**串行**。流水线内部已经对图片并行、对视频串行，
//! 再叠加文件级并行只会打爆内存并让进度与日志乱序。每个文件使用独立的
//! 临时解压目录与输出路径，单个文件失败只影响它自己，队列继续向下执行。
//!
//! 本模块不依赖 GUI，可以直接在集成测试里对真实 PPTX 做端到端验证。

use crate::core::extractor::extract_pptx_inspected;
use crate::core::pipeline::run_pipeline;
use crate::core::queue::unique_output_path;
use crate::core::{CompressMessage, CompressionSettings};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;

/// 后台批处理线程回传给 GUI 的消息。
#[derive(Debug, Clone)]
pub enum BatchMessage {
    /// (index, 文件名, 媒体数, 输入安全告警)
    JobStarted(usize, String, usize, Vec<String>),
    /// (index, 当前媒体序号, 媒体总数, 当前媒体名)
    JobProgress(usize, usize, usize, String),
    /// (index, 非致命日志)
    JobLog(usize, String),
    /// (index, 原始体积, 输出体积, 输出路径)
    JobFinished(usize, u64, u64, PathBuf),
    /// (index, 失败原因)
    JobFailed(usize, String),
    /// 队列全部结束（可能部分失败）
    Finished,
    /// 用户取消，后续文件不再处理
    Cancelled,
}

enum OneOutcome {
    Done,
    Failed(String),
    Cancelled,
}

/// 转发线程在单文件流水线结束时记录的终止状态。
#[derive(Default)]
struct JobTerminal {
    finished: bool,
    cancelled: bool,
}

/// 串行处理整个队列。失败被隔离为单个 `JobFailed`，不会中断后续文件。
///
/// `output_dir` 为 None 时，每个文件输出到它自己所在的目录；非空时统一输出到
/// 该目录。空队列会立即回传 `Finished`，不产生任何文件操作。
pub fn run_batch(
    inputs: &[PathBuf],
    output_dir: Option<&Path>,
    settings: &CompressionSettings,
    tx: Sender<BatchMessage>,
    cancel: Arc<AtomicBool>,
) {
    for (idx, input) in inputs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            let _ = tx.send(BatchMessage::Cancelled);
            return;
        }
        match process_one(idx, input, output_dir, settings, &tx, &cancel) {
            OneOutcome::Done => {}
            OneOutcome::Failed(err) => {
                let _ = tx.send(BatchMessage::JobFailed(idx, err));
            }
            OneOutcome::Cancelled => {
                let _ = tx.send(BatchMessage::Cancelled);
                return;
            }
        }
    }
    let _ = tx.send(BatchMessage::Finished);
}

/// 处理单个文稿：独立临时目录、独立输出路径、独立消息流。
fn process_one(
    idx: usize,
    input: &Path,
    output_dir: Option<&Path>,
    settings: &CompressionSettings,
    tx: &Sender<BatchMessage>,
    cancel: &Arc<AtomicBool>,
) -> OneOutcome {
    let name = input
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let original_size = fs::metadata(input).map(|m| m.len()).unwrap_or(0);

    let temp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => return OneOutcome::Failed(format!("创建临时目录失败：{e}")),
    };
    let extract_dir = temp.path().to_path_buf();

    let (mut media, inspection) = match extract_pptx_inspected(input, &extract_dir) {
        Ok(v) => v,
        Err(e) => return OneOutcome::Failed(format!("读取文稿失败：{e}")),
    };

    if cancel.load(Ordering::Relaxed) {
        return OneOutcome::Cancelled;
    }

    let warnings = inspection.warning_messages();
    let media_count = media.len();
    let _ = tx.send(BatchMessage::JobStarted(
        idx,
        name.clone(),
        media_count,
        warnings,
    ));

    let dir = output_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| input.parent().unwrap_or(input).to_path_buf());
    if let Err(e) = fs::create_dir_all(&dir) {
        return OneOutcome::Failed(format!("创建输出目录失败：{e}"));
    }
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let output_path = unique_output_path(&dir, &stem);

    // 流水线只认识 CompressMessage；起一个轻量转发线程把它实时翻译成
    // 带 index 的 BatchMessage，让 GUI 的队列与总进度不需要等整个文件跑完。
    let (job_tx, job_rx) = mpsc::channel::<CompressMessage>();
    let forward_tx = tx.clone();
    let forward_output = output_path.clone();
    let forward = std::thread::spawn(move || {
        let mut terminal = JobTerminal::default();
        for msg in job_rx {
            match msg {
                CompressMessage::Progress(current, total, file) => {
                    let _ =
                        forward_tx.send(BatchMessage::JobProgress(idx, current, total, file));
                }
                CompressMessage::Error(line) => {
                    let _ = forward_tx.send(BatchMessage::JobLog(idx, line));
                }
                CompressMessage::Finished(original, compressed) => {
                    terminal.finished = true;
                    let _ = forward_tx.send(BatchMessage::JobFinished(
                        idx,
                        original,
                        compressed,
                        forward_output.clone(),
                    ));
                }
                CompressMessage::Cancelled => terminal.cancelled = true,
                // FatalError 由 run_pipeline 返回 Err 后统一转成 JobFailed；
                // StatusUpdate 属于单文件媒体清单，批量模式不逐项展示。
                CompressMessage::FatalError(_) | CompressMessage::StatusUpdate(..) => {}
            }
        }
        terminal
    });

    let result = run_pipeline(
        &extract_dir,
        &output_path,
        &mut media,
        settings,
        original_size,
        job_tx,
        Arc::clone(cancel),
    );
    let terminal = forward.join().unwrap_or_default();

    // 解压目录随 tempdir 在函数返回时自动清理，无需手工回滚。
    if cancel.load(Ordering::Relaxed) || terminal.cancelled {
        let _ = fs::remove_file(&output_path);
        return OneOutcome::Cancelled;
    }
    if let Err(e) = result {
        return OneOutcome::Failed(e);
    }
    if !terminal.finished || !output_path.exists() {
        let _ = fs::remove_file(&output_path);
        return OneOutcome::Failed("压缩流程提前结束，未生成输出文件".into());
    }
    OneOutcome::Done
}
