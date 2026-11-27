//! 跨平台 UI 桥接协议（桌面 / 鸿蒙电脑共用）。
//!
//! 桌面版用 `mpsc::Sender<CompressMessage>` 把后台线程的事件推给 egui；
//! 鸿蒙版没有 Rust GUI，改为把**同一批事件**序列化成 JSON 事件流，
//! 由 ArkTS 侧轮询消费。事件语义、字段顺序、设置结构、输出命名规则
//! 与桌面版完全一致，ArkTS 外壳只是换了一个渲染层。
//!
//! 线程模型：`start_job()` 立即返回 job_id，真正的工作在后台 std 线程；
//! 事件写入无界队列，`poll_events()` 由 ArkTS 定时器非阻塞取走。
//! 这样 N-API 侧只需同步、极快的调用，不需要 napi_threadsafe_function，
//! 也不会在 UI 线程上做重活。

use crate::core::batch::{run_batch, BatchMessage};
use crate::core::estimate::MediaComposition;
use crate::core::extractor::extract_pptx_inspected;
use crate::core::pipeline::run_pipeline;
use crate::core::platform;
use crate::core::queue::{unique_output_path, BatchSummary};
use crate::core::{CompressMessage, CompressionSettings, MediaInfo, MediaStatus, MediaType};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use tempfile::TempDir;

/// 协议版本。ArkTS 启动时校验，不匹配直接提示升级，避免静默错解字段。
pub const PROTOCOL_VERSION: u32 = 1;

// ── 能力查询 ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub platform: String,
    pub version: String,
    pub protocol_version: u32,
    pub video_transcode: bool,
    pub batch: bool,
    pub max_image_threads: usize,
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        platform: platform::platform_label().to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol_version: PROTOCOL_VERSION,
        video_transcode: platform::video_transcode_available(),
        batch: true,
        max_image_threads: platform::max_image_threads(),
    }
}

// ── 请求 ────────────────────────────────────────────────────

/// `mode` 标签化枚举。ArkTS 侧 `Protocol.ets` 有一一对应的 interface。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum BridgeRequest {
    /// 解析单个 PPTX：只读安全检测 + 解压 + 媒体清单（对应桌面 LoadMessage::Loaded）。
    Inspect { path: String },
    /// 压缩已解析的单个 PPTX（对应桌面「开始压缩」）。
    Compress {
        session_id: u64,
        #[serde(default)]
        output_dir: Option<String>,
        #[serde(default)]
        settings: CompressionSettings,
        /// 与 `Inspected.media` 等长的启用开关；缺省沿用解析时的默认值。
        #[serde(default)]
        enabled: Option<Vec<bool>>,
    },
    /// 批量队列（对应桌面多文件模式，复用 `core::batch::run_batch`）。
    Batch {
        inputs: Vec<String>,
        #[serde(default)]
        output_dir: Option<String>,
        #[serde(default)]
        settings: CompressionSettings,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartReply {
    pub job_id: u64,
    pub capabilities: Capabilities,
}

// ── 媒体项与事件 ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeMedia {
    pub index: usize,
    pub path: String,
    pub name: String,
    pub kind: MediaType,
    pub original_size: u64,
    pub enabled: bool,
    pub status: MediaStatus,
}

