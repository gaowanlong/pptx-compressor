# 更新日志

本项目所有值得注意的变更都记录在这里，版本号遵循语义化版本。Release 说明直接取自对应章节。

## [v1.2.4] - 2026-11-15

OOXML 引用完整性版。此前媒体改名只做「全局子串替换」，`clip.mov` 会误伤 `clip.mov.bak` / `image10.mov`；`[Content_Types].xml` 又硬编码 `/ppt/media/` 前缀，notesSlides、slideLayouts / masters、handoutMaster、theme 以及自定义目录下的媒体引用一律漏更新；删除媒体或缩略图后残留的悬空关系与失效内容类型声明，会让 PowerPoint 提示「需要修复」。同时，放在 `ppt/media/` 之外的媒体此前根本不参与压缩，重打包时还会被二次 deflate。本版把引用同步改成基于 OPC 关系模型的精确改写，并在打包前清理所有指向不存在部件的引用。纯补丁版本，无新依赖、无 UI 结构变化、无配置格式变化、无破坏性变更。

### 本轮主题选型说明

去重结果：待办第 3 项「输入安全检测」已在 v1.2.1、第 1 项「批量队列」已在 v1.2.2、第 2 项「元数据保真」已在 v1.2.3 完成，三个 tag 均存在。因此本轮取最高优先级的未完成项——第 4 项「OOXML 引用完整性」。这也正好闭环了 v1.2.2 / v1.2.3 两版「已知限制」里连续出现的同一句话：*「OOXML 引用同步目前只覆盖 `ppt/media` 下的媒体改名与缩略图清理，notesSlides / masters / handoutMaster 等引用尚未专项覆盖」*。相比体积预估、CLI、CI 等增强项，这是**输出正确性**问题，优先级更高。

为保证一轮内可完成、可验证，范围收敛为「引用精确定位 + 全包悬空关系清理 + 让非 `ppt/media` 媒体参与压缩」，没有叠加体积预估（待办 5）、无头 CLI（待办 6）、GitHub Actions CI（待办 7）等其余待办。版本按 patch 递增为 v1.2.4：用户无感知、偏好文件格式不变、单文件路径行为不变、无破坏性变更。

### 新增

- 新模块 `src/core/ooxml.rs`：**基于 OPC 关系模型的引用完整性维护**，主体是可独立测试的纯函数，只做带边界检查的字节扫描、不整体重写 XML。OOXML 的 `.rels` 与 `[Content_Types].xml` 由工具生成、结构受限，逐标签定位属性能原样保留命名空间、属性顺序与空白，无需引入 XML 解析依赖；畸形输入一律「跳过该标签」而不是改写整份文档。
  - `rewrite_rels_targets`：逐个 `<Relationship>` 解析 `Target`，按**文件名部分全等**匹配替换，保留 `../media/`、`/ppt/notesSlides/media/`、`media/` 等任意相对 / 绝对前缀；兼容单引号属性、`#fragment` 与反斜杠路径；`TargetMode="External"` 与绝对 URL 一律跳过。`RewriteReport` 返回改写数量。
  - `rewrite_content_types`：`<Override>` 的 `PartName` 同样按 basename 匹配，**不再假设 `/ppt/media/` 前缀**，并按新扩展修正 `ContentType`；缺失的 `<Default Extension=.../>` 自动补齐（即使包里只有 Default、没有任何 Override 也会补），按扩展去重且插在 `<Types …>` 开标签之内而非 XML 声明的 `?>` 之后。
  - `content_type_for_ext`：常见图片 / 视频 / 音频扩展的内容类型映射表；**未知扩展保留原声明、不猜测**，避免写入错误类型。
  - `resolve_relationship_path`：按 OPC 规则把 `Target` 解析成包内路径——`<part_dir>/_rels/<name>.rels` 相对 `<part_dir>`、根 `_rels/.rels` 相对包根、绝对 `/…` 相对包根；支持 `..` 词法规范化、`%20` 等百分号解码与 `#fragment` 剥离；外部链接与会逃出解压目录的路径返回 `None`，调用方保守保留。
  - `prune_dangling_relationships`：**遍历整个包**的 `.rels`（notesSlides / slideLayouts / slideMasters / handoutMasters / theme / 自定义目录全覆盖），只删除目标部件确实不存在的内部关系，并清理 `[Content_Types].xml` 里指向不存在部件的 `<Override>`；返回 `PruneReport`（扫描数、删除数、明细）供活动日志使用，具备幂等性。
- `src/core/extractor.rs`：
  - `rewrite_media_references` 改为调用 `core::ooxml`，删除旧的 `replace_content_type` / `fix_content_type`（硬编码 `/ppt/media/` 且一律写 `video/mp4`）与全局 `String::replace`。非 UTF-8 的 XML 部件跳过改写而不是让整条流水线失败。
  - `scan_media` 扩展为扫描 `ppt/` 树**其余目录**中的可压缩媒体（`ppt/notesSlides/media/`、`ppt/slideLayouts/media/`、自定义路径等）并去重；`ppt/media/` 下全部文件仍原样保留（含不可压缩的 Other，供界面展示），`_rels` 关系目录与 `docProps` 缩略图不会误纳入。
  - `repack_pptx` 的「已压缩媒体用 Stored」判断从路径前缀 `ppt/media/` 改为**按扩展名**：放在其他 part 目录下的媒体不再被二次 deflate。
- `src/core/pipeline.rs`：重打包前调用 `prune_dangling_relationships` 做收尾，清理结果以「引用完整性：已清理 N 条悬空关系、M 条失效内容类型声明（非致命）」写入活动日志；体检本身失败只记一条非致命日志，**绝不因为体检没做成而阻断已经压好的包**。

### 设计取舍

- **basename 精确匹配，不做全局子串替换**：旧实现 `updated.replace(old, new)` 会把 `image1.mov` 改成 `image10.mov` 里的子串、把 `clip.mov.bak` 改坏，也可能碰巧改到属性或 Id 里的同名文字。新实现只在 `Target` / `PartName` 的文件名部分做全等替换，前缀与其余属性字节原样保留。
- **保守清理优于激进修复**：只删「目标部件确实不存在」的内部关系；`TargetMode="External"`、绝对 URL、`mailto:`、盘符 / UNC、会逃出解压目录的 `../..`、纯 `#fragment`、非 UTF-8 的 `.rels` 全部保留。只要部件存在（或无法安全判断），对应的 `rId` 就原样不动，因此不会破坏任何有效引用。
- **不删「多余但合法」的 `<Default>`**：改名后旧的 `Default Extension="wmv"` 仍留在 CT 里。OOXML 允许声明未被使用的扩展，删它要额外扫描全包扩展名、收益低且有误删风险，本版保留。
- **保留 `strip_thumbnails` 独立逻辑**：它已精确处理根 `.rels` 与 CT 的缩略图引用；`prune` 作为兜底，覆盖「缩略图以外」以及 `strip_thumbnails` 没照顾到的其他 `.rels`，两者职责不重叠。
- **扫描范围扩大但单文件行为不变**：`ppt/media/` 的扫描结果、UI 媒体清单、用户已有偏好、单文件压缩流程全部沿用原逻辑；新增项只是「原来根本看不见的媒体」。既有 84 项测试零改动全部通过，回归面收敛到新增代码。
- **悬空清理放在打包前最后一步**：此时媒体替换、缩略图剥离、XML 注释清理都已完成，判定「部件是否存在」的结论才是最终状态；放在更早会因为后续删除产生新的悬空关系。
- **版本定为 patch**：无新依赖、无 UI 结构变化、无配置格式变化、无破坏性变更；引用同步对用户完全无感知，只是输出包更可靠。

