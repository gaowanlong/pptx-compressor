//! 批量队列的端到端测试：在真实临时 PPTX 上验证逐个处理、失败隔离、
//! 取消、输出目录与消息契约。不依赖 GUI、FFmpeg 或网络。

use image::ImageEncoder as _;
use pptx_compressor::core::batch::{run_batch, BatchMessage};
use pptx_compressor::core::queue::BatchQueue;
use pptx_compressor::core::CompressionSettings;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const CT: &[u8] = b"<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/></Types>";
const PRESENTATION: &[u8] = b"<?xml version=\"1.0\"?><p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"/>";

/// 生成一张真实 JPEG，让流水线里的重编码走完整解码 / 编码路径。
fn jpeg_bytes(quality: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(96, 64);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([
            ((x * 7 + y * 11) % 256) as u8,
            ((x * 13 + y * 3) % 256) as u8,
            ((x * 5 + y * 17) % 256) as u8,
        ]);
    }
    let mut buf = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .write_image(img.as_raw(), 96, 64, image::ExtendedColorType::Rgb8)
        .unwrap();
    buf.into_inner()
}

/// 写一个能通过输入安全检测并能被重新打包的最小 PPTX。
fn write_pptx(path: &Path, jpeg: &[u8]) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let entries: [(&str, &[u8]); 3] = [
        ("[Content_Types].xml", CT),
        ("ppt/presentation.xml", PRESENTATION),
        ("ppt/media/image1.jpg", jpeg),
    ];
    for (name, data) in entries {
        zip.start_file(name, stored).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

/// 写 OLE 复合文档魔数：输入安全检测会把它当成旧版 .ppt / 加密包阻断。
fn write_ole_container(path: &Path) {
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.extend_from_slice(&[0u8; 64]);
    fs::write(path, bytes).unwrap();
}

fn run(inputs: &[PathBuf], output_dir: Option<&Path>, cancel: Arc<AtomicBool>) -> Vec<BatchMessage> {
    let (tx, rx) = mpsc::channel();
    run_batch(
        inputs,
        output_dir,
        &CompressionSettings::default(),
        tx,
        cancel,
    );
    rx.try_iter().collect()
}

fn is_valid_pptx(path: &Path) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return false;
    };
    // 先落地成局部变量，让 ZipFile 临时值在 archive 被 drop 之前释放。
    let found = archive.by_name("[Content_Types].xml").is_ok();
    found
}

fn finished_indices(messages: &[BatchMessage]) -> Vec<usize> {
    messages
        .iter()
        .filter_map(|m| match m {
            BatchMessage::JobFinished(idx, ..) => Some(*idx),
            _ => None,
        })
        .collect()
}

