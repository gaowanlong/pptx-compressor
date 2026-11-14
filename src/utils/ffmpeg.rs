//! FFmpeg 探测与能力识别。
//!
//! 本项目**不再随包分发、也不再自动下载 FFmpeg**。仓库内曾经内置的
//! macOS / Windows 二进制是 `--enable-gpl --enable-version3 --enable-nonfree`
//! 构建，与 MIT 源码捆绑再分发存在明确的法律风险，且 macOS 侧没有可直接
//! 再分发的 LGPL 预编译版本。因此视频压缩改为使用用户本机已有的 FFmpeg：
//!
//! 1. 用户在偏好里显式指定的路径
//! 2. 维护者用合规二进制打包时随包提供的副本
//! 3. 用户数据目录里自行放置的副本
//! 4. 系统 PATH（Homebrew / 官方安装包等）
//!
//! 用户自行安装 GPL 构建供个人使用不构成再分发，运行时不做许可证拦截；
//! 许可证门禁只作用于打包流程（见 `scripts/ffmpeg-license-check.sh`）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock, RwLock};

#[cfg(target_os = "windows")]
const FFMPEG_BIN: &str = "ffmpeg.exe";
#[cfg(not(target_os = "windows"))]
const FFMPEG_BIN: &str = "ffmpeg";

/// FFmpeg 的构建许可证类别，由 `ffmpeg -version` 的 configuration 推断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseClass {
    /// 未启用 gpl / version3 / nonfree，通常为 LGPL 构建。
    Lgpl,
    /// 启用了 `--enable-gpl` 或 `--enable-version3`。
    Gpl,
    /// 启用了 `--enable-nonfree`，官方不允许再分发。
    Nonfree,
    /// 无法解析（例如精简构建或输出被裁剪）。
    Unknown,
}

impl LicenseClass {
    pub fn label(self) -> &'static str {
        match self {
            LicenseClass::Lgpl => "LGPL 构建",
            LicenseClass::Gpl => "GPL 构建（仅供个人使用）",
            LicenseClass::Nonfree => "nonfree 构建（不可再分发）",
            LicenseClass::Unknown => "构建许可未识别",
        }
    }

    /// 是否允许被打包进发布产物。
    pub fn redistributable(self) -> bool {
        matches!(self, LicenseClass::Lgpl)
    }
}

/// FFmpeg 从哪里找到，用于在界面上如实说明来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegSource {
    UserConfigured,
    Bundled,
    UserData,
    SystemPath,
}

impl FfmpegSource {
    pub fn label(self) -> &'static str {
        match self {
            FfmpegSource::UserConfigured => "手动指定",
            FfmpegSource::Bundled => "随包提供",
            FfmpegSource::UserData => "用户数据目录",
            FfmpegSource::SystemPath => "系统 PATH",
        }
    }
}

/// 一次成功探测的结果。
#[derive(Debug, Clone)]
pub struct LocatedFfmpeg {
    pub command: String,
    pub source: FfmpegSource,
    pub license: LicenseClass,
}

/// 可用的 H.264 编码器。LGPL 构建通常没有 libx264，只有 libopenh264。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderSupport {
    pub x264: bool,
    pub openh264: bool,
}

impl EncoderSupport {
    pub fn preferred(self) -> Option<&'static str> {
        if self.x264 {
            Some("libx264")
        } else if self.openh264 {
            Some("libopenh264")
        } else {
            None
        }
    }
}

fn is_runnable_ffmpeg(command: &str) -> bool {
    Command::new(command)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        if path
            .metadata()
            .ok()
            .is_some_and(|m| m.permissions().mode() & 0o111 == 0)
        {
            return false;
        }
    }
    true
}