### 修复

- 修正媒体改名时 `.rels` 全局子串替换误伤相似文件名（`clip.mov` ↔ `image10.mov`、`clip.mov.bak`）。
- 修正 `[Content_Types].xml` 只认 `/ppt/media/` 前缀，导致其他路径的 `<Override>` 的 PartName 与 ContentType 都不更新。
- 修正 notesSlides / slideLayouts / slideMasters / handoutMasters / theme 以及自定义 part 目录下的媒体引用漏同步。
- 修正删除媒体或缩略图后残留的悬空 `.rels` 关系与失效 `<Override>`，避免 PowerPoint 提示「需要修复」。
- 修正放在 `ppt/media/` 之外的媒体完全不参与压缩、且重打包时被二次 deflate（现在按扩展名统一 Stored）。
- 修正视频改名后若源包只用 `<Default Extension="mov">` 声明、没有 Override 时，新的 MP4 内容类型声明可能缺失的问题。

### 测试

- 新增 `tests/ooxml_tests.rs` **13 项纯函数测试**：内容类型映射与未知扩展拒绝、basename 精确替换且不误伤相似名、相对 / 绝对 / 单引号 / fragment / 反斜杠前缀保留、External 与不精确匹配原样保留、空 renames 与畸形 / 未闭合 XML 不改写、任意路径 Override 改写与 Default 插入位置、无 Override 也补 Default、已有 Default 不重复、多 rename 去重、未知扩展保留原声明、关系路径解析（相对 `_rels` / 根包 / 绝对 / 中文百分号解码 / fragment）、外部与逃逸路径保守拒绝。
- 新增 `tests/ooxml_e2e.rs` **5 项端到端测试**（真实临时文件与 ZIP，无需 GUI / FFmpeg）：7 类 part 的 `.rels` 与 3 条任意路径 Override 在改名后全部同步且相似文件名不被误伤；全包悬空清理只删 2 条悬空关系与 2 条失效 Override、notesSlides / layouts / masters / handoutMaster / theme 的合法关系全部保留且幂等；空目录 / 非 UTF-8 `.rels` / 逃逸与外部目标不被误删；完整流水线把 `ppt/notesSlides/media/photo.jpg` 真正压小、输出 ZIP 可回读、所有悬空引用清空、自定义路径媒体用 Stored；`repack_pptx` 对非 `ppt/media` 媒体同样 Stored。
- 共 **102 项测试全部通过**（`cargo test -- --include-ignored`）：84 项既有 + 18 项本轮新增。本机未安装 FFmpeg，依赖真实转码的用例按设计内部跳过并打印提示；视频链路由既有 mock 端到端与参数单测覆盖。

### 验证

- `cargo check --all-targets` 零警告；`cargo clippy --all-targets -- -D warnings` 零告警；`cargo test -- --include-ignored` 102 项全通过（15 个测试组，0 失败）。
- 端到端用例直接回读输出 ZIP：断言首条目为 `[Content_Types].xml`、`ppt/notesSlides/media/photo.jpg` 确实变小、其压缩方式为 `Stored`、所有 `.rels` 与 CT 中不再出现悬空目标、源文件未被改动。
- 两个发布包按 `package.sh macos` / `package.sh windows` 重新构建并校验（见 Release 资产）：macOS 为 Mach-O 64-bit arm64、Windows 为 PE32+ x86-64，Info.plist 的 `CFBundleShortVersionString` / `CFBundleVersion` 均为 1.2.4；两包内 4 份文档（README / CHANGELOG / THIRD_PARTY_NOTICES / FFMPEG_SETUP）与仓库逐字节一致；两包均不含任何 FFmpeg 二进制。
- `scripts/ffmpeg-license-check.sh` 门禁七情形实测：GPL / version3 / nonfree / 读不到配置串的 macOS 二进制，以及含 nonfree 配置的 `.exe` 全部被拒绝；干净的 LGPL 构建（含 `.exe`）放行。没有重新引入任何 GPL / nonfree 二进制。

### 已知限制

- 引用完整性只校验 **OPC 关系与内容类型声明**，不校验幻灯片 XML 正文里的 `r:embed` / `r:id` 是否指向一个存在的 `rId`——若源包本身就有这种损坏，工具不会替它补关系，但也不会让情况变坏。
- 改名后多余的 `<Default Extension="旧扩展">` 声明按设计保留（OOXML 合法且无害）。
- 体积「预估」仍是启发式估算（待办 5）；仍无无头 CLI（待办 6）、GitHub Actions CI（待办 7）、日志导出 / `--doctor`（待办 8）、像素级视觉回归（待办 10）、超大文件流式处理与 Unicode 路径边界补齐（待办 11）、发布包签名与 SHA256SUMS（待办 12）。
- 文件之间仍串行，图片并行上限固定为 4，视频转码仍串行（待办 9）；批量模式仍不预先展开媒体清单。
- 元数据按字节段 / 块原样搬运的限制沿用 v1.2.3（不物理旋转像素、`pHYs` 缩放时丢弃、APNG 按静态图处理）。
- v1.0.0 / v1.0.1 / v1.1.0 的历史 Release 附件仍附带 GPL+nonfree FFmpeg，需维护者在 GitHub 手动删除。
- 中文字体显示与像素级视觉回归仍需在目标系统目视确认（本版未改动界面结构）。

## [v1.2.3] - 2026-11-15

元数据保真版。此前图片重编码只写像素，源文件里的 EXIF 方向、ICC 色彩配置、PNG 文本与色彩块全部丢失，GIF 循环次数还被一律写成无限循环——带方向的手机照片压缩后会被转歪，带色彩配置的图片会偏色，有限次播放的动图会变成无限播放。本版把这些元数据原样搬运回重编码结果，并修掉调色板优化后透明索引与实体颜色串位的问题。纯补丁版本，无新依赖、无 UI 结构变化、无破坏性变更。

### 本轮主题选型说明

去重结果：待办第 3 项「输入安全检测」已在 v1.2.1 完成，第 1 项「批量队列」已在 v1.2.2 完成，且两个 tag 均已存在。因此本轮取最高优先级的未完成项——第 2 项「元数据保真」。这也正好闭环了 v1.2.0 / v1.2.1 / v1.2.2 三版「已知限制」里连续出现的同一句话：JPEG 不应用 EXIF 方向、不保留 ICC / 文本元数据，GIF 循环次数统一写为无限循环。为保证一轮内可完成、可验证，本轮只覆盖图片类媒体（JPEG / PNG / GIF），不叠加视频元数据、无头 CLI、CI、体积预估等其余待办。版本按 patch 递增为 v1.2.3，因为用户无感知、配置与偏好格式不变、无破坏性变更。

