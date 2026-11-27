//! PPTX Compressor — library entry point.
//!
//! 分层约定：
//! - `core` / `handlers` / `utils` 是**平台无关**的纯 Rust 压缩内核，
//!   Windows、macOS、HarmonyOS PC 共用同一份实现与同一份数据格式；
//! - `app` / `ui` 是桌面版 egui 外壳，只在 `desktop` feature 下编译；
//! - `core::bridge` 是与 ArkTS 外壳约定的 JSON 协议层；
//! - `ffi` 是给鸿蒙 HAP 内 C++ N-API 链接的 C ABI（桌面端也可编译用于测试）。

pub mod core;
pub mod handlers;
pub mod utils;

#[cfg(feature = "desktop")]
pub mod app;
#[cfg(feature = "desktop")]
pub mod ui;

#[cfg(feature = "ffi")]
pub mod ffi;
