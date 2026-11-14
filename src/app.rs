//! 应用状态机与整体布局。
//!
//! 界面按「左侧主导区（源文件 + 媒体清单）/ 右侧从属区（方案 + 结果）/ 底部固定操作条」
//! 组织：主操作按钮在所有状态下都固定在右下角，状态反馈集中在标题栏与右栏。
//! 所有耗时工作（解包、压缩）都在后台线程完成，GUI 只通过消息通道接收结果。

use crate::core::batch::{self, BatchMessage};
use crate::core::queue::{self, BatchQueue};
use crate::core::{CompressMessage, CompressionSettings, MediaInfo};
use crate::ui::{components, file_panel, media_list, progress, settings};
use crate::utils::ffmpeg::{self, FfmpegSource, LicenseClass};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

// ── 设计令牌：纯白底、单一强调色 ────────────────────────────
pub const WHITE: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(247, 248, 250);
pub const SURFACE_2: egui::Color32 = egui::Color32::from_rgb(251, 252, 253);
pub const LINE: egui::Color32 = egui::Color32::from_rgb(231, 233, 238);
pub const LINE_SOFT: egui::Color32 = egui::Color32::from_rgb(240, 241, 244);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(27, 29, 33);
pub const TEXT_2: egui::Color32 = egui::Color32::from_rgb(91, 96, 105);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(138, 143, 152);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(47, 107, 255);
pub const ACCENT_DARK: egui::Color32 = egui::Color32::from_rgb(34, 85, 222);
pub const ACCENT_SOFT: egui::Color32 = egui::Color32::from_rgb(238, 243, 255);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(18, 161, 80);
pub const GREEN_SOFT: egui::Color32 = egui::Color32::from_rgb(234, 247, 239);
pub const AMBER: egui::Color32 = egui::Color32::from_rgb(200, 127, 10);
pub const AMBER_SOFT: egui::Color32 = egui::Color32::from_rgb(255, 246, 229);
pub const RED: egui::Color32 = egui::Color32::from_rgb(214, 69, 69);
pub const RED_SOFT: egui::Color32 = egui::Color32::from_rgb(253, 238, 238);

const STORAGE_KEY: &str = "pptx-compressor.state.v1";

/// 退出时写入磁盘的用户偏好。新增字段会走 `serde(default)`，旧文件仍可读。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    pub settings: CompressionSettings,
    pub output_dir: Option<PathBuf>,
    pub show_advanced: bool,
    /// 用户手动指定的 FFmpeg 可执行文件；留空则自动查找。
    pub ffmpeg_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Loading,
    Ready,
    Compressing,
    Done,
    Error,
}

impl AppState {
    /// 标题栏状态胶囊文案与配色。
    pub fn badge(self) -> (&'static str, egui::Color32, egui::Color32) {
        match self {
            AppState::Idle => ("等待文件", MUTED, SURFACE),
            AppState::Loading => ("正在解析", ACCENT, ACCENT_SOFT),
            AppState::Ready => ("已就绪", GREEN, GREEN_SOFT),
            AppState::Compressing => ("压缩中", ACCENT, ACCENT_SOFT),
            AppState::Done => ("已完成", GREEN, GREEN_SOFT),
            AppState::Error => ("处理失败", RED, RED_SOFT),
        }
    }

    pub fn badge_text(self, progress_pct: u8) -> String {
        let (label, _, _) = self.badge();
        if self == AppState::Compressing {
            format!("{} {}%", label, progress_pct)
        } else {
            label.to_owned()
        }
    }
}

/// 后台解包线程回传的消息。`generation` 用于丢弃过期结果（用户中途换了文件）。
pub enum LoadMessage {
    /// (generation, media, extract_dir, temp_dir, input_warnings)
    Loaded(u64, Vec<MediaInfo>, PathBuf, tempfile::TempDir, Vec<String>),
    Failed(u64, String),
}