### 新增

- 新模块 `src/handlers/metadata.rs`：**纯函数的字节级元数据搬运**，不解释 EXIF / ICC / 文本块内容，因此不受编码器对各类元数据支持程度的限制，也不会漏掉编码器不认识的历史标签。所有解析都带边界检查，畸形输入返回空集合或 `None`，调用方降级为「不注入」，绝不产出损坏图片。
  - `jpeg_metadata_segments` / `inject_jpeg_segments`：按出现顺序收集 APP1（Exif / XMP）、APP2（ICC_PROFILE 分块）、APP13（Photoshop / IPTC），插回重编码 JPEG 的 SOI 之后、紧随 APP0(JFIF) 的位置；遇到 SOS 立即停止解析，避免把熵编码字节流误当成段。
  - `png_metadata_chunks` / `inject_png_chunks`：收集 `tEXt` / `zTXt` / `iTXt` / `iCCP` / `sRGB` / `gAMA` / `cHRM` / `tIME` / `eXIf` / `pHYs`，插回 IHDR 之后、PLTE / IDAT 之前（色彩块的规范要求），并按 PNG 规范重算 CRC-32。
  - `exif_orientation`：读取 EXIF TIFF 的 0x0112 方向标签，兼容大小端，用于诊断与回归断言。
- `src/handlers/image.rs`：JPEG / PNG 重编码后把源元数据段 / 块写回候选文件；解码改为从已读入的字节构造，尺寸与内存上限保持不变；缩放判断抽成单一函数 `should_resize`，供「是否保留 `pHYs`」复用，避免两处逻辑漂移。
- `src/handlers/gif_handler.rs`：
  - 用解码器的 `repeat()` 读取源循环次数并原样写回：`Finite(n)` 保留有限次数、`Infinite` 保留无限循环、没有 Netscape 扩展的 `Finite(0)` 表示只播放一次（编码器此时不写扩展），不再一律改成无限循环。
  - 调色板优化时新增 `reserve_transparent_slot`，为透明索引预留**专用槽位**；`find_closest_color` 映射实体颜色时排除该槽，`requantize_frame` 把透明像素直接指向该槽，`build_optimized_palette` 不再把透明像素计入配色（全透明帧也保证得到可用调色板）。

### 设计取舍

- **不物理旋转像素，而是原样保留 EXIF 方向标签**：`image` 0.25 的 `ImageReader::decode()` 不会自动应用 EXIF 方向，重编码也没有旋转像素，因此「像素 + orientation 标签」的配对与源文件完全一致，查看器显示效果不变。若改成物理旋转又保留标签，反而会导致支持 EXIF 的查看器二次旋转。
- **只搬运与像素无关的 PNG 块**：`tRNS` / `PLTE` / `bKGD` / `hIST` / `sBIT` 与调色板或色型绑定，重编码后色型可能变化，原样复制会让解码器报错；APNG 的 `acTL` / `fcTL` / `fdAT` 同样排除（本工具把 PNG 当静态图处理）。
- **APP0 不搬运**：编码器会写自己的 JFIF APP0，重复容易让解码器困惑；元数据段统一插在它之后，端到端用例已断言 APP0 仍在首位。
- **缩放时丢弃 `pHYs`**：图片尺寸变了，物理像素尺寸 / DPI 就失效，留着反而误导下游；EXIF 里的尺寸与缩略图标签同样可能滞后，但它们不影响方向判定，故保留。
- **透明专用槽而非「最近色重映射」**：旧逻辑把透明索引按颜色重映射到优化调色板，去重后透明色与实体色可能落到同一槽位，于是实体像素被挖成透明、或透明区域被填色。专用槽 + 排除映射保证两者互斥，回归用例用「两个索引同为黑色、其一为透明」专门覆盖这一场景。
- **失败降级与「变小才替换」**：注入失败或源数据畸形时保留干净的候选文件；搬运元数据会让候选略变大，若不再比原件小，流水线保留原件——此时元数据自然完整，不会出现「为了压缩而丢元数据」的取舍。
- **工程护栏**：`.gitignore` 增加 `.workbuddy/`，自动化记忆与当日日志是工作区本地文件，`git add -A` 不应把它们带进公开仓库。

### 修复

- 修正带 EXIF 方向的 JPEG 压缩后被转歪、带 ICC 色彩配置的图片偏色，以及 EXIF / XMP / IPTC 元数据整体丢失。
- 修正 PNG 文本块（作者、注释等）与 `sRGB` / `gAMA` / `cHRM` / `iCCP` 色彩块丢失导致的偏色与版权信息丢失。
- 修正 GIF 循环次数被统一改写为无限循环：有限次数动图与「只播放一次」的动图现在都保持原有播放语义。
- 修正 GIF 调色板优化后透明索引与实体颜色串位，避免透明区域被填色或实体像素被挖空。

### 测试

- 新增 `tests/metadata_tests.rs` **16 项纯函数测试**：JPEG 段收集白名单与 SOS 截断、畸形长度与填充字节、APP0 后插入位置与回读、无 APP0 插入、非 JPEG 拒绝、EXIF 大小端与缺标签、PNG 块白名单与逐类排除、缩放丢弃 `pHYs`、截断输入、IHDR 后插入与块顺序、CRC-32 规范值（`AE 42 60 82`）、非 PNG 拒绝。
- 新增 `tests/metadata_e2e.rs` **10 项端到端测试**（真实编码器生成媒体，无需 GUI / FFmpeg）：JPEG 的 `orientation=6` 与 ICC 字节在重编码后仍在且图片可解码、无元数据时不凭空添加、缩放后方向标签仍在；PNG 七类块逐一比对且注入后解码器仍能读、缩放丢弃 `pHYs`；GIF 有限次数 / 无限 / 无扩展三种循环语义、透明与实体同色时不串位；完整 PPTX 批处理链路产出有效 ZIP，包内 JPEG / PNG / GIF 的元数据均保留、源文件未改动、结果写入新文件。
- 共 **84 项测试全部通过**（`cargo test -- --include-ignored`）：10 项元数据端到端、16 项元数据纯函数、7 项批处理端到端、12 项队列模型、5 项流水线端到端、8 项核心逻辑、9 项输入检测、6 项 FFmpeg、5 项 UI 布局、6 项状态与偏好。本机未安装 FFmpeg，依赖真实转码的用例按设计内部跳过并打印提示；视频链路由 mock 端到端与参数单测覆盖。

### 验证

