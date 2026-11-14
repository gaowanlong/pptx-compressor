# PPTX 压缩器

![Version](https://img.shields.io/badge/version-v1.1.0-blue.svg)
![macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)
![Windows](https://img.shields.io/badge/platform-Windows-0078d7.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

一个跨平台的 PowerPoint（PPTX）压缩工具：把 PPTX 当 ZIP 解开，重新编码内嵌的图片、GIF 与视频，再重新打包。提供 macOS 原生 `.app` 与 Windows 原生 exe，纯 Rust + egui 实现，界面为全中文。

## 功能

- **图片压缩**
  - JPEG：可调画质（0–100），可选最大宽度缩放，统一输出 3 通道 RGB
  - PNG：Best 级别重编码 + 自适应滤波
  - GIF：可调帧率、调色板优化，保留透明索引与帧处置方式
  - 所有媒体统一遵循「只有更小才替换」，变大或失败时自动保留原文件
- **视频压缩**
  - H.264 / AAC 编码，画质 0–100 映射到 CRF（界面显示真实 CRF）
  - 可选分辨率缩放，`yuv420p` + High Profile，level 由编码器按分辨率自动选择
  - 「兼容模式」开启时，非 MP4 视频转为 `.mp4` 并**同步更新 `[Content_Types].xml` 与 `.rels` 引用**；关闭时跳过非 MP4 视频
  - FFmpeg 随发布包提供；开发环境缺失时会尝试一次性下载
- **XML 与包清理**
  - 剥离 XML 注释（保留文本空白，避免破坏 `<a:t>` 内容）
  - 剥离 `docProps/thumbnail.*` 预览图，并同步清理对应的关系与内容类型声明
- **输出安全**
  - 同名结果文件自动追加序号，**绝不覆盖**已有文件
  - 先写临时文件、回读校验 ZIP 完整性、再原子替换
  - 解压有条目数 / 体积 / 压缩比 / 符号链接防护，图片解码有尺寸与内存上限
- **中文界面**
  - 左侧主导区：源文件 + 媒体清单（逐项开关、体积预估、处理状态与原因）
  - 右侧从属区：三档预设、质量滑块（显示真实 CRF）、兼容模式、高级设置折叠、进度与结果
  - 主操作按钮固定在右下角，空 / 解析中 / 就绪 / 压缩中 / 完成 / 失败六态均有明确反馈
  - 活动日志显示跳过与失败的具体原因；预设、输出目录、高级项展开状态退出后自动保存

> 体积「预估」为启发式估算，仅用于调参参考，实际结果以压缩完成后的数字为准。

## 界面

界面按 Ardot UI 设计技能的 Web App 规范重设计：单一主导区、渐进披露、状态可见、一屏一主操作，纯白底 + 单一强调色，去掉等权卡片堆叠与毛玻璃。系统字体优先使用苹方（macOS）/ 微软雅黑（Windows）/ Noto Sans CJK（Linux）；找不到中文字体时标题栏会明确提示，而不是静默显示方块。

解包与压缩都在后台线程执行，超大文稿不会冻结窗口；取消会终止 FFmpeg 子进程并重新从源文件解压，避免基于已损失画质的数据二次压缩。

## 使用方式

从 [Releases](https://github.com/gaowanlong/pptx-compressor/releases) 下载对应平台的压缩包，解压后直接运行：

- **macOS**：打开 `PPTX Compressor.app`（若 Gatekeeper 拦截，前往 系统设置 → 隐私与安全性 → 仍要打开）
- **Windows**：运行 `pptx-compressor.exe`

把 `.pptx` 拖入窗口或点击「选择 PPTX 文件」，确认右侧方案后点击右下角「开始压缩」。输出默认写到源文件同目录，命名为 `<原名>_compressed.pptx`；若已存在则改为 `_compressed_2.pptx`，以此类推。原文件永远不会被修改。压缩参数与输出目录由 eframe 写入各平台标准应用数据目录，下次启动自动恢复。

## 版本历史

- **v1.1.0**：落地全新中文双区界面、解包后台化、设置持久化、中文字体加载、活动日志与逐项原因展示，补齐偏好回归测试与合规 FFmpeg 替换路径；后台 44 处错误与日志文案中文化；按 clippy 建议清理代码质量问题。
- **v1.0.1**：修复压缩变大不回滚、视频容器与 OOXML 声明不一致、缩略图悬空关系、GIF 调色板错色、FFmpeg 下载后找不到、输出非原子写、取消无法中断编码等正确性问题。

完整清单见 [CHANGELOG.md](CHANGELOG.md)，Release 说明直接取自对应章节。

## 已知限制

- 仅支持未加密的 `.pptx`；`.pptm` 宏文稿与带数字签名的文稿，压缩后签名会失效，建议先另存副本。
- 一次只处理一个文件，暂不支持批量队列；媒体压缩仍为串行。
- JPEG 重编码不应用 EXIF 方向、不保留 ICC / 文本元数据；GIF 循环次数统一写为无限循环。
- 体积「预估」是启发式估算，不代表真实结果。
- 内置 FFmpeg 为 GPLv3 + nonfree 构建，**再分发存在合规风险**，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。本版提供环境变量替换路径，但默认二进制仍未更换。
- 已完成无窗口六态布局渲染冒烟与 macOS 真实窗口启动冒烟（17 秒无报错）；中文字体显示与像素级视觉回归仍需在目标系统目视确认。

## 开发

### 环境要求

- Rust 1.74+（已在 1.96 验证）
- 对应平台工具链；Windows 交叉编译需 `cargo-xwin` 与 `x86_64-pc-windows-msvc` target

### 测试与编译

```bash
cargo check --all-targets
cargo test                       # 单元测试与 ZIP 往返测试
cargo test -- --include-ignored  # 额外跑真实 FFmpeg 转码端到端用例
cargo clippy --all-targets -- -D warnings
cargo build --release            # 产物：target/release/pptx-compressor
```

### 打包

`package.sh` 会自动创建完整的 macOS `.app` 骨架（Info.plist、图标、Resources），无需预先准备 app bundle：

```bash
./package.sh macos        # target/PPTX-Compressor-v<版本>-macOS.zip
./package.sh windows      # 需 cargo-xwin，输出 Windows zip
./package.sh all          # 两个平台
```

默认从 `resources/ffmpeg/macos/ffmpeg` 与 `resources/ffmpeg/windows/ffmpeg.exe` 取二进制。**如需换成 LGPL 合规构建**，用环境变量指定自备文件即可，脚本会把它们打进对应位置：

```bash
FFMPEG_MACOS=/path/to/lgpl-ffmpeg ./package.sh macos
FFMPEG_WINDOWS=/path/to/lgpl-ffmpeg.exe ./package.sh windows
```

发布包会附带 README、CHANGELOG 与第三方许可说明。macOS → Windows 交叉编译也可手动执行：

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
```

## 技术栈

- **GUI**：eframe + egui（纯 Rust 即时模式 GUI），`persistence` 保存偏好
- **图片 / GIF**：`image` crate、`gif` crate
- **视频**：FFmpeg 子进程（libx264 CRF + 分辨率缩放）
- **PPTX / ZIP**：`zip` crate
- **偏好序列化**：`serde` / `serde_json`
- **构建**：cargo-xwin（macOS → Windows 交叉编译）

## 开源协议与第三方组件

本项目**源码**基于 MIT 协议开源。发布包内的 FFmpeg 二进制另有许可证约束（GPLv3 且启用了 nonfree，官方不允许再分发），详情见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)；如需合规再分发，请使用上一节的环境变量替换为 LGPL 兼容构建。
