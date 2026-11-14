//! 批量队列领域模型的单元测试：去重、失败隔离、汇总、进度与输出路径。
//! 不依赖 GUI、FFmpeg 或网络。

use pptx_compressor::core::queue::{
    combined_progress_pct, unique_output_path, BatchQueue, BatchSummary, JobStatus,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn sample_paths(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| {
            let path = dir.join(name);
            fs::write(&path, vec![0u8; 128]).unwrap();
            path
        })
        .collect()
}

#[test]
fn from_paths_deduplicates_preserves_order_and_reads_sizes() {
    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx", "b.pptx", "c.pptx"]);
    let input = vec![p[0].clone(), p[1].clone(), p[0].clone(), p[2].clone()];

    let queue = BatchQueue::from_paths(&input);

    assert_eq!(queue.len(), 3, "重复路径只保留第一次出现的位置");
    assert_eq!(queue.jobs[0].index, 0);
    assert_eq!(queue.jobs[1].index, 1);
    assert_eq!(queue.jobs[2].index, 2);
    assert_eq!(queue.jobs[0].name, "a.pptx");
    assert_eq!(queue.jobs[2].name, "c.pptx");
    assert!(queue.jobs.iter().all(|j| j.status == JobStatus::Queued));
    assert!(queue.jobs.iter().all(|j| j.input_size == 128));
    assert!(queue
        .jobs
        .iter()
        .all(|j| j.output_path.is_none() && j.error.is_none()));
}

#[test]
fn failure_is_isolated_to_a_single_job() {
    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx", "b.pptx", "c.pptx"]);
    let mut queue = BatchQueue::from_paths(&p);

    queue.mark_running(0);
    queue.mark_done(0, 1_000, 600, dir.path().join("a_out.pptx"));
    queue.mark_running(1);
    queue.mark_failed(1, "这是加密文稿".into());

    // 第 3 个文件与第 1 个文件的结果都不受中间失败影响。
    assert_eq!(queue.jobs[0].status, JobStatus::Done);
    assert_eq!(queue.jobs[1].status, JobStatus::Failed);
    assert_eq!(queue.jobs[2].status, JobStatus::Queued);
    assert_eq!(queue.finished_count(), 2);
    assert_eq!(queue.succeeded_count(), 1);
    assert_eq!(queue.failed_count(), 1);
    assert!(!queue.is_finished());

    queue.mark_done(2, 2_000, 1_500, dir.path().join("c_out.pptx"));
    assert!(queue.is_finished());
    assert_eq!(queue.progress_pct(), 100);
}

#[test]
fn summary_counts_only_successful_jobs() {
    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx", "b.pptx", "c.pptx"]);
    let mut queue = BatchQueue::from_paths(&p);

    queue.mark_done(0, 1_000, 500, dir.path().join("a_out.pptx"));
    queue.mark_failed(1, "损坏".into());
    queue.mark_done(2, 3_000, 2_000, dir.path().join("c_out.pptx"));

    let summary = queue.summary();
    assert_eq!(summary.total, 3);
    assert_eq!(summary.succeeded, 2);
    assert_eq!(summary.failed, 1);
    // 失败项的 128 字节不能混进分母，否则「节省比例」会失真。
    assert_eq!(summary.total_original, 4_000);
    assert_eq!(summary.total_compressed, 2_500);
    assert_eq!(summary.saved(), 1_500);
    assert_eq!(summary.saved_pct(), 37);
}

#[test]
fn progress_and_terminal_state_edge_cases() {
    let empty = BatchQueue::from_paths(&[]);
    assert!(empty.is_empty());
    assert_eq!(empty.progress_pct(), 0);
    assert!(!empty.is_finished());
    assert_eq!(empty.summary(), BatchSummary::default());

    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx", "b.pptx", "c.pptx", "d.pptx"]);
    let mut queue = BatchQueue::from_paths(&p);
    assert_eq!(queue.progress_pct(), 0);
    queue.mark_done(0, 100, 50, dir.path().join("o0.pptx"));
    assert_eq!(queue.progress_pct(), 25);
    queue.mark_failed(1, "坏包".into());
    assert_eq!(queue.progress_pct(), 50);
    queue.mark_done(2, 100, 50, dir.path().join("o2.pptx"));
    assert_eq!(queue.progress_pct(), 75);
    queue.mark_done(3, 100, 50, dir.path().join("o3.pptx"));
    assert_eq!(queue.progress_pct(), 100);
    assert!(queue.is_finished());
}

#[test]
fn combined_progress_is_smooth_and_clamped() {
    assert_eq!(combined_progress_pct(0, 0, 0, 0), 0);
    assert_eq!(combined_progress_pct(0, 2, 0, 0), 0);
    assert_eq!(combined_progress_pct(0, 2, 5, 10), 25);
    assert_eq!(combined_progress_pct(1, 2, 5, 10), 75);
    assert_eq!(combined_progress_pct(2, 2, 0, 0), 100);
    assert_eq!(combined_progress_pct(0, 3, 3, 3), 33);
    // 越界 / 已完成但当前进度仍非空的输入都必须收敛到 100。
    assert_eq!(combined_progress_pct(99, 2, 99, 1), 100);
    assert_eq!(combined_progress_pct(1, 1, 50, 0), 100);
}