- `cargo check --all-targets` 零警告；`cargo clippy --all-targets -- -D warnings` 零告警；`cargo test -- --include-ignored` 84 项全通过。
- 端到端用例对注入后的 PNG 用真实解码器回读，确认 CRC 与块位置合法（不是「字节看起来对」而已）；对 JPEG 确认 APP0 仍在首位、尺寸不变、EXIF 方向标签可读。
- 两个发布包按 `package.sh macos` / `package.sh windows` 重新构建并校验（见 Release 资产）：结构、二进制架构、Info.plist 版本、包内文档一致性，以及不含任何 FFmpeg 二进制；`scripts/ffmpeg-license-check.sh` 门禁保持有效。

### 已知限制

- 元数据按字节段 / 块原样搬运、不解释内容：缩放后 EXIF 中的像素尺寸与缩略图标签可能滞后（不影响方向判定）；`pHYs` 在缩放时丢弃；`tRNS` / `PLTE` / `bKGD` / `hIST` / `sBIT` 与 APNG 动画块不搬运，APNG 仍按静态图处理。
- 不物理旋转像素，显示依赖查看器正确读取 EXIF 方向——这与原文件行为一致；若下游完全忽略 EXIF，观感与原件相同，而不是「自动摆正」。
- 体积「预估」仍是启发式估算（待办 5）；OOXML 引用同步仍只覆盖 `ppt/media` 下的媒体改名与缩略图清理，notesSlides / masters / handoutMaster 等引用尚未专项覆盖（待办 4）。
- 文件之间仍串行，图片并行上限固定为 4，视频转码仍串行；仍无无头 CLI（待办 6）与 GitHub Actions CI（待办 7）。
- v1.0.0 / v1.0.1 / v1.1.0 的历史 Release 附件仍附带 GPL+nonfree FFmpeg，需维护者在 GitHub 手动删除。
- 中文字体显示与像素级视觉回归仍需在目标系统目视确认（本版未改动界面结构）。

## [v1.2.2] - 2026-11-15

批量队列版。此前每轮只能处理一个文稿：`input_files` 虽是 `Vec` 却全程只取 `[0]`，多文件选择 / 拖入会静默丢弃其余文件。本版让「一次选择或拖入多个 PPTX」成为一等公民：逐个独立解包、压缩、打包，界面显示队列、单文件结果与文件级总进度，任一文件失败都被隔离，不影响其余文件。纯补丁版本，单文件路径行为完全不变，无破坏性变更。

### 本轮主题选型说明

待办清单按优先级推进：第 3 项「输入安全检测」已在 v1.2.1 完成，因此本轮取最高优先级的未完成项第 1 项「批量队列」。为保证一轮内可完成、可验证，本轮刻意把范围收敛为「逐个串行处理 + 队列展示 + 失败隔离」，没有叠加文件级并行、无头 CLI、CI 等其余待办；串行的理由见设计取舍。版本按 patch 递增为 v1.2.2，因为没有破坏性变更、单文件用户无感知。

### 新增

- 新模块 `src/core/queue.rs`：批量队列领域模型，与 GUI / 文件系统写入解耦。`JobStatus`（等待中 / 处理中 / 已完成 / 失败）、`BatchJob`（路径、体积、状态、错误、输入告警、输出路径、媒体数）、`BatchQueue`（去重构造、状态迁移、重排、移除重编号、汇总、进度）与 `BatchSummary`（成功 / 失败数、体积汇总、节省比例）。所有类型实现 `Serialize` / `Deserialize`，为后续无头 CLI 的结构化结果预留接口。
- 新模块 `src/core/batch.rs`：`run_batch` 在后台线程里逐个处理队列，定义 `BatchMessage`（JobStarted / JobProgress / JobLog / JobFinished / JobFailed / Finished / Cancelled）。每个文件使用独立临时解压目录与独立输出路径，复用现有 `run_pipeline`，无需改动压缩与 OOXML 引用同步逻辑；流水线消息由轻量转发线程实时翻译成带 index 的队列消息。
- **多选与多拖放**：文件对话框改为 `pick_files`，拖放接收全部 `dropped_files` 而非只取第一个；按扩展名过滤、按路径去重。合并后恰好 1 个文件仍走原有的「先解析预览再压缩」流程，2 个及以上进入批量队列。
- **队列视图**：左侧主导区按文件列出序号、名称、体积、状态徽章与单文件结果（成功显示「原始 → 输出」，失败直接显示原因，处理中显示媒体数）；输入安全告警按文件展示；支持「添加文件」「清空队列」与逐项「移除」，移除后自动重编号，队列退回 1 个文件时自动恢复单文件预览。
- **总进度与汇总**：文件级进度按「已完成文件数 / 总数」，并把当前文件内部的媒体进度按比例补进这一份（`combined_progress_pct`），避免长文稿上百分比长时间停滞；标题栏状态胶囊、进度条与底部文案在批量态切换为「已加入 N 个 PPTX」「正在逐个压缩」「批量完成：成功 X 个、失败 Y 个」等中文提示。完成页额外注明「体积仅统计成功文件」。
- **失败隔离**：解包被输入安全检测阻断、输出目录不可写、重打包失败等错误只把对应文件标记为 Failed，队列仍继续处理后续文件并以 `Finished` 正常收尾；全部失败才进入 Error 态。取消时停止后续文件，已生成的输出文件保留，源文件始终不改动。

### 设计取舍

- **文件之间串行，不做文件级并行**：流水线内部已对图片用最多 4 线程并行、对视频转码串行（FFmpeg 自身多线程）。再叠加文件级并行会成倍放大内存占用、让进度与活动日志乱序，且难以给出稳定的取消语义。逐个串行换来可预测的进度、清晰的失败隔离与复用的压缩逻辑，代价是批量总耗时约为各文件之和——对桌面工具而言可接受。
- **单文件路径零改动**：`queue` 为 `None` 时所有状态字段、解析流程、消息处理与 UI 分支保持 v1.2.1 的行为；批量分支通过新增字段与 match guard 叠加。这样既保住既有测试与用户习惯，也把回归面收敛到新增代码。
- **批量模式不预先解包**：多文件时每个文稿在开始后才解析，避免一次性把所有包解到磁盘；左侧媒体清单在批量态明确标注「不预先展开」，处理中的媒体信息通过 `JobStarted` / `JobProgress` 呈现在队列与进度文字里。
- **汇总只统计成功文件**：失败文件没有输出，若把其原始体积计入分母，「节省百分比」会凭空变大，属于误导性数字。界面在结果页显式说明这一口径。
- **输出命名沿用不覆盖策略**：批量复用抽到 `queue.rs` 的公共函数 `unique_output_path`，不同目录下同名源文件输出到同一目录时自动追加 `_2` / `_3`，端到端用例已覆盖。
- **越界索引静默忽略**：后台消息理论上可能乱序或过期，队列的所有状态变更方法对越界 index 不 panic、不误改其他任务。

### 修复

