//! 跨平台共享的持久化状态模型。
//!
//! 桌面版存到 eframe storage、鸿蒙版存到 `@ohos.data.preferences`，
//! 但两边**读写的是同一个 JSON 结构、同一个键名**
//! `pptx-compressor.state.v1`，因此设置、输出目录、FFmpeg 覆盖路径
//! 可以在两端无缝迁移。`serde(default)` 保证旧版本数据仍可解析。

use crate::core::CompressionSettings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 持久化键；桌面与鸿蒙保持一致。
pub const STORAGE_KEY: &str = "pptx-compressor.state.v1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    pub settings: CompressionSettings,
    pub output_dir: Option<PathBuf>,
    pub show_advanced: bool,
    /// 用户手动指定的 FFmpeg 可执行文件；鸿蒙沙箱内恒为 `None`。
    pub ffmpeg_override: Option<String>,
}

impl PersistedState {
    /// 解析偏好字符串；解析失败时静默回退到默认值（与桌面版行为一致）。
    pub fn from_json(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