/// 主应用状态。
pub struct PptxCompressorApp {
    pub state: AppState,
    pub input_files: Vec<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub media_files: Vec<MediaInfo>,
    pub settings: CompressionSettings,
    pub _temp_dir: Option<tempfile::TempDir>,
    pub extract_dir: Option<PathBuf>,
    pub rx: Option<mpsc::Receiver<CompressMessage>>,
    pub load_rx: Option<mpsc::Receiver<LoadMessage>>,
    pub load_generation: u64,
    pub cancel_flag: Arc<AtomicBool>,
    pub progress_current: usize,
    pub progress_total: usize,
    pub progress_file: String,
    pub result_original: u64,
    pub result_compressed: u64,
    /// 活动日志：非致命错误、跳过原因、清理告警都在这里可见。
    pub activity_log: Vec<String>,
    /// 输入安全检测的告警（签名、宏、嵌入对象、修改保护等），显示在文件卡片下。
    pub input_warnings: Vec<String>,
    pub ffmpeg_available: bool,
    /// 实际使用的 FFmpeg 命令（自动查找或用户指定）。
    pub ffmpeg_path: Option<String>,
    pub ffmpeg_source: Option<FfmpegSource>,
    pub ffmpeg_license: Option<LicenseClass>,
    /// 持久化的用户手动指定路径。
    pub ffmpeg_override: Option<String>,
    pub show_advanced: bool,
    pub show_only_compressible: bool,
    pub output_path: Option<PathBuf>,
    pub font_ok: bool,
    /// 多文件批量队列；单文件模式为 `None`，沿用原有解析 / 压缩流程。
    pub queue: Option<BatchQueue>,
    pub batch_rx: Option<mpsc::Receiver<BatchMessage>>,
    /// 当前文件内部的媒体进度 `(current, total)`，用于平滑文件级总进度。
    pub batch_current_media: (usize, usize),
}

impl Default for PptxCompressorApp {
    fn default() -> Self {
        let located = ffmpeg::locate_current();
        Self {
            state: AppState::Idle,
            input_files: Vec::new(),
            output_dir: None,
            media_files: Vec::new(),
            settings: CompressionSettings::default(),
            _temp_dir: None,
            extract_dir: None,
            rx: None,
            load_rx: None,
            load_generation: 0,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            progress_current: 0,
            progress_total: 0,
            progress_file: String::new(),
            result_original: 0,
            result_compressed: 0,
            activity_log: Vec::new(),
            input_warnings: Vec::new(),
            ffmpeg_available: located.is_some(),
            ffmpeg_path: located.as_ref().map(|f| f.command.clone()),
            ffmpeg_source: located.as_ref().map(|f| f.source),
            ffmpeg_license: located.as_ref().map(|f| f.license),
            ffmpeg_override: None,
            show_advanced: false,
            show_only_compressible: false,
            output_path: None,
            font_ok: true,
            queue: None,
            batch_rx: None,
            batch_current_media: (0, 0),
        }
    }
}

impl PptxCompressorApp {
    /// 由 eframe 创建，恢复上次保存的偏好。
    pub fn new(cc: &eframe::CreationContext<'_>, font_ok: bool) -> Self {
        let mut app = Self {
            font_ok,
            ..Self::default()
        };
        if let Some(storage) = cc.storage {
            if let Some(raw) = storage.get_string(STORAGE_KEY) {
                match serde_json::from_str::<PersistedState>(&raw) {
                    Ok(saved) => {
                        app.settings = saved.settings;
                        app.output_dir = saved.output_dir;
                        app.show_advanced = saved.show_advanced;
                        app.ffmpeg_override = saved.ffmpeg_override;
                    }
                    Err(e) => eprintln!("忽略损坏的偏好设置：{}", e),
                }
            }
        }
        app.refresh_ffmpeg();
        app
    }

    /// 重新定位 FFmpeg，并把结果同步给后台压缩线程与界面。
    pub fn refresh_ffmpeg(&mut self) {
        ffmpeg::set_preferred(self.ffmpeg_override.clone());
        let located = ffmpeg::locate_current();
        self.ffmpeg_available = located.is_some();
        self.ffmpeg_path = located.as_ref().map(|f| f.command.clone());
        self.ffmpeg_source = located.as_ref().map(|f| f.source);
        self.ffmpeg_license = located.as_ref().map(|f| f.license);
    }