- 修正「多选 / 多拖放只处理第一个文件」的静默行为：此前 `load_pptx` 会强制 `input_files = vec![单个路径]`，多余文件被无声丢弃，现在全部进入队列并可见。
- `load_pptx` 与进入批量模式时都会递增 `load_generation`，丢弃仍在运行的旧解析线程结果，避免旧消息覆盖新队列状态。
- 批量取消后队列 `reset()` 回到等待态并保留文件列表，可直接重试；单文件取消仍沿用原有「重新载入源文件」逻辑，两者互不干扰。

### 测试

- 新增 `tests/queue_tests.rs` 12 项：去重与顺序、失败隔离、汇总只计成功、进度与终态边界、平滑总进度与钳制、reset、移除重编号、越界变更无副作用、输出路径不覆盖、first_output、汇总序列化、中文状态标签。
- 新增 `tests/batch_e2e.rs` 7 项（真实临时 PPTX，无需 FFmpeg / GUI）：3 个合法包全部产出有效输出、加密 / OLE 损坏包被隔离且队列仍收尾、同名源文件输出互不覆盖、统一输出目录生效、空队列立即收尾、取消不产生任何输出、把后台消息回填 `BatchQueue` 的契约一致性。
- `tests/ui_layout_tests.rs` 新增 2 项：批量队列在就绪 / 压缩中 / 完成 / 失败四态的混合任务渲染冒烟，以及全待处理 + 字体缺失 + FFmpeg 不可用 + 高级区展开的边界组合。
- 共 **58 项测试全部通过**（`cargo test -- --include-ignored`）：7 项批处理端到端、12 项队列模型、5 项流水线端到端、8 项核心逻辑、9 项输入检测、6 项 FFmpeg、5 项 UI 布局、6 项状态与偏好。本机未安装 FFmpeg，依赖真实转码的用例按设计内部跳过并打印提示；视频链路由 mock 端到端与参数单测覆盖。

### 验证

- `cargo check --all-targets` 零警告；`cargo clippy --all-targets -- -D warnings` 零告警；`cargo test -- --include-ignored` 58 项全通过。
- 批处理端到端用例直接对真实 ZIP 包验证输出可被 `ZipArchive` 回读且含 `[Content_Types].xml`，并断言源文件未被改动、失败项不产生输出。
- 两个发布包按 `package.sh macos` / `package.sh windows` 重新构建并校验（见 Release 资产）：结构、二进制架构、Info.plist 版本、包内文档一致性，以及不含任何 FFmpeg 二进制；`scripts/ffmpeg-license-check.sh` 门禁保持有效。

### 已知限制

- 文件之间仍串行；图片并行上限固定为 4，视频转码仍串行。
- 批量模式不预先展开媒体清单，压缩前也看不到每个文稿的媒体明细与体积预估；如需逐项开关，请仍用单文件模式。
- 体积「预估」仍是启发式估算；JPEG 不应用 EXIF 方向、不保留 ICC / 文本元数据，GIF 循环次数统一写为无限循环。
- OOXML 引用同步目前只覆盖 `ppt/media` 下的媒体改名与缩略图清理，notesSlides / masters / handoutMaster 等引用尚未专项覆盖。
- 仍无无头 CLI 与 GitHub Actions CI；`BatchQueue` / `BatchSummary` 已可序列化，为结构化输出打好了基础。
- v1.0.0 / v1.0.1 / v1.1.0 的历史 Release 附件仍附带 GPL+nonfree FFmpeg，需维护者在 GitHub 手动删除。
- 中文字体显示与像素级视觉回归仍需在目标系统目视确认（本版已做无窗口队列四态渲染冒烟）。

## [v1.2.1] - 2026-11-14

输入安全检测版。此前对「加密、带签名、带宏、带嵌入对象」的文稿只有一句静态提示，实际仍会尝试解包并重打包，可能生成看似成功却打不开的文件。本版在解压前先做一次只读检查，把风险分成「必须阻断」与「明确告警」两类。纯补丁版本，无破坏性变更。

### 新增

- 新模块 `src/core/inspection.rs`：只读取 ZIP 中央目录与最多 2 MiB 的 `ppt/presentation.xml`，不解压、不写磁盘即可完成判定；分类逻辑抽成纯函数 `classify`，便于回归测试。
- **阻断项**（拒绝解包并给出可操作建议，原文件与解压目录都不会被改动）：
  - 加密 / 信息权限保护（IRM）：`EncryptionInfo`、`EncryptedPackage`、`\u{6}DataSpaces/*`
  - OLE 复合文档魔数：旧版 `.ppt` 或用密码加密后改扩展名的文件，改报「这是旧版 .ppt 或加密的复合文档，不是可直接解压的 .pptx」，不再让用户看到笼统的 ZIP 解析错误
  - 空压缩包、缺少 `[Content_Types].xml`（重打包校验必然失败）
- **告警项**（允许继续，但显示在文件卡片与活动日志）：数字签名（压缩后失效）、VBA 宏、OLE 嵌入对象、`p:modifyVerifier` 修改密码 / 只读保护、缺少标准主部件 `ppt/presentation.xml`。
- `extractor` 新增 `extract_pptx_inspected`，在保留原 `extract_pptx` 签名（不破坏既有调用与测试）的同时把告警透传给界面；后台解包线程把告警随 `LoadMessage::Loaded` 回传。
- 界面：源文件区副标题改为「自动检测加密 / 签名 / 宏」；文件卡片下方最多直接展示两条黄色提示，其余提示写入活动日志；「移除文件 / 更换文件」会清空旧告警但保留用户偏好。

### 设计取舍

- 缺 `[Content_Types].xml` 设为阻断、缺 `ppt/presentation.xml` 只设为告警：前者会让输出包通不过现有的 ZIP 回读校验，必然损坏；后者可能只是非标准主部件路径，原样重打包仍可成功，误阻断风险更高。
- 签名、宏、嵌入对象不阻断：这些部件在压缩中会被原样复制，输出包本身有效。真正的后果是签名失效或宏 / OLE 需人工验证，因此以显著提示代替一刀切拒绝。
- 检测放在解压循环之前，阻断时解压目录保持为空；ZIP 炸弹 / 符号链接 / 4 GiB 上限等原有防护仍在其后按原顺序生效。

### 修复与测试

- 修正 `tests/pipeline_tests.rs` 的 ZIP 炸弹用例：过去包内只有 `ppt/media/big.bin`，加入输入检测后会先因结构缺失被拒，测不到压缩比防护。现在补齐 `[Content_Types].xml` 与 `ppt/presentation.xml`，并断言错误信息确实来自压缩比守卫。
- 新增 `tests/inspection_tests.rs` 9 项：正常包无提示、加密与 IRM、结构缺失分级、四类风险信号齐发、真实 ZIP 解包透传、加密包阻断前不写磁盘、OLE 复合文档可操作报错、告警随解包返回、合法骨架的 ZIP 炸弹仍被拦截。
- 新增界面回归 2 项：四条输入告警时文件面板正常渲染；`reset_for_new_file` 清空告警与日志但保留预设和输出目录。
- 共 37 项测试全部通过；`cargo check --all-targets` 零警告，`cargo clippy --all-targets -- -D warnings` 零告警。
- 在 macOS 上直接启动 release 二进制保持运行 47 秒无 panic、无错误输出（随后手动结束），验证窗口创建、中文字体加载与首帧渲染。

