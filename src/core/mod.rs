//! Core data types shared across the application.

use std::path::PathBuf;

pub mod extractor;
pub mod pipeline;
pub mod scanner;

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

    /// Estimated compressed size (rough heuristic for preview).
    pub fn estimated_size(&self, settings: &CompressionSettings) -> u64 {
        if !self.enabled {
            return self.original_size;
        }
        let ratio = match self.media_type {
            MediaType::Jpeg => {
                // JPEG: quality-based ratio
                let q = settings.image_quality as f64 / 100.0;
                0.15 + 0.55 * q // quality 75 -> ~56% ratio
            }
            MediaType::Png => 0.85, // oxipng typically saves 10-20%
            MediaType::Gif => 0.45, // palette + frame drop can be aggressive
            MediaType::Video => {
                // CRF-based: lower CRF = bigger file
                let crf = settings.video_crf as f64;
                let ratio = 0.1 + (crf / 51.0) * 0.6; // CRF 28 -> ~43%
                ratio
            }
            MediaType::Other => 1.0,
        };
        (self.original_size as f64 * ratio) as u64
    }
}

/// User-configurable compression settings.
#[derive(Debug, Clone)]
pub struct CompressionSettings {
    /// JPEG quality (1–100, higher = better)
    pub image_quality: u8,
    /// Video CRF (0–51, lower = better)
    pub video_crf: u8,
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
}

impl Default for CompressionSettings {
    fn default() -> Self {
        Self {
            image_quality: 75,
            video_crf: 28,
            gif_palette_optimize: true,
            gif_frame_drop: true,
            gif_target_fps: 15,
            image_max_width_enabled: false,
            image_max_width: 1920,
            video_max_resolution_enabled: false,
            video_max_height: 1080,
        }
    }
}

impl CompressionSettings {
    /// Apply "High quality" preset.
    pub fn preset_high(&mut self) {
        self.image_quality = 90;
        self.video_crf = 18;
    }

    /// Apply "Medium quality" preset.
    pub fn preset_medium(&mut self) {
        self.image_quality = 75;
        self.video_crf = 28;
    }

    /// Apply "Low quality" preset.
    pub fn preset_low(&mut self) {
        self.image_quality = 50;
        self.video_crf = 35;
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
    /// An error occurred (non-fatal, single file skipped)
    Error(String),
    /// Fatal error — cannot continue
    FatalError(String),
}