    /// 让用户从本机选择一个 FFmpeg 可执行文件，校验通过后持久化。
    pub fn pick_ffmpeg(&mut self) {
        #[cfg(target_os = "windows")]
        let dialog = rfd::FileDialog::new()
            .set_title("选择 FFmpeg 可执行文件")
            .add_filter("FFmpeg", &["exe"]);
        #[cfg(not(target_os = "windows"))]
        let dialog = rfd::FileDialog::new().set_title("选择 FFmpeg 可执行文件");
        let Some(path) = dialog.pick_file() else {
            return;
        };
        match ffmpeg::validate_path(&path) {
            Ok(info) => {
                self.ffmpeg_override = Some(info.command);
                self.refresh_ffmpeg();
                self.log(format!(
                    "已指定 FFmpeg：{}",
                    self.ffmpeg_path.as_deref().unwrap_or("（未知）")
                ));
            }
            Err(e) => self.log(format!("选择 FFmpeg 失败：{}", e)),
        }
    }

    /// 清除手动指定，回到自动查找。
    pub fn clear_ffmpeg_override(&mut self) {
        self.ffmpeg_override = None;
        self.refresh_ffmpeg();
        self.log("已恢复自动查找 FFmpeg");
    }

    /// 是否处于多文件批量模式。单文件模式 `queue` 为 `None`，行为完全不变。
    pub fn is_batch(&self) -> bool {
        self.queue.is_some()
    }

    pub fn batch_len(&self) -> usize {
        self.queue.as_ref().map_or(0, queue::BatchQueue::len)
    }

    pub fn progress_pct(&self) -> u8 {
        if let Some(q) = &self.queue {
            let (current, total) = self.batch_current_media;
            return queue::combined_progress_pct(q.finished_count(), q.len(), current, total);
        }
        if self.progress_total == 0 {
            0
        } else {
            ((self.progress_current.min(self.progress_total) as f32
                / self.progress_total as f32)
                * 100.0) as u8
        }
    }

    /// 清空当前文件与结果，但保留用户偏好（设置、输出目录、折叠状态）。
    pub fn reset_for_new_file(&mut self) {
        self.state = AppState::Idle;
        self.input_files.clear();
        self.media_files.clear();
        self._temp_dir = None;
        self.extract_dir = None;
        self.rx = None;
        self.load_rx = None;
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.progress_current = 0;
        self.progress_total = 0;
        self.progress_file.clear();
        self.result_original = 0;
        self.result_compressed = 0;
        self.activity_log.clear();
        self.input_warnings.clear();
        self.output_path = None;
        self.queue = None;
        self.batch_rx = None;
        self.batch_current_media = (0, 0);
    }

    pub fn log(&mut self, message: impl Into<String>) {
        if self.activity_log.len() < 50 {
            self.activity_log.push(message.into());
        }
    }

