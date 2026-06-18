# PPTX Compressor

![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

一个跨平台的 PowerPoint（PPTX）文件压缩工具，通过重新压缩内嵌的图片、GIF 和视频来减小文件大小。提供 macOS 原生 .app 和 Windows 原生 exe。

## 功能

- **图片压缩**
  - JPEG：可调画质（0-100），可选最大宽度缩放
  - PNG：最大压缩级别重新编码，不变大则不替换
  - GIF：可调帧率，调色板优化
- **视频压缩**
  - H.264 编码，可调画质（0-100，统一滑块逻辑）
  - 可选分辨率缩放
  - H.264 格式转换开关，提升跨平台兼容性
  - 静态打包的 FFmpeg，无需额外下载
- **XML 优化**：清理 PPTX 内 XML 注释和多余空白
- **实时预估**：调整参数时即时显示预估压缩率，压缩完成后展示实际结果
- **Apple HIG 风格界面**
  - 毛玻璃卡片设计
  - 拖拽或浏览选择 PPTX 文件
  - 三种预设（高/中/低画质）
  - 单文件启用/禁用
  - 实时进度显示

## 使用方式

从 [Releases](https://github.com/gaowanlong/pptx-compressor/releases) 下载对应平台的压缩包，解压后直接运行：

- **macOS**：打开 `PPTX Compressor.app`（若 Gatekeeper 拦截，前往 系统设置 → 隐私与安全性 → 仍要打开）
- **Windows**：运行 `pptx-compressor.exe`

> macOS 版本首次打开可能被 Gatekeeper 拦截，因为开发者未通过 Apple 认证签名，这是正常的。

## 编译

### 环境要求

- Rust 1.74+
- 对应平台工具链

### macOS 原生编译

```bash
cargo build --release
```

产物：`target/release/pptx-compressor`

打包 .app（需准备 FFmpeg 二进制至 `resources/ffmpeg/macos/ffmpeg`）：

```bash
./package.sh macos
```

### macOS → Windows 交叉编译

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
```

产物：`target/x86_64-pc-windows-msvc/release/pptx-compressor.exe`

打包 Windows 分发版（需准备 FFmpeg 二进制至 `resources/ffmpeg/windows/ffmpeg.exe`）：

```bash
./package.sh windows
```

### Windows 原生编译

```bash
cargo build --release
```

## 技术栈

- **GUI：** egui + eframe（纯 Rust 即时模式 GUI，毛玻璃风格）
- **图片处理：** `image` crate（JPEG 重编码 + PNG 最优压缩）
- **GIF 处理：** `gif` crate（帧率调减 + 调色板优化）
- **视频处理：** FFmpeg 子进程（H.264 CRF + 分辨率缩放），静态打包
- **PPTX/ZIP：** `zip` crate + `quick-xml`
- **构建：** cargo-xwin（macOS → Windows 交叉编译）

## 开源协议

本项目基于 MIT 协议开源。
