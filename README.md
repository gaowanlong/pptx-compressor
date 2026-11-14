# PPTX 压缩器

![Version](https://img.shields.io/badge/version-v1.2.1-blue.svg)
![macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)
![Windows](https://img.shields.io/badge/platform-Windows-0078d7.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

一个跨平台的 PowerPoint（PPTX）压缩工具：把 PPTX 当 ZIP 解开，重新编码内嵌的图片、GIF 与视频，再重新打包。提供 macOS 原生 `.app` 与 Windows 原生 exe，纯 Rust + egui 实现，界面为全中文。

> **v1.2.0 重要变更**：发布包**不再内置 FFmpeg**，也不会自动下载。图片 / GIF / XML 压缩不受影响；视频压缩需要你本机提供一个 FFmpeg。原因与做法见 [FFMPEG_SETUP.md](FFMPEG_SETUP.md)。

## 功能

- **图片压缩**
  - JPEG：可调画质（0–100），可选最大宽度缩放，统一输出 3 通道 RGB
  - PNG：Best 级别重编码 + 自适应滤波
  - GIF：可调帧率、调色板优化，保留透明索引与帧处置方式
  - 所有媒体统一遵循「只有更小才替换」，变大或失败时自动保留原文件
  - 图片与 GIF 用最多 4 线程并行重编码，视频转码保持串行以免内存爆炸
- **视频压缩**（需自备 FFmpeg）
  - H.264 / AAC 编码；有 `libx264` 时画质滑杆映射为 CRF（界面显示真实 CRF），只有 `libopenh264` 的 LGPL 构建自动切换为码率模型
  - 可选分辨率缩放，`yuv420p` 保证 PowerPoint 可播放，H.264 level 交给编码器决定
  - 「兼容模式」开启时，非 MP4 视频转为 `.mp4` 并**同步更新 `[Content_Types].xml` 与 `.rels` 引用**；关闭时跳过非 MP4 视频
  - 找不到 FFmpeg 或缺少 H.264 编码器时，视频项明确报错并写入活动日志，绝不静默跳过
- **XML 与包清理**
  - 剥离 XML 注释（保留文本空白，避免破坏 `<a:t>` 内容）
  - 剥离 `docProps/thumbnail.*` 预览图，并同步清理对应的关系与内容类型声明
- **输入安全检测**
  - 解压前只读检查：加密 / IRM 保护、旧版 `.ppt` 或加密的 OLE 复合文档、空包、缺少 `[Content_Types].xml` 会**直接阻断**，给出「另存为未加密 .pptx」的建议，且不写入任何解压文件
  - 数字签名、VBA 宏、OLE 嵌入对象、修改密码 / 只读保护、缺少标准主部件会**明确告警**，显示在文件卡片与活动日志中
- **输出安全**
  - 同名结果文件自动追加序号，**绝不覆盖**已有文件
  - 先写临时文件、回读校验 ZIP 完整性、再原子替换
  - 解压有条目数 / 体积 / 压缩比 / 符号链接防护，图片解码有尺寸与内存上限
  - 候选文件只写私有工作目录，串行裁决阶段才替换原件；取消时解压树尚未被改动，无需回滚
- **中文界面**
  - 左侧主导区：源文件 + 媒体清单（逐项开关、体积预估、处理状态与原因）
  - 右侧从属区：三档预设、质量滑块、兼容模式、高级设置折叠、进度与结果，底部显示 FFmpeg 来源与构建许可
  - 主操作按钮固定在右下角，空 / 解析中 / 就绪 / 压缩中 / 完成 / 失败六态均有明确反馈
  - 活动日志显示跳过与失败的具体原因；预设、输出目录、高级项展开状态与手动指定的 FFmpeg 路径退出后自动保存

> 体积「预估」为启发式估算，仅用于调参参考，实际结果以压缩完成后的数字为准。

## FFmpeg 从哪来

程序启动时按以下顺序查找，找不到时右下角「环境说明」会给出安装指引和「选择本机 FFmpeg…」按钮：

1. 你在界面里手动指定并保存的路径
2. 随包副本（仅当维护者用合规构建重新打包）
3. 用户数据目录 `pptx-compressor/ffmpeg/`
4. 系统 `PATH`

| 平台 | 准备方式 |
| --- | --- |
| macOS | `brew install ffmpeg`，或下载官方构建后解压到 PATH |
| Windows | 下载官方构建或 [BtbN LGPL 构建](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest)，把 `ffmpeg.exe` 放进 PATH 或记住路径 |

用户自行安装的 FFmpeg 由用户按其许可证使用；本项目不分发、不下载该二进制。完整说明（含维护者如何重新打包合规的随包版本）见 [FFMPEG_SETUP.md](FFMPEG_SETUP.md)。

## 界面

界面按 Ardot UI 设计技能的 Web App 规范重设计：单一主导区、渐进披露、状态可见、一屏一主操作，纯白底 + 单一强调色，去掉等权卡片堆叠与毛玻璃。系统字体优先使用苹方（macOS）/ 微软雅黑（Windows）/ Noto Sans CJK（Linux）；找不到中文字体时标题栏会明确提示，而不是静默显示方块。

输入安全检测、解包与压缩都在后台线程执行，阻断时窗口直接进入失败态并说明原因，超大文稿也不会冻结窗口；取消会终止 FFmpeg 子进程并重新从源文件解压，避免基于已损失画质的数据二次压缩。

## 使用方式

从 [Releases](https://github.com/gaowanlong/pptx-compressor/releases) 下载对应平台的压缩包，解压后直接运行：

- **macOS**：打开 `PPTX Compressor.app`（若 Gatekeeper 拦截，前往 系统设置 → 隐私与安全性 → 仍要打开）
- **Windows**：运行 `pptx-compressor.exe`

把 `.pptx` 拖入窗口或点击「选择 PPTX 文件」，确认右侧方案后点击右下角「开始压缩」。输出默认写到源文件同目录，命名为 `<原名>_compressed.pptx`；若已存在则改为 `_compressed_2.pptx`，以此类推。原文件永远不会被修改。压缩参数、输出目录与 FFmpeg 路径由 eframe 写入各平台标准应用数据目录，下次启动自动恢复。

## 版本历史

- **v1.2.1**：新增输入安全检测，解压前阻断加密 / IRM / OLE / 结构损坏包，对签名、宏、嵌入对象与修改保护给出明确告警，并补齐相应界面与回归测试。
- **v1.2.0**：彻底移除不可再分发的 GPL+nonfree FFmpeg（默认不内置、不自动下载），提供用户自备路径、许可证门禁与合规构建脚本；支持 libx264 / libopenh264 双编码器；媒体压缩改为「并行生成候选 + 串行裁决」。
- **v1.1.0**：落地全新中文双区界面、解包后台化、设置持久化、中文字体加载、活动日志与逐项原因展示；后台 44 处错误与日志文案中文化；按 clippy 建议清理代码质量问题。
- **v1.0.1**：修复压缩变大不回滚、视频容器与 OOXML 声明不一致、缩略图悬空关系、GIF 调色板错色、FFmpeg 下载后找不到、输出非原子写、取消无法中断编码等正确性问题。

完整清单见 [CHANGELOG.md](CHANGELOG.md)，Release 说明直接取自对应章节。

## 已知限制

- 加密 / IRM 保护、旧版 `.ppt`、空包与缺少 `[Content_Types].xml` 的文件会被输入检测直接阻断；带数字签名、VBA 宏、OLE 嵌入对象或修改保护的文稿仍可压缩，但需按界面提示先另存副本并在 PowerPoint 中验证。
- 一次只处理一个文件，暂不支持批量队列；图片并行上限为 4，视频转码仍串行。
- JPEG 重编码不应用 EXIF 方向、不保留 ICC / 文本元数据；GIF 循环次数统一写为无限循环。
- 体积「预估」是启发式估算，不代表真实结果。
- 视频压缩依赖用户自备 FFmpeg；本机未安装时，真实转码端到端用例会在测试中明确跳过，视频链路由 mock 用例与参数单测覆盖。
- **v1.0.0 / v1.0.1 / v1.1.0 的历史 Release 资产仍附带 GPL+nonfree FFmpeg**，需维护者在 GitHub 手动删除对应附件；当前仓库与 v1.2.0 起的新包已不含该二进制。
- 已完成无窗口六态布局渲染冒烟与 macOS 真实窗口启动冒烟；中文字体显示与像素级视觉回归仍需在目标系统目视确认。

## 开发

### 环境要求

- Rust 1.74+（已在 1.96 验证）
- 对应平台工具链；Windows 交叉编译需 `cargo-xwin` 与 `x86_64-pc-windows-msvc` target
- 跑真实视频转码用例需要本机 FFmpeg（带 `libx264` 或 `libopenh264`）

### 测试与编译

```bash
cargo check --all-targets
cargo test                       # 单元测试、ZIP 往返、输入安全检测、mock FFmpeg 视频链路、并行一致性
cargo test -- --include-ignored  # 装了 FFmpeg 时额外跑真实转码端到端用例
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

**默认产物不含 FFmpeg**，只附带 `FFMPEG_SETUP.md`。若要恢复开箱即用的视频压缩，必须显式给出 LGPL 合规二进制，脚本会强制执行许可证门禁：

```bash
./scripts/fetch-ffmpeg-lgpl.sh windows                       # 下载 BtbN win64 LGPL 构建
FFMPEG_WINDOWS=build/compliant/ffmpeg.exe ./package.sh windows

./scripts/build-ffmpeg-lgpl-macos.sh                        # macOS 从源码构建（libopenh264）
FFMPEG_MACOS=build/compliant/ffmpeg ./package.sh macos
```

单独校验任意二进制：`./scripts/ffmpeg-license-check.sh /path/to/ffmpeg`。发布包会附带 README、CHANGELOG、`FFMPEG_SETUP.md` 与第三方许可说明。macOS → Windows 交叉编译也可手动执行：

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
```

## 技术栈

- **GUI**：eframe + egui（纯 Rust 即时模式 GUI），`persistence` 保存偏好
- **图片 / GIF**：`image` crate、`gif` crate
- **视频**：用户自备的 FFmpeg 子进程（自动选择 libx264 CRF 或 libopenh264 码率模型）
- **并行**：`rayon` 受限线程池（最多 4 线程）
- **PPTX / ZIP**：`zip` crate
- **偏好序列化**：`serde` / `serde_json`
- **构建**：cargo-xwin（macOS → Windows 交叉编译）

## 开源协议与第三方组件

本项目**源码**基于 MIT 协议开源。v1.2.0 起的发布包不再包含 FFmpeg 二进制，因此不再捆绑 GPL / nonfree 组件；用户自行准备的 FFmpeg 及其许可证由使用者自行负责。Rust 依赖与各平台合规说明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 与 [FFMPEG_SETUP.md](FFMPEG_SETUP.md)。