    fn process_messages(&mut self) {
        // 1) 解包结果
        let mut loaded = None;
        let mut load_failed = None;
        if let Some(rx) = &self.load_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    LoadMessage::Loaded(gen, media, dir, temp, warnings)
                        if gen == self.load_generation =>
                    {
                        loaded = Some((media, dir, temp, warnings));
                        load_failed = None;
                    }
                    LoadMessage::Failed(gen, err) if gen == self.load_generation => {
                        load_failed = Some(err);
                        loaded = None;
                    }
                    // 过期结果：tempdir 在这里被丢弃并自动清理
                    _ => {}
                }
            }
        }
        if let Some((media, dir, temp, warnings)) = loaded {
            let count = media.len();
            self.media_files = media;
            self.extract_dir = Some(dir);
            self._temp_dir = Some(temp);
            self.state = AppState::Ready;
            self.load_rx = None;
            self.log(format!("已解析文稿，共发现 {} 个媒体文件", count));
            self.input_warnings = warnings.clone();
            for warning in &warnings {
                self.log(format!("输入检查 · {}", warning));
            }
        } else if let Some(err) = load_failed {
            self.state = AppState::Error;
            self.load_rx = None;
            self.log(format!("读取文稿失败：{}", err));
        }

        // 2) 压缩进度。消息在持有 receiver 借用期间只做字段级更新，
        //    状态切换与日志写入放到借用结束后，避免 &mut self 冲突。
        let mut cancelled = false;
        let mut finished = None;
        let mut fatal = false;
        let mut new_logs: Vec<String> = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    CompressMessage::StatusUpdate(idx, status, compressed_size) => {
                        if idx < self.media_files.len() {
                            self.media_files[idx].status = status;
                            if compressed_size > 0 {
                                self.media_files[idx].compressed_size = compressed_size;
                            }
                        }
                    }
                    CompressMessage::Progress(current, total, file) => {
                        self.progress_current = current;
                        self.progress_total = total;
                        self.progress_file = file;
                    }
                    CompressMessage::Finished(original, compressed) => {
                        finished = Some((original, compressed));
                    }
                    CompressMessage::Error(msg) => new_logs.push(msg),
                    CompressMessage::FatalError(msg) => {
                        new_logs.push(msg);
                        fatal = true;
                    }
                    CompressMessage::Cancelled => cancelled = true,
                }
            }
        }
        for msg in new_logs {
            self.log(msg);
        }
        if let Some((original, compressed)) = finished {
            self.result_original = original;
            self.result_compressed = compressed;
            self.state = AppState::Done;
            self.progress_file.clear();
        }
        if fatal {
            self.state = AppState::Error;
        }

        // 取消后丢弃已被修改的解包目录，重新从源文件解压，避免二次压缩
        // 基于已经损失画质的数据。
        if cancelled {
            self.rx = None;
            self.cancel_flag.store(false, Ordering::Relaxed);
            self.media_files.clear();
            self.extract_dir = None;
            self._temp_dir = None;
            self.progress_current = 0;
            self.progress_total = 0;
            self.progress_file.clear();
            self.state = AppState::Idle;
            let source = self.input_files.first().cloned();
            if let Some(path) = source {
                file_panel::load_pptx(self, Some(&path));
                self.log("已取消，未生成输出文件；已重新载入源文件");
            } else {
                self.input_files.clear();
            }
        }

        // 3) 批量队列消息。先收齐再处理，避免持有 receiver 借用期间改动 self。
        let batch_messages: Vec<BatchMessage> = match &self.batch_rx {
            Some(rx) => rx.try_iter().collect(),
            None => Vec::new(),
        };
        for msg in batch_messages {
            match msg {
                BatchMessage::JobStarted(idx, name, count, warnings) => {
                    if let Some(q) = self.queue.as_mut() {
                        q.mark_running(idx);
                        q.set_warnings(idx, warnings.clone());
                        q.set_media_count(idx, count);
                    }
                    self.progress_current = idx;
                    self.batch_current_media = (0, count);
                    self.progress_file = format!(
                        "第 {}/{} 个 · {}（{} 个媒体）",
                        idx + 1,
                        self.batch_len(),
                        name,
                        count
                    );
                    self.log(format!("开始处理 {name}：发现 {count} 个媒体"));
                    for warning in warnings {
                        self.log(format!("输入检查 · {name} · {warning}"));
                    }
                }
                BatchMessage::JobProgress(idx, current, total, file) => {
                    self.batch_current_media = (current, total);
                    self.progress_file = format!(
                        "第 {}/{} 个 · {}/{} · {}",
                        idx + 1,
                        self.batch_len(),
                        current,
                        total,
                        file
                    );
                }
                BatchMessage::JobLog(_idx, line) => self.log(line),
                BatchMessage::JobFinished(idx, original, compressed, path) => {
                    if let Some(q) = self.queue.as_mut() {
                        q.mark_done(idx, original, compressed, path.clone());
                    }
                    self.progress_current = idx + 1;
                    self.batch_current_media = (0, 0);
                    let short = path.file_name().unwrap_or_default().to_string_lossy();
                    self.log(format!(
                        "完成 · {short}：{} → {}",
                        components::fmt_size(original),
                        components::fmt_size(compressed)
                    ));
                }
                BatchMessage::JobFailed(idx, err) => {
                    if let Some(q) = self.queue.as_mut() {
                        q.mark_failed(idx, err.clone());
                    }
                    self.progress_current = idx + 1;
                    self.batch_current_media = (0, 0);
                    self.log(format!("失败 · 第 {} 个文件：{err}", idx + 1));
                }
                BatchMessage::Finished => {
                    self.batch_rx = None;
                    let (summary, first_output) = {
                        let q = self.queue.as_ref();
                        (
                            q.map(queue::BatchQueue::summary).unwrap_or_default(),
                            q.and_then(queue::BatchQueue::first_output),
                        )
                    };
                    self.result_original = summary.total_original;
                    self.result_compressed = summary.total_compressed;
                    self.progress_current = self.progress_total;
                    self.batch_current_media = (0, 0);
                    self.progress_file.clear();
                    self.output_path = first_output;
                    self.state = if summary.succeeded > 0 {
                        AppState::Done
                    } else {
                        AppState::Error
                    };
                    self.log(format!(
                        "批量完成：成功 {} 个、失败 {} 个",
                        summary.succeeded, summary.failed
                    ));
                }
                BatchMessage::Cancelled => {
                    self.batch_rx = None;
                    self.cancel_flag.store(false, Ordering::Relaxed);
                    self.state = AppState::Idle;
                    self.progress_current = 0;
                    self.progress_file.clear();
                    self.batch_current_media = (0, 0);
                    if let Some(q) = self.queue.as_mut() {
                        q.reset();
                    }
                    self.log("已取消批量处理；已生成的输出文件保留，源文件未改动");
                }
            }
        }
    }

    /// 启动批量压缩：建通道、重置队列状态、交给后台线程串行处理。
    fn start_batch(&mut self) {
        let inputs: Vec<PathBuf> = self
            .queue
            .as_ref()
            .map(|q| q.jobs.iter().map(|j| j.path.clone()).collect())
            .unwrap_or_default();
        if inputs.is_empty() {
            return;
        }

        self.state = AppState::Compressing;
        self.activity_log.clear();
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.progress_current = 0;
        self.progress_total = inputs.len();
        self.progress_file.clear();
        self.batch_current_media = (0, 0);
        self.result_original = 0;
        self.result_compressed = 0;
        self.output_path = None;
        if let Some(q) = self.queue.as_mut() {
            q.reset();
        }

        let (tx, rx) = mpsc::channel();
        self.batch_rx = Some(rx);
        let settings = self.settings.clone();
        let output_dir = self.output_dir.clone();
        let cancel = self.cancel_flag.clone();

        std::thread::spawn(move || {
            batch::run_batch(&inputs, output_dir.as_deref(), &settings, tx, cancel);
        });
    }

    pub fn start_compression(&mut self) {
        if self.is_batch() {
            self.start_batch();
            return;
        }
        if self.input_files.is_empty() || self.extract_dir.is_none() {
            return;
        }

        self.state = AppState::Compressing;
        self.activity_log.clear();
        self.cancel_flag.store(false, Ordering::Relaxed);

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let mut media_files = self.media_files.clone();
        let settings = self.settings.clone();
        let extract_dir = self.extract_dir.clone().unwrap();
        let cancel = self.cancel_flag.clone();

        let input = &self.input_files[0];
        let original_pptx_size = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);
        let stem = input.file_stem().unwrap_or_default().to_string_lossy();
        let dir = self
            .output_dir
            .clone()
            .unwrap_or_else(|| input.parent().unwrap_or(input.as_path()).to_path_buf());
        let output_path = queue::unique_output_path(&dir, &stem);
        self.output_path = Some(output_path.clone());

        std::thread::spawn(move || {
            let result = crate::core::pipeline::run_pipeline(
                &extract_dir,
                &output_path,
                &mut media_files,
                &settings,
                original_pptx_size,
                tx.clone(),
                cancel,
            );
            if let Err(e) = result {
                let _ = tx.send(CompressMessage::FatalError(e));
            }
        });
    }
}