fn finished_paths(messages: &[BatchMessage]) -> Vec<PathBuf> {
    messages
        .iter()
        .filter_map(|m| match m {
            BatchMessage::JobFinished(_, _, _, path) => Some(path.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn batch_processes_every_valid_file_and_reports_each_result() {
    let dir = tempdir().unwrap();
    let jpeg = jpeg_bytes(95);
    let inputs: Vec<PathBuf> = (0..3)
        .map(|i| {
            let path = dir.path().join(format!("deck{i}.pptx"));
            write_pptx(&path, &jpeg);
            path
        })
        .collect();

    let messages = run(&inputs, None, Arc::new(AtomicBool::new(false)));

    assert_eq!(finished_indices(&messages), vec![0, 1, 2]);
    assert_eq!(
        messages
            .iter()
            .filter(|m| matches!(m, BatchMessage::JobFailed(..)))
            .count(),
        0
    );
    assert!(messages
        .iter()
        .any(|m| matches!(m, BatchMessage::Finished)));
    assert!(messages
        .iter()
        .any(|m| matches!(m, BatchMessage::JobStarted(..))));
    assert!(messages
        .iter()
        .any(|m| matches!(m, BatchMessage::JobProgress(..))));

    for (i, input) in inputs.iter().enumerate() {
        let out = dir.path().join(format!("deck{i}_compressed.pptx"));
        assert!(out.exists(), "缺少输出文件 {out:?}");
        assert!(is_valid_pptx(&out), "输出不是有效 PPTX：{out:?}");
        assert!(fs::metadata(&out).unwrap().len() > 0);
        // 源文件绝对不能被改动。
        assert!(input.exists());
    }
}

#[test]
fn corrupted_file_is_isolated_and_queue_still_finishes() {
    let dir = tempdir().unwrap();
    let jpeg = jpeg_bytes(95);
    let good0 = dir.path().join("good0.pptx");
    let good2 = dir.path().join("good2.pptx");
    let broken = dir.path().join("broken.pptx");
    write_pptx(&good0, &jpeg);
    write_pptx(&good2, &jpeg);
    write_ole_container(&broken);

    let inputs = vec![good0, broken, good2];
    let messages = run(&inputs, None, Arc::new(AtomicBool::new(false)));

    assert_eq!(finished_indices(&messages), vec![0, 2]);
    let failures: Vec<usize> = messages
        .iter()
        .filter_map(|m| match m {
            BatchMessage::JobFailed(idx, _) => Some(*idx),
            _ => None,
        })
        .collect();
    assert_eq!(failures, vec![1]);
    assert!(
        messages.iter().any(|m| matches!(m, BatchMessage::Finished)),
        "单个文件失败不能中断队列收尾"
    );
    assert!(messages.iter().any(|m| matches!(
        m,
        BatchMessage::JobFailed(_, reason)
            if reason.contains("旧版 .ppt") || reason.contains("加密")
    )));

    assert!(dir.path().join("good0_compressed.pptx").exists());
    assert!(dir.path().join("good2_compressed.pptx").exists());
    assert!(!dir.path().join("broken_compressed.pptx").exists());
}

#[test]
fn duplicate_stems_get_distinct_outputs_in_shared_dir() {
    let dir = tempdir().unwrap();
    let out_dir = dir.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let jpeg = jpeg_bytes(95);
    let a_dir = dir.path().join("a");
    let b_dir = dir.path().join("b");
    fs::create_dir_all(&a_dir).unwrap();
    fs::create_dir_all(&b_dir).unwrap();
    let a = a_dir.join("deck.pptx");
    let b = b_dir.join("deck.pptx");
    write_pptx(&a, &jpeg);
    write_pptx(&b, &jpeg);

    let messages = run(&[a, b], Some(&out_dir), Arc::new(AtomicBool::new(false)));
    let outputs = finished_paths(&messages);

    assert_eq!(outputs.len(), 2);
    assert_ne!(outputs[0], outputs[1], "同名源文件绝不能互相覆盖");
    for path in &outputs {
        assert_eq!(path.parent(), Some(out_dir.as_path()));
        assert!(is_valid_pptx(path));
    }
}

#[test]
fn custom_output_dir_is_used_for_every_file() {
    let dir = tempdir().unwrap();
    let out_dir = dir.path().join("统一输出");
    let jpeg = jpeg_bytes(95);
    let inputs: Vec<PathBuf> = (0..3)
        .map(|i| {
            let sub = dir.path().join(format!("src{i}"));
            fs::create_dir_all(&sub).unwrap();
            let path = sub.join(format!("file{i}.pptx"));
            write_pptx(&path, &jpeg);
            path
        })
        .collect();

    let messages = run(&inputs, Some(&out_dir), Arc::new(AtomicBool::new(false)));
    let outputs = finished_paths(&messages);

    assert_eq!(outputs.len(), 3);
    for path in &outputs {
        assert_eq!(path.parent(), Some(out_dir.as_path()));
        assert!(is_valid_pptx(path));
    }
}

#[test]
fn empty_queue_finishes_without_touching_disk() {
    let messages = run(&[], None, Arc::new(AtomicBool::new(false)));
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], BatchMessage::Finished));
}

#[test]
fn cancel_stops_the_queue_before_any_work() {
    let dir = tempdir().unwrap();
    let jpeg = jpeg_bytes(95);
    let inputs: Vec<PathBuf> = (0..2)
        .map(|i| {
            let path = dir.path().join(format!("cancel{i}.pptx"));
            write_pptx(&path, &jpeg);
            path
        })
        .collect();

    let cancel = Arc::new(AtomicBool::new(true));
    let messages = run(&inputs, None, cancel);

    assert!(messages
        .iter()
        .any(|m| matches!(m, BatchMessage::Cancelled)));
    assert!(!messages
        .iter()
        .any(|m| matches!(m, BatchMessage::Finished)));
    assert!(!messages
        .iter()
        .any(|m| matches!(m, BatchMessage::JobFinished(..))));
    for i in 0..2 {
        assert!(!dir.path().join(format!("cancel{i}_compressed.pptx")).exists());
    }
}

#[test]
fn messages_rebuild_a_consistent_queue_summary() {
    let dir = tempdir().unwrap();
    let jpeg = jpeg_bytes(95);
    let inputs: Vec<PathBuf> = (0..3)
        .map(|i| {
            let path = dir.path().join(format!("contract{i}.pptx"));
            write_pptx(&path, &jpeg);
            path
        })
        .collect();
    let messages = run(&inputs, None, Arc::new(AtomicBool::new(false)));

    // 模拟 GUI 把后台消息写回队列模型，验证两侧契约一致。
    let mut queue = BatchQueue::from_paths(&inputs);
    for message in &messages {
        match message {
            BatchMessage::JobStarted(idx, _, count, warnings) => {
                queue.mark_running(*idx);
                queue.set_media_count(*idx, *count);
                queue.set_warnings(*idx, warnings.clone());
            }
            BatchMessage::JobFinished(idx, original, compressed, path) => {
                queue.mark_done(*idx, *original, *compressed, path.clone());
            }
            BatchMessage::JobFailed(idx, reason) => queue.mark_failed(*idx, reason.clone()),
            _ => {}
        }
    }

    assert!(queue.is_finished());
    let summary = queue.summary();
    assert_eq!(summary.total, 3);
    assert_eq!(summary.succeeded, 3);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.total_compressed, finished_paths(&messages).iter().map(|p| fs::metadata(p).unwrap().len()).sum::<u64>());
    assert_eq!(queue.first_output(), finished_paths(&messages).first().cloned());
}
