//! 批量队列的领域模型。
//!
//! 这一层刻意不依赖 GUI、egui 或文件系统写入：所有状态迁移都是纯数据操作，
//! 因此「失败隔离」「总进度」「汇总」等语义可以被单元测试完整覆盖。
//! 类型实现 `Serialize` / `Deserialize`，为后续 CLI 结构化结果输出预留接口。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 单个文稿在队列中的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
}

impl JobStatus {
    pub fn label_zh(self) -> &'static str {
        match self {
            JobStatus::Queued => "等待中",
            JobStatus::Running => "处理中",
            JobStatus::Done => "已完成",
            JobStatus::Failed => "失败",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Done | JobStatus::Failed)
    }
}

/// 队列中一个 PPTX 的处理记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJob {
    pub index: usize,
    pub name: String,
    pub path: PathBuf,
    pub input_size: u64,
    pub output_size: u64,
    pub status: JobStatus,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub output_path: Option<PathBuf>,
    pub media_count: usize,
}

impl BatchJob {
    fn new(index: usize, path: PathBuf) -> Self {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let input_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self {
            index,
            name,
            path,
            input_size,
            output_size: 0,
            status: JobStatus::Queued,
            error: None,
            warnings: Vec::new(),
            output_path: None,
            media_count: 0,
        }
    }
}

/// 批量队列汇总。
///
/// 总体积只统计**成功**的文稿：失败项没有输出，若把它们的原始体积计入
/// 分母，会让「节省比例」看起来凭空多出一大截，属于误导性数字。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub total_original: u64,
    pub total_compressed: u64,
}

impl BatchSummary {
    pub fn saved(self) -> u64 {
        self.total_original.saturating_sub(self.total_compressed)
    }

    pub fn saved_pct(self) -> u8 {
        if self.total_original == 0 {
            return 0;
        }
        ((self.saved() as f64 / self.total_original as f64) * 100.0).min(100.0) as u8
    }
}

/// 批量队列。所有变更方法对越界索引静默忽略，保证后台消息再乱序也不会 panic。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchQueue {
    pub jobs: Vec<BatchJob>,
}

impl BatchQueue {
    /// 按给定顺序构造队列；重复路径只保留第一次出现的位置。
    pub fn from_paths(paths: &[PathBuf]) -> Self {
        let mut seen: Vec<PathBuf> = Vec::new();
        let mut jobs = Vec::new();
        for path in paths {
            if seen.contains(path) {
                continue;
            }
            seen.push(path.clone());
            jobs.push(BatchJob::new(jobs.len(), path.clone()));
        }
        Self { jobs }
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// 重新排队（用于「再压一次」或取消后重试）：保留文件列表，清空结果。
    pub fn reset(&mut self) {
        for job in &mut self.jobs {
            job.status = JobStatus::Queued;
            job.output_size = 0;
            job.output_path = None;
            job.error = None;
            job.warnings.clear();
            job.media_count = 0;
        }
    }

    /// 移除一个文稿并重新编号，避免后台消息里的 index 与队列错位。
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.jobs.len() {
            return false;
        }
        self.jobs.remove(index);
        for (i, job) in self.jobs.iter_mut().enumerate() {
            job.index = i;
        }
        true
    }

    pub fn mark_running(&mut self, index: usize) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.status = JobStatus::Running;
        }
    }

    pub fn set_warnings(&mut self, index: usize, warnings: Vec<String>) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.warnings = warnings;
        }
    }

    pub fn set_media_count(&mut self, index: usize, count: usize) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.media_count = count;
        }
    }

    pub fn mark_done(
        &mut self,
        index: usize,
        original: u64,
        compressed: u64,
        output_path: PathBuf,
    ) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.status = JobStatus::Done;
            job.input_size = original;
            job.output_size = compressed;
            job.output_path = Some(output_path);
            job.error = None;
        }
    }

    pub fn mark_failed(&mut self, index: usize, error: String) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.status = JobStatus::Failed;
            job.error = Some(error);
            job.output_size = 0;
            job.output_path = None;
        }
    }

    pub fn finished_count(&self) -> usize {
        self.jobs.iter().filter(|j| j.status.is_terminal()).count()
    }

    pub fn succeeded_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.status == JobStatus::Done)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.status == JobStatus::Failed)
            .count()
    }

    pub fn is_finished(&self) -> bool {
        !self.jobs.is_empty() && self.jobs.iter().all(|j| j.status.is_terminal())
    }

    /// 文件级总进度（不含当前文件内部的媒体进度）。
    pub fn progress_pct(&self) -> u8 {
        if self.jobs.is_empty() {
            return 0;
        }
        ((self.finished_count().min(self.jobs.len()) as f64 / self.jobs.len() as f64) * 100.0)
            as u8
    }

    pub fn summary(&self) -> BatchSummary {
        let mut summary = BatchSummary {
            total: self.jobs.len(),
            ..BatchSummary::default()
        };
        for job in &self.jobs {
            match job.status {
                JobStatus::Done => {
                    summary.succeeded += 1;
                    summary.total_original =
                        summary.total_original.saturating_add(job.input_size);
                    summary.total_compressed =
                        summary.total_compressed.saturating_add(job.output_size);
                }
                JobStatus::Failed => summary.failed += 1,
                JobStatus::Queued | JobStatus::Running => {}
            }
        }
        summary
    }

    /// 第一个成功输出，供「打开结果文件夹 / 复制路径」使用。
    pub fn first_output(&self) -> Option<PathBuf> {
        self.jobs.iter().find_map(|job| {
            if job.status == JobStatus::Done {
                job.output_path.clone()
            } else {
                None
            }
        })
    }
}

/// 把「已完成文件数」与「当前文件内部媒体进度」合成为平滑的 0–100 总进度。
///
/// 每个文件平均占 `100/total`；正在处理的文件按 `current/current_total` 补足
/// 这一份里的比例，避免界面在长文稿上长时间停在同一个整数百分比。
pub fn combined_progress_pct(
    finished: usize,
    total: usize,
    current: usize,
    current_total: usize,
) -> u8 {
    if total == 0 {
        return 0;
    }
    let per_file = 100.0 / total as f64;
    let base = finished.min(total) as f64 * per_file;
    let frac = if current_total == 0 {
        0.0
    } else {
        (current.min(current_total) as f64 / current_total as f64).min(1.0)
    };
    (base + per_file * frac).min(100.0) as u8
}

/// 从不覆盖已有结果：`<原名>_compressed.pptx`，重名时追加序号。
pub fn unique_output_path(dir: &Path, stem: &str) -> PathBuf {
    let base = dir.join(format!("{stem}_compressed.pptx"));
    if !base.exists() {
        return base;
    }
    for i in 2..1000 {
        let candidate = dir.join(format!("{stem}_compressed_{i}.pptx"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}