impl eframe::App for PptxCompressorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_messages();

        self.render_titlebar(ctx);
        self.render_actionbar(ctx);

        let window_width = ctx.screen_rect().width();
        let primary_width = (window_width * 0.60).clamp(380.0, (window_width - 300.0).max(300.0));

        egui::SidePanel::left("workspace_primary")
            .exact_width(primary_width)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(WHITE)
                    .inner_margin(egui::Margin::symmetric(26.0, 20.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        file_panel::show(ui, self);
                        ui.add_space(22.0);
                        media_list::show(ui, self);
                    });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(SURFACE_2)
                    .inner_margin(egui::Margin::symmetric(24.0, 20.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        settings::show(ui, self);
                        ui.add_space(18.0);
                        components::hairline(ui);
                        ui.add_space(14.0);
                        progress::show(ui, self);
                        ui.add_space(16.0);
                        progress::show_notes(ui, self);
                    });
            });

        if matches!(self.state, AppState::Loading | AppState::Compressing) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = PersistedState {
            settings: self.settings.clone(),
            output_dir: self.output_dir.clone(),
            show_advanced: self.show_advanced,
            ffmpeg_override: self.ffmpeg_override.clone(),
        };
        if let Ok(raw) = serde_json::to_string(&state) {
            storage.set_string(STORAGE_KEY, raw);
        }
    }
}