#[test]
fn reset_clears_results_but_keeps_files() {
    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx", "b.pptx"]);
    let mut queue = BatchQueue::from_paths(&p);

    queue.mark_running(0);
    queue.mark_done(0, 100, 50, dir.path().join("a_out.pptx"));
    queue.set_warnings(1, vec!["包含数字签名".into()]);
    queue.set_media_count(1, 7);
    queue.mark_failed(1, "坏包".into());
    queue.reset();

    assert_eq!(queue.len(), 2);
    assert_eq!(queue.jobs[0].path, p[0]);
    assert_eq!(queue.jobs[1].path, p[1]);
    assert!(queue.jobs.iter().all(|j| j.status == JobStatus::Queued));
    assert!(queue
        .jobs
        .iter()
        .all(|j| j.output_size == 0 && j.output_path.is_none() && j.error.is_none()));
    assert!(queue
        .jobs
        .iter()
        .all(|j| j.warnings.is_empty() && j.media_count == 0));
    assert_eq!(queue.first_output(), None);
    assert_eq!(queue.progress_pct(), 0);
}

#[test]
fn remove_renumbers_and_rejects_bad_index() {
    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx", "b.pptx", "c.pptx"]);
    let mut queue = BatchQueue::from_paths(&p);

    assert!(!queue.remove(9));
    assert!(queue.remove(1));
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.jobs[0].index, 0);
    assert_eq!(queue.jobs[1].index, 1);
    assert_eq!(queue.jobs[0].name, "a.pptx");
    assert_eq!(queue.jobs[1].name, "c.pptx");
}

#[test]
fn out_of_range_mutations_are_noops() {
    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx"]);
    let mut queue = BatchQueue::from_paths(&p);

    // 后台消息乱序时也不能 panic 或改坏别的任务。
    queue.mark_running(7);
    queue.mark_done(7, 100, 50, dir.path().join("o.pptx"));
    queue.mark_failed(7, "坏包".into());
    queue.set_warnings(7, vec!["告警".into()]);
    queue.set_media_count(7, 3);

    assert_eq!(queue.jobs[0].status, JobStatus::Queued);
    assert_eq!(queue.jobs[0].media_count, 0);
    assert!(queue.jobs[0].warnings.is_empty());
    assert!(queue.jobs[0].error.is_none());
    assert_eq!(queue.progress_pct(), 0);
}

#[test]
fn unique_output_path_never_overwrites_existing_results() {
    let dir = tempdir().unwrap();

    let first = unique_output_path(dir.path(), "deck");
    assert_eq!(first.file_name().unwrap(), "deck_compressed.pptx");
    assert!(!first.exists());
    fs::write(&first, b"first").unwrap();

    let second = unique_output_path(dir.path(), "deck");
    assert_eq!(second.file_name().unwrap(), "deck_compressed_2.pptx");
    fs::write(&second, b"second").unwrap();

    let third = unique_output_path(dir.path(), "deck");
    assert_eq!(third.file_name().unwrap(), "deck_compressed_3.pptx");

    assert_eq!(fs::read(&first).unwrap(), b"first");
    assert_eq!(fs::read(&second).unwrap(), b"second");
}

#[test]
fn first_output_returns_first_success_only() {
    let dir = tempdir().unwrap();
    let p = sample_paths(dir.path(), &["a.pptx", "b.pptx", "c.pptx"]);
    let mut queue = BatchQueue::from_paths(&p);
    assert_eq!(queue.first_output(), None);

    let b_out = dir.path().join("b_out.pptx");
    let c_out = dir.path().join("c_out.pptx");
    queue.mark_failed(0, "坏包".into());
    queue.mark_done(1, 100, 50, b_out.clone());
    queue.mark_done(2, 100, 50, c_out);
    assert_eq!(queue.first_output(), Some(b_out));
}

#[test]
fn summary_serializes_for_structured_output() {
    let summary = BatchSummary {
        total: 3,
        succeeded: 2,
        failed: 1,
        total_original: 4_000,
        total_compressed: 2_500,
    };
    let raw = serde_json::to_string(&summary).unwrap();
    let restored: BatchSummary = serde_json::from_str(&raw).unwrap();
    assert_eq!(restored, summary);
    assert!(raw.contains("total_original"));
    assert!(raw.contains("succeeded"));
}

#[test]
fn job_status_labels_are_chinese() {
    assert_eq!(JobStatus::Queued.label_zh(), "等待中");
    assert_eq!(JobStatus::Running.label_zh(), "处理中");
    assert_eq!(JobStatus::Done.label_zh(), "已完成");
    assert_eq!(JobStatus::Failed.label_zh(), "失败");
    assert!(JobStatus::Done.is_terminal());
    assert!(JobStatus::Failed.is_terminal());
    assert!(!JobStatus::Queued.is_terminal());
    assert!(!JobStatus::Running.is_terminal());
}