fn to_bridge_media(media: &[MediaInfo]) -> Vec<BridgeMedia> {
    media
        .iter()
        .enumerate()
        .map(|(index, m)| BridgeMedia {
            index,
            path: m.relative_path.clone(),
            name: PathBuf::from(&m.relative_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            kind: m.media_type,
            original_size: m.original_size,
            enabled: m.enabled,
            status: m.status,
        })
        .collect()
}

/// 事件流的唯一类型。`type` 为 snake_case 标签，ArkTS 用 switch 分发。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    /// 解析完成，携带会话 id（解压树在 Rust 侧保活到压缩结束）。
    Inspected {
        session_id: u64,
        source_path: String,
        source_size: u64,
        warnings: Vec<String>,
        media: Vec<BridgeMedia>,
        composition: MediaComposition,
    },
    StatusUpdate {
        index: usize,
        status: MediaStatus,
        compressed_size: u64,
    },
    JobStarted {
        index: usize,
        name: String,
        media_count: usize,
        warnings: Vec<String>,
    },
    JobProgress {
        index: usize,
        current: usize,
        total: usize,
        file: String,
    },
    JobLog {
        index: usize,
        message: String,
    },
    JobFinished {
        index: usize,
        original: u64,
        compressed: u64,
        output_path: String,
    },
    JobFailed {
        index: usize,
        message: String,
    },
    Finished {
        summary: BatchSummary,
    },
    Cancelled,
    Fatal {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollReply {
    pub events: Vec<BridgeEvent>,
    pub done: bool,
}

// ── 全局注册表 ──────────────────────────────────────────────

/// 解析会话：持有 TempDir，保证解压树在两次调用之间不被清理。
struct Session {
    _temp: TempDir,
    extract_dir: PathBuf,
    media: Vec<MediaInfo>,
    source_path: PathBuf,
    source_size: u64,
}

struct Job {
    cancel: Arc<AtomicBool>,
    events: Arc<Mutex<VecDeque<BridgeEvent>>>,
    done: Arc<AtomicBool>,
}

#[derive(Default)]
struct Registry {
    next_id: u64,
    sessions: HashMap<u64, Session>,
    jobs: HashMap<u64, Job>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn next_id() -> u64 {
    let mut reg = lock_registry();
    reg.next_id += 1;
    reg.next_id
}

fn lock_registry() -> std::sync::MutexGuard<'static, Registry> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

fn emit(job: &Job, event: BridgeEvent) {
    if let Ok(mut queue) = job.events.lock() {
        queue.push_back(event);
    }
}

fn finish(job: &Job) {
    job.done.store(true, Ordering::Release);
}

fn job_snapshot(id: u64) -> Option<Job> {
    let reg = lock_registry();
    reg.jobs.get(&id).map(|j| Job {
        cancel: Arc::clone(&j.cancel),
        events: Arc::clone(&j.events),
        done: Arc::clone(&j.done),
    })
}

// ── 对外 API ────────────────────────────────────────────────

/// 启动一个后台任务，立即返回 job_id。请求 JSON 非法时返回 Err。
pub fn start_job(request_json: &str) -> Result<StartReply, String> {
    let request: BridgeRequest =
        serde_json::from_str(request_json).map_err(|e| format!("请求格式错误：{e}"))?;

    let job_id = next_id();
    let job = Job {
        cancel: Arc::new(AtomicBool::new(false)),
        events: Arc::new(Mutex::new(VecDeque::new())),
        done: Arc::new(AtomicBool::new(false)),
    };
    lock_registry().jobs.insert(job_id, job.clone_fields());
    let worker = job;
    match request {
        BridgeRequest::Inspect { path } => {
            let session_id = next_id();
            std::thread::spawn(move || {
                run_inspect(&worker, session_id, &path);
                finish(&worker);
            });
        }
        BridgeRequest::Compress {
            session_id,
            output_dir,
            settings,
            enabled,
        } => {
            let session = lock_registry().sessions.remove(&session_id);
            std::thread::spawn(move || {
                run_compress(&worker, session, output_dir, settings, enabled);
                finish(&worker);
            });
        }
        BridgeRequest::Batch {
            inputs,
            output_dir,
            settings,
        } => {
            std::thread::spawn(move || {
                run_batch_job(&worker, inputs, output_dir, settings);
                finish(&worker);
            });
        }
    }

    Ok(StartReply {
        job_id,
        capabilities: capabilities(),
    })
}

impl Job {
    fn clone_fields(&self) -> Job {
        Job {
            cancel: Arc::clone(&self.cancel),
            events: Arc::clone(&self.events),
            done: Arc::clone(&self.done),
        }
    }
}

/// 非阻塞取走当前积压的事件。`done` 表示后台线程已结束。
pub fn poll_events(job_id: u64) -> PollReply {
    let Some(job) = job_snapshot(job_id) else {
        return PollReply {
            events: vec![BridgeEvent::Fatal {
                message: "任务不存在或已被回收".into(),
            }],
            done: true,
        };
    };
    let mut events = Vec::new();
    if let Ok(mut queue) = job.events.lock() {
        while let Some(event) = queue.pop_front() {
            events.push(event);
        }
    }
    PollReply {
        events,
        done: job.done.load(Ordering::Acquire),
    }
}

/// 请求取消。与桌面版同一个 AtomicBool 语义，不写输出文件。
pub fn cancel_job(job_id: u64) -> bool {
    let Some(job) = job_snapshot(job_id) else {
        return false;
    };
    job.cancel.store(true, Ordering::Relaxed);
    true
}

/// 任务结束后回收 job 记录，避免长时间运行后内存无限增长。
pub fn drop_job(job_id: u64) -> bool {
    lock_registry().jobs.remove(&job_id).is_some()
}

/// 放弃一次解析会话（用户改选文件时），立即清理解压树。
pub fn drop_session(session_id: u64) -> bool {
    lock_registry().sessions.remove(&session_id).is_some()
}

// ── 三个任务实现 ────────────────────────────────────────────

fn run_inspect(job: &Job, session_id: u64, raw_path: &str) {
    let source_path = PathBuf::from(raw_path);
    let source_size = fs::metadata(&source_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let temp = match platform::tempdir() {
        Ok(t) => t,
        Err(e) => {
            emit(job, BridgeEvent::Fatal { message: format!("创建临时目录失败：{e}") });
            return;
        }
    };
    let extract_dir = temp.path().to_path_buf();

    let (mut media, inspection) = match extract_pptx_inspected(&source_path, &extract_dir) {
        Ok(v) => v,
        Err(e) => {
            emit(job, BridgeEvent::Fatal { message: format!("读取文稿失败：{e}") });
            return;
        }
    };

    let mut warnings = inspection.warning_messages();
    if !platform::video_transcode_available() {
        let mut video_count = 0usize;
        for item in &mut media {
            if item.media_type == MediaType::Video {
                item.enabled = false;
                video_count += 1;
            }
        }
        if video_count > 0 {
            warnings.push(format!(
                "当前平台不提供 FFmpeg，{video_count} 个视频将保持原样；图片、GIF 与 XML 清理正常执行"
            ));
        }
    }

    let composition = MediaComposition::from_media(&media, source_size);
    let snapshot = to_bridge_media(&media);

    let session = Session {
        _temp: temp,
        extract_dir,
        media,
        source_path: source_path.clone(),
        source_size,
    };
    lock_registry().sessions.insert(session_id, session);

    emit(
        job,
        BridgeEvent::Inspected {
            session_id,
            source_path: source_path.to_string_lossy().into_owned(),
            source_size,
            warnings,
            media: snapshot,
            composition,
        },
    );
}

fn run_compress(
    job: &Job,
    session: Option<Session>,
    output_dir: Option<String>,
    settings: CompressionSettings,
    enabled: Option<Vec<bool>>,
) {
    let Some(mut session) = session else {
        emit(job, BridgeEvent::Fatal { message: "解析会话已失效，请重新选择文件".into() });
        return;
    };

    if let Some(flags) = enabled {
        for (item, flag) in session.media.iter_mut().zip(flags.iter()) {
            // 平台上不可用的能力不允许被 UI 强行打开。
            item.enabled = *flag
                && (item.media_type != MediaType::Video
                    || platform::video_transcode_available());
        }
    }

    let dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            session
                .source_path
                .parent()
                .unwrap_or(&session.source_path)
                .to_path_buf()
        });
    if let Err(e) = fs::create_dir_all(&dir) {
        emit(job, BridgeEvent::Fatal { message: format!("创建输出目录失败：{e}") });
        return;
    }
    let stem = session
        .source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let output_path = unique_output_path(&dir, &stem);
    let output_display = output_path.to_string_lossy().into_owned();

    let (tx, rx) = mpsc::channel::<CompressMessage>();
    let cancel = Arc::clone(&job.cancel);
    let pipeline_cancel = Arc::clone(&cancel);
    let pipeline_output = output_path.clone();
    let handle = std::thread::spawn(move || {
        let Session {
            _temp,
            extract_dir,
            mut media,
            source_size,
            ..
        } = session;
        run_pipeline(
            &extract_dir,
            &pipeline_output,
            &mut media,
            &settings,
            source_size,
            tx,
            pipeline_cancel,
        )
    });

    let mut finished = false;
    let mut cancelled = false;
    let mut fatal: Option<String> = None;
    let mut original_bytes = 0u64;
    let mut compressed_bytes = 0u64;
    for message in rx {
        match message {
            CompressMessage::StatusUpdate(i, status, size) => emit(
                job,
                BridgeEvent::StatusUpdate { index: i, status, compressed_size: size },
            ),
            CompressMessage::Progress(current, total, file) => emit(
                job,
                BridgeEvent::JobProgress { index: 0, current, total, file },
            ),
            CompressMessage::Error(line) => {
                emit(job, BridgeEvent::JobLog { index: 0, message: line })
            }
            CompressMessage::Finished(original, compressed) => {
                finished = true;
                original_bytes = original;
                compressed_bytes = compressed;
                emit(
                    job,
                    BridgeEvent::JobFinished {
                        index: 0,
                        original,
                        compressed,
                        output_path: output_display.clone(),
                    },
                );
            }
            CompressMessage::Cancelled => cancelled = true,
            CompressMessage::FatalError(e) => fatal = Some(e),
        }
    }

    let result = handle.join();
    if cancel.load(Ordering::Relaxed) || cancelled {
        let _ = fs::remove_file(&output_path);
        emit(job, BridgeEvent::Cancelled);
    } else if let Some(message) = fatal.or_else(|| result.ok().and_then(|r| r.err())) {
        let _ = fs::remove_file(&output_path);
        emit(job, BridgeEvent::JobFailed { index: 0, message });
    } else if !finished || !output_path.exists() {
        let _ = fs::remove_file(&output_path);
        emit(job, BridgeEvent::JobFailed {
            index: 0,
            message: "压缩流程提前结束，未生成输出文件".into(),
        });
    } else {
        emit(job, BridgeEvent::Finished {
            summary: BatchSummary {
                total: 1,
                succeeded: 1,
                failed: 0,
                total_original: original_bytes,
                total_compressed: compressed_bytes,
            },
        });
    }
}

fn run_batch_job(
    job: &Job,
    inputs: Vec<String>,
    output_dir: Option<String>,
    settings: CompressionSettings,
) {
    let paths: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
    let out = output_dir.map(PathBuf::from);
    let (tx, rx) = mpsc::channel::<BatchMessage>();
    let cancel = Arc::clone(&job.cancel);
    let batch_cancel = Arc::clone(&cancel);

    let handle = std::thread::spawn(move || {
        run_batch(&paths, out.as_deref(), &settings, tx, batch_cancel);
    });

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut total_original = 0u64;
    let mut total_compressed = 0u64;

    for message in rx {
        match message {
            BatchMessage::JobStarted(index, name, media_count, warnings) => emit(
                job,
                BridgeEvent::JobStarted { index, name, media_count, warnings },
            ),
            BatchMessage::JobProgress(index, current, total, file) => emit(
                job,
                BridgeEvent::JobProgress { index, current, total, file },
            ),
            BatchMessage::JobLog(index, message) => {
                emit(job, BridgeEvent::JobLog { index, message })
            }
            BatchMessage::JobFinished(index, original, compressed, output_path) => {
                succeeded += 1;
                total_original = total_original.saturating_add(original);
                total_compressed = total_compressed.saturating_add(compressed);
                emit(job, BridgeEvent::JobFinished {
                    index,
                    original,
                    compressed,
                    output_path: output_path.to_string_lossy().into_owned(),
                });
            }
            BatchMessage::JobFailed(index, message) => {
                failed += 1;
                emit(job, BridgeEvent::JobFailed { index, message });
            }
            BatchMessage::Finished => {
                emit(job, BridgeEvent::Finished {
                    summary: BatchSummary {
                        total: succeeded + failed,
                        succeeded,
                        failed,
                        total_original,
                        total_compressed,
                    },
                });
            }
            BatchMessage::Cancelled => emit(job, BridgeEvent::Cancelled),
        }
    }
    let _ = handle.join();
}
