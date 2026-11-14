# PPTX Compressor

![Version](https://img.shields.io/badge/version-v1.0.1-blue.svg)
![macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)
![Windows](https://img.shields.io/badge/platform-Windows-0078d7.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

一个跨平台的 PowerPoint（PPTX）文件压缩工具：把 PPTX 当 ZIP 解开，重新编码内嵌的图片、GIF 和视频，再重新打包。提供 macOS 原生 `.app` 与 Windows 原生 exe，纯 Rust + egui 实现。

## 功能

- **图片压缩**
  - JPEG：可调画质（0–100），可选最大宽度缩放，统一输出 3 通道 RGB
  - PNG：Best 级别重编码 + 自适应滤波
  - GIF：可调帧率、调色板优化、透明索引与帧处置方式保留
  - 所有媒体统一遵循「只有更小才替换」，变大或失败时自动保留原文件
- **视频压缩**
  - H.264 / AAC 编码，画质 0–100 映射到 CRF（界面显示真实 CRF）
  - 可选分辨率缩放，`yuv420p` + High Profile，level 由编码器按分辨率自动选择
  - 「兼容模式」开启时，非 MP4 视频转为 `.mp4` 并**同步更新 `[Content_Types].xml` 与 `.rels` 引用**；关闭时跳过非 MP4 视频，避免格式与声明不符
  - FFmpeg 随发布包静态提供；开发环境缺失时会尝试一次性下载
- **XML 与包清理**
  - 剥离 XML 注释（保留文本空白，避免破坏 `<a:t>` 内容）
  - 剥离 `docProps/thumbnail.*` 预览图，并同步清理对应的关系与内容类型声明
- **输出安全**
  - 同名结果文件自动追加序号，不覆盖已有文件
  - 先写临时文件、回读校验 ZIP 完整性、再原子替换
  - 解压有条目数 / 体积 / 压缩比 / 符号链接防护，图片解码有尺寸与内存上限
- **界面**：拖拽或浏览选择单个 PPTX、三档预设（有选中态）、逐文件启用/禁用、实时进度与活动日志、压缩前后体积对比

> 体积「预估」为启发式估算，仅用于调参参考，实际结果以压缩完成后的数字为准。

## 使用方式

从 [Releases](https://github.com/gaowanlong/pptx-compressor/releases) 下载对应平台的压缩包，解压后直接运行：

- **macOS**：打开 `PPTX Compressor.app`（若 Gatekeeper 拦截，前往 系统设置 → 隐私与安全性 → 仍要打开）
- **Windows**：运行 `pptx-compressor.exe`

输出默认写到源文件同目录，命名为 `<原名>_compressed.pptx`；若已存在则改为 `_compressed_2.pptx`，以此类推。原文件永远不会被修改。

## v1.0.1 修复内容

该版本修复了压缩变大不回滚、视频容器与 OOXML 声明不一致、缩略图悬空关系、GIF 调色板错色、FFmpeg 下载后找不到、输出非原子写、取消无法中断编码等一系列正确性问题，并补充回归测试与构建脚本修复。完整清单见 [CHANGELOG.md](CHANGELOG.md)。

## 已知限制

- 仅支持未加密的 `.pptx`；`.pptm` 宏文稿与带数字签名的文稿，压缩后签名会失效，建议先另存副本。
- 一次只处理一个文件，暂不支持批量队列。
- JPEG 重编码不应用 EXIF 方向、不保留 ICC / 文本元数据；GIF 循环次数统一写为无限循环。
- 解包仍在 GUI 线程同步执行，超大文稿会短暂卡顿；媒体压缩仍为串行。
- 内置 FFmpeg 为 GPLv3 + nonfree 构建，再分发存在合规风险，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## 开发

### 环境要求

- Rust 1.74+（已在 1.96 验证）
- 对应平台工具链；Windows 交叉编译需 `cargo-xwin` 与 `x86_64-pc-windows-msvc` target

### 测试与编译

```bash
cargo test                # 单元测试与 ZIP 往返测试
cargo build --release     # 产物：target/release/pptx-compressor
```

### 打包

`package.sh` 会自动创建完整的 macOS `.app` 骨架（Info.plist、图标、Resources），无需预先准备 app bundle：

```bash
./package.sh macos        # target/PPTX-Compressor-v<版本>-macOS.zip
./package.sh windows      # 需 cargo-xwin，输出 Windows zip
./package.sh all          # 两个平台
```

打包前请确认 FFmpeg 二进制已就位：`resources/ffmpeg/macos/ffmpeg`、`resources/ffmpeg/windows/ffmpeg.exe`。发布包会附带 README、CHANGELOG 与第三方许可说明。

macOS → Windows 交叉编译也可手动执行：

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
```

## 技术栈

- **GUI**：eframe + egui（纯 Rust 即时模式 GUI）
- **图片 / GIF**：`image` crate、`gif` crate
- **视频**：FFmpeg 子进程（libx264 CRF + 分辨率缩放）
- **PPTX / ZIP**：`zip` crate
- **构建**：cargo-xwin（macOS → Windows 交叉编译）

## 开源协议与第三方组件

本项目**源码**基于 MIT 协议开源。发布包内的 FFmpeg 二进制另有许可证约束（GPLv3 且启用了 nonfree，官方不允许再分发），详情见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)；如需合规再分发，请替换为 LGPL 兼容构建。