### 已知限制

- 检测基于文件名与主部件文本，不做密码学验证；刻意构造的伪装包仍可能绕过。
- 仍一次处理一个 PPTX；视频转码仍串行；JPEG 的 EXIF 方向 / ICC 与 GIF 循环次数尚未保留；体积预估仍是启发式。
- v1.0.0 / v1.0.1 / v1.1.0 的历史 Release 附件仍附带 GPL+nonfree FFmpeg，需维护者在 GitHub 手动删除。
- 中文字体显示与像素级视觉回归仍需在目标系统目视确认（已做无窗口六态 / 告警态渲染冒烟与 macOS 真实窗口 47 秒启动冒烟）。

## [v1.2.0] - 2026-11-14

合规整改与压缩并行化版。上一版两次「如实说明」的 FFmpeg 许可证风险在本版彻底闭环，同时把媒体压缩从全串行改为受限并行。

### 破坏性变更（升级前请读）

- **发布包默认不再内置 FFmpeg，也不再运行时自动下载二进制**。v1.1.0 及更早版本随包分发的 macOS / Windows FFmpeg 是 `--enable-gpl --enable-version3 --enable-nonfree` 构建，官方明确不允许再分发，与 MIT 源码捆绑提供存在法律风险；macOS 侧也不存在可直接再分发的 LGPL 预编译版本。因此仓库中这两个二进制（43 MB + 79 MB）已删除，`.gitignore` 同时屏蔽 `resources/ffmpeg/` 与 `build/`，防止再次误提交。
- 影响范围：**图片、GIF、XML 注释与缩略图压缩完全不受影响**；只有 PPTX 内嵌视频的重编码需要用户本机提供 FFmpeg。找不到时视频项会明确报错并在活动日志说明，不再静默跳过或悄悄联网下载。安装与指定方式见新增的 `FFMPEG_SETUP.md`。

### 新增

- **四级 FFmpeg 查找**：手动指定并持久化的路径 → 随包副本（仅当维护者用合规构建打包）→ 用户数据目录 → 系统 PATH。找不到时右下角「环境说明」给出平台安装指引，并提供「选择本机 FFmpeg…」「重新检测」「恢复自动查找」三个动作；选择时会先执行 `ffmpeg -version` 校验，路径随偏好保存（旧偏好文件仍可读取）。
- **许可证可见**：解析 `ffmpeg -version` 的 `configuration` 行，把构建识别为 LGPL / GPL / nonfree / 未识别并在界面标注。用户自行安装的 GPL 构建供个人使用不构成再分发，运行时不做拦截；拦截只发生在打包阶段。
- **打包许可证门禁**：新增 `scripts/ffmpeg-license-check.sh`，`package.sh` 只有在显式传入 `FFMPEG_MACOS` / `FFMPEG_WINDOWS` 时才会内置 FFmpeg，且必须通过该门禁；任何 `--enable-gpl` / `--enable-version3` / `--enable-nonfree` 或读不到构建配置的二进制都会被拒绝。Windows `.exe` 无法在 macOS 上执行，改从二进制中提取编译期配置字符串判定。
- **合规构建路径**：新增 `scripts/fetch-ffmpeg-lgpl.sh`（Windows 下载 BtbN 的 win64 LGPL 静态构建并自动门禁校验）与 `scripts/build-ffmpeg-lgpl-macos.sh`（macOS 从源码构建，用 BSD 许可的 libopenh264 替代 GPL 的 libx264）。两者都只生成到 `build/compliant/`，必须通过环境变量显式指定才会打包。
- **双 H.264 编码器自适应**：运行时探测 `libx264` / `libopenh264`。有 libx264 时沿用 CRF + `medium` preset + High profile；LGPL 构建只有 libopenh264 时自动切换为码率模型（`-b:v`，按质量滑杆映射），并去掉 x264 专有参数；两者都没有时给出明确中文错误。命令行构造抽成纯函数 `build_video_args`，无 FFmpeg 也能回归测试。
- **压缩并行化**：流水线拆成「并行生成候选 / 串行裁决安装」两阶段。图片与 GIF 用最多 4 线程的受限线程池并行重编码，视频转码保持串行（FFmpeg 自身已多线程，并发多进程会打爆内存）。handler 仍只写私有工作目录，只有串行阶段会改动解压树，因此「变小才替换」「失败回滚」「OOXML 引用同步」「日志顺序稳定」全部保持，且取消时无需逐文件回滚。

### 修复

- 去掉 `download_ffmpeg` 及其整套 curl / 解压 / 进度轮询逻辑：此前开发环境会自动下载 GPL 二进制到用户数据目录，既是合规风险也带来无声的网络行为。
- 修复 v1.0.1 起「FFmpeg 下载完成却找不到」问题的根因场景（现在根本没有下载这一步），并把查找候选路径与来源标签统一到一处。
- 视频压缩失败时不再出现英文错误；所有面向用户的提示中文化。
- 并行阶段的进度消息用原子计数合并发送，最终状态仍按媒体索引升序，避免线程调度导致界面清单跳动。

### 工程与文档

- `Cargo.toml` 升为 1.2.0，新增 `rayon`（仅用于受限并行）；`package.sh` 默认产物不再含 FFmpeg，改为随包附带 `FFMPEG_SETUP.md`。
- 重写 `THIRD_PARTY_NOTICES.md`（不再声明随包二进制，改为说明用户自备与维护者合规打包流程）、重写 README 的 FFmpeg / 合规 / 测试章节，新增 `FFMPEG_SETUP.md` 与本 CHANGELOG 章节。
- 新增 `tests/ffmpeg_tests.rs` 6 项（许可证判定、编码器回退、x264 CRF 参数、openh264 码率参数且不残留 x264 专有参数、非法编码器拒绝、缩放滤镜在两种编码器下均生效）。
- `tests/pipeline_e2e.rs` 新增「mock FFmpeg」端到端用例：用本地脚本模拟 `-version` / `-encoders` / 编码调用，在没有真实 FFmpeg 的机器上完整验证 `.mov → .mp4` 改名、Content_Types 与 `.rels` 同步；另新增并行一致性用例（6 个 JPEG，验证最终状态按索引升序、进度覆盖全部任务、变小的替换 / 变大的保留原件）。真实 FFmpeg 用例改为使用系统 FFmpeg，未安装时明确跳过并打印提示，不再伪装成通过。
- `tests/ui_state_tests.rs` 补充 `ffmpeg_override` 的往返序列化与旧偏好文件兼容断言。

### 验证