impl PptxCompressorApp {
    fn render_titlebar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("titlebar")
            .exact_height(56.0)
            .frame(
                egui::Frame::none()
                    .fill(WHITE)
                    .inner_margin(egui::Margin::symmetric(20.0, 0.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // 品牌标记
                    egui::Frame::none()
                        .fill(ACCENT)
                        .rounding(8.0)
                        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("P")
                                    .color(WHITE)
                                    .size(14.0)
                                    .strong(),
                            );
                        });
                    ui.add_space(9.0);
                    ui.label(
                        egui::RichText::new("PPTX 压缩器")
                            .color(TEXT)
                            .size(14.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    components::version_badge(ui, env!("CARGO_PKG_VERSION"));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (_, fg, bg) = self.state.badge();
                        components::status_pill(ui, &self.state.badge_text(self.progress_pct()), fg, bg);
                        if !self.ffmpeg_available {
                            ui.add_space(8.0);
                            components::badge(ui, "FFmpeg 未就绪", AMBER, AMBER_SOFT);
                        }
                        if !self.font_ok {
                            ui.add_space(8.0);
                            components::badge(ui, "缺少中文字体", AMBER, AMBER_SOFT);
                        }
                    });
                });

                let rect = ui.max_rect();
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left(), rect.bottom()),
                        egui::pos2(rect.right(), rect.bottom()),
                    ],
                    egui::Stroke::new(1.0, LINE),
                );
            });
    }

    fn render_actionbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("actionbar")
            .exact_height(72.0)
            .frame(
                egui::Frame::none()
                    .fill(WHITE)
                    .inner_margin(egui::Margin::symmetric(24.0, 0.0)),
            )
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left(), rect.top()),
                        egui::pos2(rect.right(), rect.top()),
                    ],
                    egui::Stroke::new(1.0, LINE),
                );

                let (main, sub) = self.action_hint();
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(main).color(TEXT_2).size(11.5));
                        ui.add_space(3.0);
                        ui.label(egui::RichText::new(sub).color(MUTED).size(10.5));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.render_actions(ui);
                    });
                });
            });
    }

    fn action_hint(&self) -> (String, String) {
        match self.state {
            // 批量模式的专属文案；单文件路径的分支保持原样。
            AppState::Ready if self.is_batch() => (
                format!("已加入 {} 个 PPTX，将逐个独立压缩", self.batch_len()),
                format!("输出到 {} · 单文件失败不影响其他", self.output_hint()),
            ),
            AppState::Compressing if self.is_batch() => (
                format!("正在逐个压缩 {} 个文稿，可随时取消", self.batch_len()),
                self.progress_file.clone(),
            ),
            AppState::Done if self.is_batch() => {
                let summary = self
                    .queue
                    .as_ref()
                    .map(queue::BatchQueue::summary)
                    .unwrap_or_default();
                (
                    format!(
                        "批量完成：成功 {} 个、失败 {} 个",
                        summary.succeeded, summary.failed
                    ),
                    self.output_hint(),
                )
            }
            AppState::Error if self.is_batch() => (
                "全部文稿处理失败，请查看右侧日志定位原因".into(),
                "源文件未被修改，可清空队列后重试".into(),
            ),
            AppState::Idle => (
                "尚未选择文件".into(),
                "拖入 PPTX 或点击「选择 PPTX 文件」开始".into(),
            ),
            AppState::Loading => (
                "正在后台解析文稿，界面保持可操作".into(),
                "大文件解压不会阻塞窗口".into(),
            ),
            AppState::Ready => {
                let enabled = self
                    .media_files
                    .iter()
                    .filter(|m| m.enabled && m.media_type.is_compressible())
                    .count();
                let skipped = self.media_files.len().saturating_sub(enabled);
                (
                    format!("将处理 {} 个媒体文件，跳过 {} 个", enabled, skipped),
                    format!("输出到 {}", self.output_hint()),
                )
            }
            AppState::Compressing => (
                "正在重编码媒体并重新打包，可随时取消".into(),
                "取消后丢弃临时文件，原文件保持不变".into(),
            ),
            AppState::Done => (
                "压缩完成，输出文件已通过完整性校验".into(),
                self.output_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            AppState::Error => (
                "处理未完成，请查看右侧日志定位原因".into(),
                "源文件未被修改，可更换文件或重试".into(),
            ),
        }
    }

    fn output_hint(&self) -> String {
        if self.is_batch() {
            return self
                .output_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "各自源文件所在目录".into());
        }
        let Some(input) = self.input_files.first() else {
            return "（未选择源文件）".into();
        };
        let stem = input.file_stem().unwrap_or_default().to_string_lossy();
        let dir = self
            .output_dir
            .clone()
            .unwrap_or_else(|| input.parent().unwrap_or(input.as_path()).to_path_buf());
        queue::unique_output_path(&dir, &stem)
            .to_string_lossy()
            .into_owned()
    }

    fn render_actions(&mut self, ui: &mut egui::Ui) {
        // 主按钮永远在最右、尺寸固定，避免状态切换时按钮位置跳动。
        match self.state {
            AppState::Idle | AppState::Loading => {
                let _ = components::primary_button(ui, "开始压缩", false);
            }
            AppState::Ready => {
                let label = if self.is_batch() {
                    format!("开始压缩 {} 个文件", self.batch_len())
                } else {
                    "开始压缩".to_owned()
                };
                if components::primary_button(ui, &label, true).clicked() {
                    self.start_compression();
                }
                ui.add_space(8.0);
                let clear = if self.is_batch() {
                    "清空队列"
                } else {
                    "移除文件"
                };
                if components::secondary_button(ui, clear).clicked() {
                    self.reset_for_new_file();
                }
            }
            AppState::Compressing => {
                let _ = components::primary_button(ui, "压缩中…", false);
                ui.add_space(8.0);
                if components::secondary_button(ui, "取消").clicked() {
                    self.cancel_flag.store(true, Ordering::Relaxed);
                }
            }
            AppState::Done => {
                let again = if self.is_batch() {
                    "再压一批"
                } else {
                    "再压一个"
                };
                if components::primary_button(ui, again, true).clicked() {
                    self.reset_for_new_file();
                }
                ui.add_space(8.0);
                if components::secondary_button(ui, "打开结果文件夹").clicked() {
                    if let Some(path) = self.output_path.clone() {
                        if let Some(parent) = path.parent() {
                            let _ = components::open_folder(parent);
                        }
                    }
                }
                ui.add_space(8.0);
                if components::secondary_button(ui, "复制路径").clicked() {
                    if let Some(path) = self.output_path.clone() {
                        ui.output_mut(|o| o.copied_text = path.to_string_lossy().into_owned());
                        self.log("输出路径已复制到剪贴板");
                    }
                }
            }
            AppState::Error => {
                let retry = if self.is_batch() {
                    "重新压缩整批"
                } else {
                    "重新设置并重试"
                };
                if components::primary_button(ui, retry, true).clicked() {
                    if self.is_batch() {
                        self.start_batch();
                    } else {
                        let source = self.input_files.first().cloned();
                        if let Some(path) = source {
                            file_panel::load_pptx(self, Some(&path));
                        } else {
                            self.reset_for_new_file();
                        }
                    }
                }
                ui.add_space(8.0);
                let change = if self.is_batch() {
                    "清空队列"
                } else {
                    "更换文件"
                };
                if components::secondary_button(ui, change).clicked() {
                    self.reset_for_new_file();
                }
            }
        }
    }
}
