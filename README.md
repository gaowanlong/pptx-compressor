# PPTX 压缩器

![Version](https://img.shields.io/badge/version-v1.2.5-blue.svg)
![macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)
![Windows](https://img.shields.io/badge/platform-Windows-0078d7.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

一个跨平台的 PowerPoint（PPTX）压缩工具：把 PPTX 当 ZIP 解开，重新编码内嵌的图片、GIF 与视频，再重新打包。提供 macOS 原生 `.app` 与 Windows 原生 exe，纯 Rust + egui 实现，界面为全中文。

> **v1.2.0 重要变更**：发布包**不再内置 FFmpeg**，也不会自动下载。图片 / GIF / XML 压缩不受影响；视频压缩需要你本机提供一个 FFmpeg。原因与做法见 [FFMPEG_SETUP.md](FFMPEG_SETUP.md)。

## 功能

- **批量队列**（v1.2.2 新增）
  - 一次选择 / 拖入多个 PPTX，界面按文件显示队列、状态徽章、单文件结果（原始 → 输出）与输入告警
  - 逐个独立解包、压缩、重打包，文件级总进度会补入当前文件的媒体进度；任一文件加密 / 损坏 / 打包失败只标记它自己，其余文件照常输出
  - 支持「添加文件」「清空队列」与逐项「移除」；取消停止后续文件，已生成的输出保留，源文件永不改动
  - 批量模式不预先解包，压缩前不展开媒体清单；需要逐项开关或媒体构成信息时请仍用单文件模式
- **图片压缩**
  - JPEG：可调画质（0–100），可选最大宽度缩放，统一输出 3 通道 RGB
  - PNG：Best 级别重编码 + 自适应滤波
  - GIF：可调帧率、调色板优化，保留原循环次数、透明索引与帧处置方式
  - 所有媒体统一遵循「只有更小才替换」，变大或失败时自动保留原文件
  - 图片与 GIF 用最多 4 线程并行重编码，视频转码保持串行以免内存爆炸
- **元数据保真**（v1.2.3 新增）
  - JPEG：EXIF（含方向标签）、ICC 色彩配置、XMP 与 IPTC 段原样写回重编码结果，带方向的照片不会被转歪、带色彩配置的图片不偏色
  - PNG：文本块（`tEXt` / `zTXt` / `iTXt`）、色彩块（`sRGB` / `gAMA` / `cHRM` / `iCCP`）、`eXIf`、`tIME`、`pHYs` 原样保留
  - GIF：保留原循环次数（有限次数 / 无限 / 只播放一次），调色板优化时为透明索引预留专用槽位，透明区域不会被填色、实体像素也不会被挖成透明
  - 采用字节级搬运，不解释元数据内容；解析异常时降级为「不注入」，绝不产出损坏图片。缩放时丢弃已失效的 `pHYs`
- **视频压缩**（需自备 FFmpeg）
  - H.264 / AAC 编码；有 `libx264` 时画质滑杆映射为 CRF（界面显示真实 CRF），只有 `libopenh264` 的 LGPL 构建自动切换为码率模型
  - 可选分辨率缩放，`yuv420p` 保证 PowerPoint 可播放，H.264 level 交给编码器决定
  - 「兼容模式」开启时，非 MP4 视频转为 `.mp4` 并**同步更新全包 `[Content_Types].xml` 与 `.rels` 引用**（含 notesSlides / 母版 / 主题 / 自定义目录）；关闭时跳过非 MP4 视频
  - 找不到 FFmpeg 或缺少 H.264 编码器时，视频项明确报错并写入活动日志，绝不静默跳过
- **OOXML 引用完整性**（v1.2.4 新增）
  - 媒体改名按 `Target` / `PartName` 的**文件名精确匹配**改写，不再做全局子串替换（不会误伤 `image10.mov`、`clip.mov.bak` 这类相似名字）
  - `[Content_Types].xml` 的 `<Override>` 不再假设 `/ppt/media/` 前缀：notesSlides、slideLayouts / slideMasters、handoutMaster、theme 与任意自定义目录下的引用都同步，并自动补齐缺失的 `<Default>` 扩展声明
  - 打包前清理**所有**指向不存在部件的 `.rels` 关系与失效 `<Override>`（不只是缩略图），避免 PowerPoint 提示「需要修复」；外部链接、有效 `rId` 一律保留，体检失败也不会阻断已压好的包
  - 放在 `ppt/media/` 之外的媒体同样参与压缩，重打包时按扩展名统一用 Stored，不再被二次 deflate
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
- **诚实的体积信息**（v1.2.5 新增）
  - 就绪态展示文稿真实大小、媒体总体积与按类型（JPEG / PNG / 动图 / 视频 / 其他）的构成，**不再显示按固定系数推算的「预计可节省 X%」**
  - 「可优化空间上限」= 被启用且可压缩的媒体原始体积之和，是「变小才替换」保证的数学上界；实际节省恒在 `[0, 上界]` 内，完成后才显示真实数字
  - 媒体清单待处理项只标「可优化 / 不处理」，已完成项显示真实节省百分比；质量滑杆不再让体积数字跳动
- **中文界面**
  - 左侧主导区：源文件 + 媒体清单（逐项开关、真实媒体构成与可优化空间、处理状态与原因）
  - 右侧从属区：三档预设、质量滑块、兼容模式、高级设置折叠、进度与结果，底部显示 FFmpeg 来源与构建许可
  - 主操作按钮固定在右下角，空 / 解析中 / 就绪 / 压缩中 / 完成 / 失败六态均有明确反馈
  - 活动日志显示跳过与失败的具体原因；预设、输出目录、高级项展开状态与手动指定的 FFmpeg 路径退出后自动保存

> 自 v1.2.5 起不再提供按系数推算的「预计可节省」：界面只展示真实媒体构成与**可优化空间上限**（「变小才替换」所保证的数学上界），实际节省在压缩完成后显示。

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

把 `.pptx` 拖入窗口或点击「选择 PPTX 文件（可多选）」：只选一个时走原来的「解析预览 → 逐项开关 → 开始压缩」流程；一次选多个或分多次拖入多个时进入批量队列，点「开始压缩 N 个文件」后逐个独立处理。队列里每个文件单独显示状态与结果，一个文件失败不会中断其余文件。

输出默认写到源文件同目录（也可在「高级设置」里指定统一输出目录），命名为 `<原名>_compressed.pptx`；若已存在则改为 `_compressed_2.pptx`，以此类推，不同目录下的同名源文件也不会互相覆盖。原文件永远不会被修改。压缩参数、输出目录与 FFmpeg 路径由 eframe 写入各平台标准应用数据目录，下次启动自动恢复。

## 版本历史

- **v1.2.5**：诚实的体积信息 —— 删除按媒体类型写死的全部启发式压缩比系数，改为基于真实媒体构成展示文稿体积、按类型构成与「可优化空间上限」（变小才替换保证的真实上界）；待处理项不再显示假的「≈ -N%」，压缩参数变化不再让体积数字跳动；新增 23 项回归测试，含从 egui 一帧输出提取文本断言界面不再出现「预计可节省」。
- **v1.2.4**：OOXML 引用完整性 —— 媒体改名改为按关系 / 内容类型标签精确定位（不再全局子串替换、不再假设 `/ppt/media/` 前缀），notesSlides / 母版 / 主题 / 自定义目录引用全量同步；打包前清理所有悬空关系与失效内容类型声明；非 `ppt/media` 路径的媒体也参与压缩并统一 Stored；新增 18 项回归测试。
- **v1.2.3**：元数据保真 —— JPEG 的 EXIF 方向 / ICC、PNG 的文本与色彩块、GIF 的循环次数与透明索引在重编码后原样保留，避免图片旋转、偏色、循环次数被改写与透明区域串色；新增 26 项回归测试。
- **v1.2.2**：新增批量队列，一次选择 / 拖入多个 PPTX 逐个独立处理，界面显示队列、单文件结果与文件级总进度，失败隔离；单文件路径行为不变。
- **v1.2.1**：新增输入安全检测，解压前阻断加密 / IRM / OLE / 结构损坏包，对签名、宏、嵌入对象与修改保护给出明确告警，并补齐相应界面与回归测试。
- **v1.2.0**：彻底移除不可再分发的 GPL+nonfree FFmpeg（默认不内置、不自动下载），提供用户自备路径、许可证门禁与合规构建脚本；支持 libx264 / libopenh264 双编码器；媒体压缩改为「并行生成候选 + 串行裁决」。
- **v1.1.0**：落地全新中文双区界面、解包后台化、设置持久化、中文字体加载、活动日志与逐项原因展示；后台 44 处错误与日志文案中文化；按 clippy 建议清理代码质量问题。
- **v1.0.1**：修复压缩变大不回滚、视频容器与 OOXML 声明不一致、缩略图悬空关系、GIF 调色板错色、FFmpeg 下载后找不到、输出非原子写、取消无法中断编码等正确性问题。

完整清单见 [CHANGELOG.md](CHANGELOG.md)，Release 说明直接取自对应章节。

## 已知限制

- 加密 / IRM 保护、旧版 `.ppt`、空包与缺少 `[Content_Types].xml` 的文件会被输入检测直接阻断；带数字签名、VBA 宏、OLE 嵌入对象或修改保护的文稿仍可压缩，但需按界面提示先另存副本并在 PowerPoint 中验证。
- 批量队列的文件之间仍串行处理（流水线内部对图片最多 4 线程并行、对视频转码串行），总耗时约为各文件之和；批量模式不预先展开媒体清单，也没有逐项开关与媒体构成信息。
- 元数据按字节段 / 块原样搬运，不解释其内容：缩放后 EXIF 里的像素尺寸与缩略图标签可能滞后（不影响方向判定）；PNG 的 `pHYs` 在缩放时会被丢弃；与调色板 / 色型绑定的 `tRNS`、`PLTE`、`bKGD`、`hIST`、`sBIT` 及 APNG 动画块不搬运，APNG 仍按静态图处理。
- 候选文件（含搬运回来的元数据）比原件大时会走「变小才替换」，此时保留原件，元数据自然完整。
- 引用完整性只校验 OPC 关系与 `[Content_Types].xml` 声明，不校验幻灯片 XML 正文里的 `r:embed` / `r:id` 是否指向存在的 `rId`（源包自带的这类损坏不会被自动修复）；改名后多余的旧 `<Default>` 扩展声明按设计保留。
- 本版有意不再预测压缩结果：可优化空间上限是真实上界（通常明显大于最终节省），确切数字在压缩完成后显示；构成模型按解压后媒体体积与 ZIP 包体积分别展示，不计算口径不同的「占文稿百分比」。
- 视频压缩依赖用户自备 FFmpeg；本机未安装时，真实转码端到端用例会在测试中明确跳过，视频链路由 mock 用例与参数单测覆盖。
- **v1.0.0 / v1.0.1 / v1.1.0 的历史 Release 资产仍附带 GPL+nonfree FFmpeg**，需维护者在 GitHub 手动删除对应附件；当前仓库与 v1.2.0 起的新包已不含该二进制。
- 已完成无窗口六态 / 批量队列四态布局渲染冒烟与 macOS 真实窗口启动冒烟；中文字体显示与像素级视觉回归仍需在目标系统目视确认。

## 开发

### 环境要求

- Rust 1.74+（已在 1.96 验证）
- 对应平台工具链；Windows 交叉编译需 `cargo-xwin` 与 `x86_64-pc-windows-msvc` target
- 跑真实视频转码用例需要本机 FFmpeg（带 `libx264` 或 `libopenh264`）

### 测试与编译

```bash
cargo check --all-targets
cargo test                       # 125 项：体积构成、OOXML 引用、元数据、队列模型、批量端到端、ZIP 往返、输入检测、mock FFmpeg、UI 冒烟
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