- `cargo check --all-targets` 零警告；`cargo clippy --all-targets -- -D warnings` 零告警。
- **26 项测试全部通过**（`cargo test -- --include-ignored`）：6 项 FFmpeg 合规 / 参数、5 项流水线端到端（含 mock 视频链路与并行一致性）、8 项核心逻辑、2 项无窗口六态渲染、5 项状态与偏好。
- 许可证门禁实测四种情形：macOS GPL+nonfree 拒绝、Windows GPL 拒绝、伪造无配置二进制拒绝、构造的 LGPL 配置通过。
- 本机未安装 FFmpeg，因此依赖真实转码的用例按设计跳过；视频链路改由 mock 端到端用例与参数单测覆盖。在装有 FFmpeg 的机器上执行 `cargo test -- --include-ignored` 会额外跑真实转码。
- 在 macOS 上直接启动 v1.2.0 release 二进制保持运行 49 秒无 panic、无错误输出（随后手动结束），验证移除随包 FFmpeg 后窗口创建、中文字体加载与首帧渲染仍正常。
- 两个发布包均校验过结构与架构：包内不含任何 FFmpeg 二进制，附带 `FFMPEG_SETUP.md` 与完整文档，macOS Info.plist 版本为 1.2.0。

### 已知限制（未在本版解决）

- **历史 Release 资产**：v1.0.0 / v1.0.1 / v1.1.0 的已发布二进制仍附带 GPL+nonfree FFmpeg。清理这些资产属于对外部已发布内容的操作，本版未擅自改动，需维护者在 GitHub Release 页面手动删除对应附件或替换为合规包。
- 一次仍只处理一个 PPTX；视频转码保持串行，图片并行上限固定为 4。
- 体积「预估」仍是启发式估算；JPEG 不应用 EXIF 方向、不保留 ICC / 文本元数据，GIF 循环次数统一写为无限循环。
- 仅支持未加密 `.pptx`；宏文稿与带数字签名的文稿压缩后签名失效。
- 若维护者重新内置 LGPL FFmpeg，还需随包附上对应 LGPL 许可文本、FFmpeg 源码获取方式与第三方库许可信息；`scripts/` 只负责生成合规二进制与门禁，不代替许可证义务。
- 中文字体显示与像素级视觉回归仍需在目标系统目视确认（已做无窗口六态渲染与 macOS 真实窗口启动冒烟）。

## [v1.1.0] - 2026-11-14

界面重设计落地版。上一版留下的「界面稿未合入 GUI、解包阻塞窗口、设置不持久化、界面未中文化」四项全部完成，并顺手清理后台文案与代码质量问题。

### 新增

- **全新中文界面（egui）落地**：把 v1.0.1 交付的 HTML 高保真设计稿真正写进 GUI 代码。按单一主导区原则重排为「左侧文件 + 媒体清单 / 右侧方案 + 结果」双区，去掉四张等权毛玻璃卡片与整条蓝色 header；纯白底、细分隔线、单一强调色、微软雅黑 / 苹方系统字体。
- **一屏一主操作**：主按钮固定在右下角，位置与尺寸在空、解析中、就绪、压缩中、完成、失败六态下保持不变；取消、重试、打开结果、复制路径均为次要动作。
- **状态可见**：标题栏状态胶囊（含压缩百分比）、FFmpeg 就绪状态、逐项媒体状态（待处理 / 处理中 / 已完成 / 保留原件 / 失败）、失败与跳过原因、体积对比、活动日志全部可见；预设改为有明确选中态的分段控件，高风险参数收进「高级设置」折叠区。
- **解包后台化**：PPTX 解压移到后台线程，GUI 通过消息通道接收结果，超大文稿不再冻结窗口；引入「世代编号」丢弃过期结果，用户中途换文件时旧线程不会写回状态，临时目录自动清理。
- **设置持久化**：预设、质量滑块、开关、输出目录、高级区展开状态在退出时写入 eframe 标准存储，启动时恢复；`serde(default)` 保证旧偏好文件与未来新增字段向后兼容，损坏文件只会打印日志不会崩溃。
- **中文字体加载**：按平台加载苹方 / 微软雅黑 / Noto Sans CJK，显式指定 TTC 的 face index；找不到字体时标题栏显示「缺少中文字体」，不再静默显示方块。
- **诚实反馈**：视频质量显示真实 CRF；预估标注「按当前参数估算，实际结果以压缩完成为准」；界面明确写出「原文件不改动、同名自动追加序号、变大的媒体自动保留原件、输出前回读校验」；FFmpeg 来源（随包 / 系统 PATH / 用户数据目录）与 GPL + nonfree 合规风险在右栏底部如实告知。
- **测试**：新增 `tests/ui_state_tests.rs` 5 项（预设来回切换不残留缩放开关、CRF 映射、偏好往返序列化、旧偏好文件兼容、中文状态与标签）与 `tests/ui_layout_tests.rs` 2 项（六态面板渲染冒烟、空清单 / 高级区 / 字体缺失场景渲染）。

### 修复

- **预设切换残留缩放**：`preset_high` / `preset_medium` 现在会重置「图片最大宽度」「视频最大高度」开关；此前从「最小体积」切回「均衡」后分段控件显示均衡、实际仍按 1024px / 720p 压缩。
- **取消后日志被清空**：取消消息处理改为先重新载入源文件、再写入提示，避免「已取消」说明被加载流程的日志清空动作吞掉；同时保留用户偏好而不重置设置。
- **后台文案中文化**：pipeline、extractor、image / GIF / video handler 与 FFmpeg 下载共 44 处错误与日志文案改为中文，中文界面不再夹杂英文报错；FFmpeg 原始 stderr 仍保留原文以便排查。
- **代码质量**：按 `cargo clippy -D warnings` 清理 15 项告警（排序改用 `sort_by_key`、`io::Error::other`、`is_some_and`、`clamp`、`while let`、管道参数改为切片、去掉多余引用与可折叠 if 等），并让轮询计数不依赖 1.87 才稳定的新 API，维持 MSRV 1.74。

### 工程与文档

- `Cargo.toml` 版本升为 1.1.0，新增 `serde` / `serde_json`（均在既有 lock 中）；`main.rs` 主题、窗口尺寸（1080×740，最小 900×600）与标题同步更新。
- `package.sh` 新增 `FFMPEG_MACOS` / `FFMPEG_WINDOWS` 环境变量，可直接用自备的 LGPL 合规 FFmpeg 覆盖默认二进制；`.app` 的 `CFBundleDisplayName` 改为中文「PPTX 压缩器」。
- 重写 README（界面、持久化、合规替换、测试命令）、更新 `THIRD_PARTY_NOTICES.md`（合规替换步骤与查找顺序）、本 CHANGELOG 新增本章。

### 验证

- `cargo check --all-targets` 零警告通过；`cargo clippy --all-targets -- -D warnings` 通过。
- 共 18 项测试全部通过（原 11 项 + 新增 7 项），含 `cargo test -- --include-ignored` 触发的真实 FFmpeg `.mov → .mp4` 端到端用例；`cargo clippy --all-targets -- -D warnings` 零告警。
- 在 macOS 上直接启动 release 二进制保持运行 17 秒无 panic、无错误输出（随后手动结束），验证窗口创建、中文字体加载与首帧渲染。
- macOS 与 Windows 发布包均重新构建并校验内容（见 Release 资产）。

