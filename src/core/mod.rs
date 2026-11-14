//! Core data types shared across the application.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod batch;
pub mod estimate;
pub mod extractor;
pub mod inspection;
pub mod ooxml;
pub mod pipeline;
pub mod queue;

/// Convert the 0–100 quality slider into an FFmpeg CRF value.
/// quality 0 -> CRF 51 (worst), quality 100 -> CRF 0 (lossless).
pub fn crf_from_quality(quality: u8) -> u8 {
    51u8.saturating_sub((quality as u32 * 51 / 100).min(51) as u8)
}

/// Classification of media files found inside a PPTX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Jpeg,
    Png,
    Gif,
    Video,
    Other,
}

impl MediaType {
    /// Detect media type from file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" => MediaType::Jpeg,
            "png" => MediaType::Png,
            "gif" => MediaType::Gif,
            "mp4" | "wmv" | "avi" | "mov" | "m4v" | "mpg" | "mpeg" | "webm" | "asf" => {
                MediaType::Video
            }
            _ => MediaType::Other,
        }
    }

    pub fn is_compressible(&self) -> bool {
        !matches!(self, MediaType::Other)
    }

    pub fn label(&self) -> &'static str {
        match self {
            MediaType::Jpeg => "JPEG",
            MediaType::Png => "PNG",
            MediaType::Gif => "GIF",
            MediaType::Video => "Video",
            MediaType::Other => "Other",
        }
    }

    /// Chinese label used by the desktop UI.
    pub fn label_zh(&self) -> &'static str {
        match self {
            MediaType::Jpeg => "JPEG 图片",
            MediaType::Png => "PNG 图片",
            MediaType::Gif => "GIF 动图",
            MediaType::Video => "视频",
            MediaType::Other => "其他",
        }
    }
}

/// Information about a single media file inside the PPTX.
#[derive(Debug, Clone)]
pub struct MediaInfo {
    /// Path relative to the PPTX root (e.g. "ppt/media/image1.jpg")
    pub relative_path: String,
    /// Absolute path inside the extracted temp directory
    pub extracted_path: PathBuf,
    /// Detected media type
    pub media_type: MediaType,
    /// Original file size in bytes
    pub original_size: u64,
    /// Size after compression (0 if not yet compressed)
    pub compressed_size: u64,
    /// Whether this file should be compressed
    pub enabled: bool,
    /// Current processing status
    pub status: MediaStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaStatus {
    Pending,
    Processing,
    Done,
    Skipped,
    Failed,
}

impl MediaInfo {
    pub fn new(relative_path: String, extracted_path: PathBuf, media_type: MediaType) -> Self {
        let original_size = std::fs::metadata(&extracted_path)
            .map(|m| m.len())
            .unwrap_or(0);
        MediaInfo {
            relative_path,
            extracted_path,
            media_type,
            original_size,
            compressed_size: 0,
            enabled: media_type.is_compressible(),
            status: MediaStatus::Pending,
        }
    }

    // v1.2.5：原先的 `estimated_size(&CompressionSettings)` 按媒体类型套固定
    // 压缩比（JPEG 0.15+0.55q、GIF 0.40…、视频 0.08+0.65q 等），与素材内容
    // 完全无关，却会随滑杆给出看似精确的「预计可节省 N%」。已整体移除，
    // 改由 `core::estimate::MediaComposition` 基于真实媒体构成报告可优化空间上界。
}

/// Preset shown as a segmented control in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    High,
    Balanced,
    Small,
    Custom,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Preset::High => "高质量",
            Preset::Balanced => "均衡",
            Preset::Small => "最小体积",
            Preset::Custom => "自定义",
        }
    }
}

/// User-configurable compression settings.
///
/// `serde(default)` keeps old preference files readable when new fields are
/// added in later versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressionSettings {
    /// JPEG quality (0–100, higher = better)
    pub image_quality: u8,
    /// Video quality (0–100, higher = better). See [`crf_from_quality`].
    pub video_quality: u8,
    /// GIF palette optimization
    pub gif_palette_optimize: bool,
    /// GIF frame drop
    pub gif_frame_drop: bool,
    /// GIF target frame rate
    pub gif_target_fps: u8,
    /// Limit image max width
    pub image_max_width_enabled: bool,
    /// Max image width in pixels
    pub image_max_width: u32,
    /// Limit video max resolution
    pub video_max_resolution_enabled: bool,
    /// Max video height in pixels (e.g. 1080)
    pub video_max_height: u32,
    /// Convert non-MP4 videos to H.264 MP4 (and update OOXML declarations)
    pub video_convert_to_h264: bool,
}

impl Default for CompressionSettings {
    fn default() -> Self {
        Self {
            image_quality: 75,
            video_quality: 45, // CRF 29
            gif_palette_optimize: true,
            gif_frame_drop: true,
            gif_target_fps: 15,
            image_max_width_enabled: false,
            image_max_width: 1920,
            video_max_resolution_enabled: false,
            video_max_height: 1080,
            video_convert_to_h264: true,
        }
    }
}

impl CompressionSettings {
    /// CRF value used by the FFmpeg call.
    pub fn crf(&self) -> u8 {
        crf_from_quality(self.video_quality)
    }

    /// Which segmented-control option matches the current values.
    pub fn preset(&self) -> Preset {
        let key = (
            self.image_quality,
            self.video_quality,
            self.image_max_width_enabled,
            self.image_max_width,
            self.video_max_resolution_enabled,
            self.video_max_height,
        );
        match key {
            (90, 65, false, 1920, false, 1080) => Preset::High,
            (75, 45, false, 1920, false, 1080) => Preset::Balanced,
            (30, 18, true, 1024, true, 720) => Preset::Small,
            _ => Preset::Custom,
        }
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        match preset {
            Preset::High => self.preset_high(),
            Preset::Balanced => self.preset_medium(),
            Preset::Small => self.preset_low(),
            Preset::Custom => {}
        }
    }

    fn reset_scaling(&mut self) {
        self.image_max_width_enabled = false;
        self.image_max_width = 1920;
        self.video_max_resolution_enabled = false;
        self.video_max_height = 1080;
    }

    /// Apply "High quality" preset.
    pub fn preset_high(&mut self) {
        self.image_quality = 90;
        self.video_quality = 65; // CRF 18
        self.reset_scaling();
    }

    /// Apply "Balanced" preset. Matches the default quality values.
    pub fn preset_medium(&mut self) {
        self.image_quality = 75;
        self.video_quality = 45; // CRF 29
        self.reset_scaling();
    }

    /// Apply "Small size" preset.
    pub fn preset_low(&mut self) {
        self.image_quality = 30;
        self.video_quality = 18; // CRF 42
        self.image_max_width_enabled = true;
        self.image_max_width = 1024;
        self.video_max_resolution_enabled = true;
        self.video_max_height = 720;
    }
}

/// Message from the background compression thread to the GUI.
#[derive(Debug, Clone)]
pub enum CompressMessage {
    /// (index, status, compressed_size) — a media file's status changed
    StatusUpdate(usize, MediaStatus, u64),
    /// (completed_count, total_count, current_file_name)
    Progress(usize, usize, String),
    /// All done: (original_total_bytes, compressed_total_bytes)
    Finished(u64, u64),
    /// A non-fatal problem (single file skipped, XML part unreadable, ...)
    Error(String),
    /// Fatal error — cannot continue
    FatalError(String),
    /// The user cancelled; no output file was written
    Cancelled,
}