/// 解析 `ffmpeg -version` 输出中的 configuration 行，判断构建许可证。
pub fn detect_license(command: &str) -> LicenseClass {
    let Ok(output) = Command::new(command).arg("-version").output() else {
        return LicenseClass::Unknown;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find(|l| l.trim_start().starts_with("configuration:"))
        .map(license_from_configuration)
        .unwrap_or(LicenseClass::Unknown)
}

/// 解析 configuration 文本（便于单元测试，不需要真实 ffmpeg）。
pub fn license_from_configuration(line: &str) -> LicenseClass {
    if line.contains("--enable-nonfree") {
        LicenseClass::Nonfree
    } else if line.contains("--enable-gpl") || line.contains("--enable-version3") {
        LicenseClass::Gpl
    } else {
        LicenseClass::Lgpl
    }
}

fn encoder_cache() -> &'static Mutex<HashMap<String, EncoderSupport>> {
    static CACHE: OnceLock<Mutex<HashMap<String, EncoderSupport>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 探测可用的 H.264 编码器，按可执行文件路径缓存结果。
pub fn encoder_support(command: &str) -> EncoderSupport {
    if let Ok(cache) = encoder_cache().lock() {
        if let Some(hit) = cache.get(command) {
            return *hit;
        }
    }
    let support = probe_encoders(command);
    if let Ok(mut cache) = encoder_cache().lock() {
        cache.insert(command.to_owned(), support);
    }
    support
}

fn probe_encoders(command: &str) -> EncoderSupport {
    let Ok(output) = Command::new(command)
        .args(["-hide_banner", "-encoders"])
        .output()
    else {
        return EncoderSupport { x264: false, openh264: false };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    EncoderSupport {
        x264: text.lines().any(|l| l.split_whitespace().any(|w| w == "libx264")),
        openh264: text.lines().any(|l| l.split_whitespace().any(|w| w == "libopenh264")),
    }
}

/// 打包与开发时可能放置 FFmpeg 的位置（按优先级）。
fn candidate_paths() -> Vec<(PathBuf, FfmpegSource)> {
    let mut paths = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            #[cfg(target_os = "macos")]
            if exe_dir.ends_with("MacOS") {
                if let Some(contents) = exe_dir.parent() {
                    paths.push((
                        contents.join("Resources").join(FFMPEG_BIN),
                        FfmpegSource::Bundled,
                    ));
                }
            }

            #[cfg(target_os = "windows")]
            paths.push((exe_dir.join(FFMPEG_BIN), FfmpegSource::Bundled));
        }
    }

    if let Some(data_dir) = dirs::data_local_dir() {
        paths.push((
            data_dir
                .join("pptx-compressor")
                .join("ffmpeg")
                .join(FFMPEG_BIN),
            FfmpegSource::UserData,
        ));
    }

    paths
}

/// 按优先级定位 FFmpeg。`preferred` 是用户在设置里手动指定的路径。
pub fn locate(preferred: Option<&str>) -> Option<LocatedFfmpeg> {
    if let Some(raw) = preferred.map(str::trim).filter(|s| !s.is_empty()) {
        let path = Path::new(raw);
        if executable_file(path) && is_runnable_ffmpeg(raw) {
            return Some(LocatedFfmpeg {
                command: raw.to_owned(),
                source: FfmpegSource::UserConfigured,
                license: detect_license(raw),
            });
        }
        // 显式指定但不可用时不静默回退：让用户知道配置失效，
        // 但其余媒体仍然可以压缩，因此继续走后续候选路径。
    }

    for (path, source) in candidate_paths() {
        if !executable_file(&path) {
            continue;
        }
        let command = path.to_string_lossy().into_owned();
        if is_runnable_ffmpeg(&command) {
            return Some(LocatedFfmpeg {
                license: detect_license(&command),
                command,
                source,
            });
        }
    }

    if is_runnable_ffmpeg(FFMPEG_BIN) {
        return Some(LocatedFfmpeg {
            command: FFMPEG_BIN.into(),
            source: FfmpegSource::SystemPath,
            license: detect_license(FFMPEG_BIN),
        });
    }

    None
}

/// 不带用户偏好的便捷入口（默认值、测试与工具脚本使用）。
pub fn find_ffmpeg() -> Option<String> {
    locate(None).map(|f| f.command)
}

/// 会话级的用户指定路径。后台压缩线程拿不到 GUI 状态，通过这个显式
/// 运行时配置传递，避免把不可序列化的路径塞进 `CompressionSettings`。
fn preferred_store() -> &'static RwLock<Option<String>> {
    static STORE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

pub fn set_preferred(path: Option<String>) {
    if let Ok(mut slot) = preferred_store().write() {
        *slot = path;
    }
}

pub fn preferred() -> Option<String> {
    preferred_store().read().ok().and_then(|slot| slot.clone())
}

/// GUI 与后台线程共用的定位入口。
pub fn locate_current() -> Option<LocatedFfmpeg> {
    locate(preferred().as_deref())
}

/// 校验用户手动选择的文件确实是可用的 FFmpeg。
pub fn validate_path(path: &Path) -> Result<LocatedFfmpeg, String> {
    if !executable_file(path) {
        return Err("所选文件不存在或不可执行".into());
    }
    let command = path.to_string_lossy().into_owned();
    if !is_runnable_ffmpeg(&command) {
        return Err("所选文件不是可用的 FFmpeg（无法执行 ffmpeg -version）".into());
    }
    Ok(LocatedFfmpeg {
        license: detect_license(&command),
        command,
        source: FfmpegSource::UserConfigured,
    })
}