### 已知限制（未在本版解决）

- 一次仍只处理一个 PPTX；媒体压缩仍为串行，未做并行化。
- 体积「预估」仍是启发式估算；JPEG 不应用 EXIF 方向、不保留 ICC / 文本元数据，GIF 循环次数统一写为无限循环。
- 仅支持未加密 `.pptx`；宏文稿与带数字签名的文稿压缩后签名失效。
- **内置 FFmpeg 仍是 GPLv3 + nonfree 构建**，本版只提供了环境变量替换路径与界面合规提示，尚未完成许可证整改；再分发前请自行替换为 LGPL 合规构建。
- 已完成无窗口六态布局渲染冒烟与 macOS 真实窗口启动冒烟，但未做像素级视觉回归；中文字体显示效果仍需在目标系统目视确认。

## [v1.0.1] - 2026-11-14

修复版。重点是解决 v1.0.0 中可能影响输出 PPTX 正确性与数据安全的问题，并补齐回归测试、构建脚本与第三方许可说明。

### 修复

- **压缩结果变大不再污染输出**：所有 handler 改为只生成候选文件，由 pipeline 统一裁决「只有更小才替换」；替换前先备份原文件，任何失败都能回滚。此前 JPEG / GIF / 视频即使变大也会被写回，界面却显示「已跳过」。
- **视频格式与 OOXML 声明保持一致**：非 MP4 视频转码后改名为 `.mp4`，并同步更新 `[Content_Types].xml` 与所有 `.rels` 的引用；关闭「兼容模式」时不再处理非 MP4 视频，杜绝「内容是 MP4、文件名和声明却是 WMV」的情况。
- **剥离缩略图不再留下悬空关系**：删除 `docProps/thumbnail.*` 的同时清理 `_rels/.rels` 中的 Relationship 与 `[Content_Types].xml` 中的 Override，避免 PowerPoint 提示修复文稿。
- **GIF 重新量化修复**：调色板统计与像素映射统一使用「帧局部表优先、全局表兜底」，透明色索引同步重映射，重建后清除帧局部表，保留原始 dispose 与帧延迟（仅真正抽帧时改写 delay），修复颜色错乱与坏帧；编码失败时清理候选文件。
- **FFmpeg 自动下载路径修复**：把下载目录加入查找候选，解决「下载完成却找不到二进制」；未打包 FFmpeg 的开发环境也能正常压缩视频。
- **输出文件原子写入**：先写同目录临时文件，回读校验 ZIP 与 `[Content_Types].xml` 后再重命名；同名输出自动追加序号，不再静默覆盖；中途失败或磁盘写满不会留下损坏的 `.pptx`。
- **取消与重试语义**：取消可轮询并终止 FFmpeg 子进程；取消或失败后丢弃被修改的临时目录，重新从源文件解压，避免二次压缩基于已损失画质的数据，也避免统计失真。
- **修正预设与质量刻度**：Balanced 预设由 23（实际 CRF 40）改回 45（CRF 29），CRF 计算收敛到 `crf_from_quality` 单一函数，界面显示真实 CRF 值。
- **尊重用户选择**：逐项启用/禁用开关真正生效（此前 H.264 开关会强制处理所有视频）；文件选择改为单选，避免「可多选但只压第一个」的误导；PPTX 扩展名大小写不敏感。
- **错误可见**：非致命错误进入「Activity log」（最多 50 条），XML 清理、缩略图清理失败不再被静默吞掉；错误与取消分别处理。
- **XML 注释剥离**：遇到未闭合的 `<!--` 时原样保留剩余内容，不再截断 XML part 导致文稿损坏。
- **解压与解码安全**：增加条目数上限（20,000）、累计解压体积上限（4 GiB）、单条目压缩比上限（200）与符号链接条目检查；图片解码限制最大尺寸（20000px）与内存（256 MiB）；缩放后高度最小为 1px。
- **视频编码参数**：不再对 4K 素材强制声明 H.264 Level 4.0，level 交由 x264 自动选择；JPEG 统一输出 3 通道 RGB，避免 PowerPoint 无法渲染 4 通道 JPEG。
- **界面细节**：版本号改为读取 `CARGO_PKG_VERSION`（此前硬编码 v0.2）、预设选中态可见、滑块数值不再滞后一帧、进度不再显示 `(N+1)/N`（界面提示新增一轮修复说明）。

### 工程与文档

- `main.rs` 改用库 crate，消除模块重复编译与重复的 dead-code 警告；删除 `scanner.rs`、`content_types.rs` 死代码，移除未使用的 `rayon` / `quick-xml` / `which` 依赖。
- 重打包时 `Stored` 白名单补全 `avi / mpg / mpeg / asf`，并跳过临时日志与系统垃圾文件。
- 新增 11 项回归测试：`tests/pipeline_tests.rs`（CRF 与预设、变大不替换判定、XML 注释含未闭合、OOXML 引用重写、缩略图关系清理、ZIP 解包炸弹拦截、打包往返校验）与 `tests/pipeline_e2e.rs`（真实 JPEG 的变小替换 / 变大回滚、真实 FFmpeg 视频转码改名与声明同步；后者需内置 FFmpeg，默认 `#[ignore]`）。全部在 macOS 本地通过。
- `package.sh` 现在自动创建完整的 macOS `.app` 骨架（Info.plist、图标、Resources）并校验 FFmpeg 是否存在，干净克隆可直接打包；分发包附带 README、CHANGELOG 与第三方许可说明。
- 新增 `THIRD_PARTY_NOTICES.md`，说明随包分发的 FFmpeg 二进制版本、构建配置、许可证与源码获取方式；README 与实现逐条对齐，并列出已知限制。
- 版本号统一为 v1.0.1，`Cargo.lock` 同步。

### 已知限制（未在本版解决）

- 暂不支持加密 / 带密码的 PPTX；`.pptm` 宏文稿与带数字签名的文稿在内容变更后签名会失效，建议先另存副本。
- 一次只处理一个 PPTX；列表中的「预估体积」是启发式估算，不代表真实结果。
- JPEG 重编码不应用 EXIF 方向、不保留 ICC 与文本元数据；GIF 循环次数统一写为无限循环。
- 解包与重新解压仍在 GUI 线程同步执行，超大文稿会短暂卡顿；媒体压缩仍为串行。
- 设置持久化、界面中文化与全新的双区界面设计仍停留在设计稿阶段，未合入本版 GUI 代码。
- 内置 FFmpeg 二进制为 `--enable-gpl --enable-version3 --enable-nonfree` 构建，再分发存在合规风险，建议后续替换为 LGPL 合规构建（见 `THIRD_PARTY_NOTICES.md`）。

## [v1.0.0]（历史版本）

首个正式版：图片 / GIF / 视频重压缩、Apple 风格 GUI、macOS 与 Windows 双平台打包。具体发布时间见 GitHub Releases。
