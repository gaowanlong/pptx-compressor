//! 平台能力抽象层。
//!
//! 鸿蒙电脑（HarmonyOS PC / 2in1）的 Rust target 是
//! `aarch64-unknown-linux-ohos` / `x86_64-unknown-linux-ohos`：
//! `target_os = "linux"`、`target_env = "ohos"`、C 运行时为 musl。
//! 因此凡是写 `#[cfg(target_os = "linux")]` 的地方都必须排除 `ohos`，
//! 否则会错误套用桌面 Linux 的字体、PATH 与数据目录约定。
//!
//! HAP 应用沙箱内**不能依赖可执行子进程**（无法再分发/执行独立 FFmpeg），
//! 也不能假定 `HOME`、`TMPDIR` 存在；这些路径由 ArkTS 侧显式注入。

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use tempfile::TempDir;

/// 是否为鸿蒙目标。`target_env = "ohos"` 是最可靠的判定条件。
pub const fn is_harmonyos() -> bool {
    cfg!(target_env = "ohos")
}

/// 平台标识，写进能力查询的 JSON，供 ArkTS 界面显示与埋点。
pub fn platform_label() -> &'static str {
    if is_harmonyos() {
        "harmonyos_pc"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux_desktop"
    }
}

/// 鸿蒙沙箱注入后（或测试显式模拟后）关闭视频转码。
static VIDEO_DISABLED: AtomicBool = AtomicBool::new(false);

/// 视频重编码是否可用。
///
/// 桌面版依赖用户本机 FFmpeg 子进程；鸿蒙 HAP 沙箱没有可执行的外部
/// FFmpeg，视频项必须整体跳过，但图片 / GIF / XML / OOXML 引用完整性
/// 等核心能力不受影响，输出仍是合法 PPTX。
pub fn video_transcode_available() -> bool {
    !VIDEO_DISABLED.load(Ordering::Relaxed)
}

/// 由 `init_sandbox()` 在鸿蒙侧调用；桌面端不调用，行为完全不变。
pub fn disable_video_transcode() {
    VIDEO_DISABLED.store(true, Ordering::Relaxed);
}

/// 测试辅助：在非鸿蒙主机上模拟鸿蒙的能力边界。
pub fn set_video_transcode_available(available: bool) {
    VIDEO_DISABLED.store(!available, Ordering::Relaxed);
}

/// 图片并行上限。鸿蒙 PC 内存与调度策略与桌面不同，保守用 2 线程。
pub fn max_image_threads() -> usize {
    if is_harmonyos() {
        2
    } else {
        4
    }
}

/// ArkTS 通过 `init` 注入的沙箱目录。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxPaths {
    /// `context.filesDir` — 偏好、输出、文稿副本。
    pub data_dir: PathBuf,
    /// `context.cacheDir` — 可清理的中间产物。
    pub cache_dir: PathBuf,
    /// 通常为 `cacheDir/tmp` — 解压树与候选文件。
    pub temp_dir: PathBuf,
}

static SANDBOX: OnceLock<RwLock<Option<SandboxPaths>>> = OnceLock::new();

fn sandbox_cell() -> &'static RwLock<Option<SandboxPaths>> {
    SANDBOX.get_or_init(|| RwLock::new(None))
}

pub fn sandbox_paths() -> Option<SandboxPaths> {
    sandbox_cell().read().ok().and_then(|guard| guard.clone())
}

/// 由鸿蒙 HAP 启动时调用一次：建立目录、记录路径、关闭视频转码。
///
/// 桌面版不调用此方法，因此 `tempdir()` 与 FFmpeg 查找路径保持原样。
pub fn init_sandbox(paths: SandboxPaths) -> Result<(), String> {
    for dir in [&paths.data_dir, &paths.cache_dir, &paths.temp_dir] {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建沙箱目录 {} 失败：{e}", dir.display()))?;
    }
    let mut guard = sandbox_cell()
        .write()
        .map_err(|e| format!("沙箱状态锁损坏：{e}"))?;
    *guard = Some(paths);
    if is_harmonyos() {
        disable_video_transcode();
    }
    Ok(())
}

/// 创建临时目录。鸿蒙侧固定落在注入的 `temp_dir` 内，避免依赖 `TMPDIR`。
pub fn tempdir() -> io::Result<TempDir> {
    match sandbox_paths() {
        Some(paths) => tempfile::Builder::new()
            .prefix("pptx-")
            .tempdir_in(&paths.temp_dir),
        None => tempfile::tempdir(),
    }
}

/// 用户数据目录。鸿蒙侧用沙箱注入路径，桌面侧沿用 `dirs`。
pub fn data_dir() -> Option<PathBuf> {
    if let Some(paths) = sandbox_paths() {
        return Some(paths.data_dir);
    }
    #[cfg(not(target_env = "ohos"))]
    {
        dirs::data_dir()
    }
    #[cfg(target_env = "ohos")]
    {
        None
    }
}
