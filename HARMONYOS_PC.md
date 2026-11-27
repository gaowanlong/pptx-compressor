# 鸿蒙电脑（HarmonyOS PC）版本适配说明

适用版本：v1.3.0 ｜ 目标设备：HarmonyOS 5+/6 的 2in1 / 鸿蒙电脑（MateBook Pro、擎云 HM 系列等）
构建验证环境：DevEco Studio 6.1.1（SDK API 24，`compatibleSdkVersion = 5.0.0(12)`）、Rust 1.96、OHOS NDK、`aarch64-unknown-linux-ohos` 与 `x86_64-unknown-linux-ohos`。

---

## 0. 先说结论与边界

鸿蒙电脑的原生应用是 **HAP 包（ArkTS + ArkUI）**，不是可执行的 ELF/EXE；系统没有 winit/OpenGL 窗口后端，也没有可随意执行的外部进程。因此：

- **不能**把桌面版 egui/eframe 直接交叉编译过去；
- **可以**把纯 Rust 压缩内核（zip / image / gif / rayon / serde，全部无 C 依赖）编译为 `staticlib`，通过 C++ N-API 挂到 `libentry.so`，由 ArkTS 界面调用。

适配采用「**同一内核 + 双 UI 壳 + 同一 JSON 协议**」：桌面版功能、交互状态机、设置结构、事件语义、输出命名与 PPTX 数据格式全部保持不变，鸿蒙版只是换了一层渲染。

**唯一的能力降级**：HAP 沙箱内没有可再分发/可执行的 FFmpeg 子进程，**视频项保持原样并显式跳过**，图片、GIF、XML 清理、OOXML 引用完整性、批量队列、取消、原子写与回读校验等其余核心能力全部可用。视频转码开关在界面上可见但禁用，并有明确提示。未来若把 LGPL FFmpeg 以 `libav*` 动态库方式链入 HAP（改用 C API 而非子进程），只需放开 `core::platform::video_transcode_available()` 即可恢复，协议与界面无需改动。

---

## 1. 需要调整的模块

| 层 | 文件 | 调整内容 |
|---|---|---|
| 构建配置 | `Cargo.toml` | 新增 `desktop` / `ohos` / `ffi` feature；`eframe`、`egui`、`rfd` 改为可选；`[lib] crate-type = ["rlib","staticlib"]`；`[[bin]]` 加 `required-features=["desktop"]`；`image` 关闭默认 features，只保留 jpeg/png |
| 入口分层 | `src/lib.rs`、`src/main.rs` | `app`/`ui` 只在 desktop feature 编译；Linux 字体/路径分支排除 `target_env="ohos"` |
| 平台抽象（新增） | `src/core/platform.rs` | `is_harmonyos()`、`video_transcode_available()`、`max_image_threads()`、`init_sandbox()`、`tempdir()`、`data_dir()` |
| 共享状态模型（新增） | `src/core/state.rs` | `PersistedState` 与桌面版解耦上移，两端共用同一个 JSON 结构和键 `pptx-compressor.state.v1` |
| 协议层（新增） | `src/core/bridge.rs` | `BridgeRequest`（inspect/compress/batch）、`BridgeEvent`（11 类标签化事件）、`Capabilities`、`StartReply`、`PollReply`；后台线程 + 事件队列 + 取消 |
| C ABI（新增） | `src/ffi/mod.rs` | 单入口 `pc_call(request, out, cap)` 两遍长度协议、`pc_abi_version()`；panic 不跨 FFI 边界 |
| 内核改动 | `src/core/pipeline.rs`、`batch.rs` | 临时目录改走 `platform::tempdir()`；线程数走 `platform::max_image_threads()`；视频不可用时的跳过理由 |
| 内核改动 | `src/utils/ffmpeg.rs` | `locate()` 在能力关闭时直接返回 None，连探测子进程都不发起；随包/数据目录候选路径在 ohos 下 cfg 掉 |
| 数据模型 | `src/core/mod.rs` | `MediaType` / `MediaStatus` 增加 serde 派生，供 JSON 事件跨端传输 |
| 鸿蒙工程（新增） | `ohos/` | Stage 模型 HAP：`app.json5` / `module.json5`（`deviceTypes: 2in1/tablet/phone`）/ `build-profile.json5`（CMake、`abiFilters`）/ ArkTS 界面 / N-API 层 |
| N-API 层（新增） | `ohos/entry/src/main/cpp/` | `CMakeLists.txt` 链接 `libpptx_compressor.a`；`napi_init.cpp` 导出 `call` / `abiVersion`；`pptx_bridge.h` C ABI 声明；`types/libentry/index.d.ts` |
| ArkTS 层（新增） | `ohos/entry/src/main/ets/` | `Protocol.ets`（协议模型）、`Bridge.ets`（N-API 封装）、`SettingsStore.ets`（preferences）、`SandboxFiles.ets`（文档选择器 URI → 沙箱路径 → 导出）、`Index.ets`（双区界面与六态状态机）、`Theme.ets`（与桌面同款设计令牌） |
| 构建脚本（新增） | `scripts/build-ohos.sh`、`scripts/build-hap.sh`、`scripts/verify-ohos.sh` | 双 ABI 静态库交叉编译、HAP 打包、产物自检 |
| 测试（新增） | `tests/bridge_tests.rs`、`tests/ffi_tests.rs` | 主机上模拟鸿蒙沙箱跑通 inspect→compress→batch 全链路；C ABI 两遍协议与错误注入 |

---

## 2. 系统兼容性处理

1. **目标判定**：OHOS 的 Rust triple 是 `aarch64-unknown-linux-ohos` / `x86_64-unknown-linux-ohos`，其 `target_os = "linux"` 但 `target_env = "ohos"`，C 运行时为 musl。所有原本写 `#[cfg(target_os="linux")]` 的分支都改成 `all(target_os="linux", not(target_env="ohos"))`，避免误用桌面 Linux 的字体、PATH 与数据目录。
2. **沙箱路径**：HAP 没有可靠的 `HOME` / `TMPDIR`。ArkTS 在 `aboutToAppear` 调用 `init`，把 `filesDir` / `cacheDir` / `cacheDir/tmp` 注入 Rust；`platform::init_sandbox()` 建目录并让所有 `tempfile` 落在其中。
3. **文件访问**：`DocumentViewPicker` 返回的是 content URI，Rust 只能读普通路径。`SandboxFiles.pickAndImport()` 先把选中 PPTX 复制进沙箱 `filesDir/input/`，压缩结果落在 `filesDir/output/`，再由 `DocumentSaveOptions` 导出到用户目录。桌面版拿绝对路径的语义在这里由 URI 转换补齐。
4. **持久化**：`@ohos.data.preferences` 存同一份 `PersistedState` JSON、同一个键；首次启动向 Rust `default_settings` 取权威默认值，避免两端默认值漂移。
5. **进程限制**：`locate()` 在鸿蒙侧不启动任何子进程；`pipeline` 把视频项标记为 `Skipped` 并写活动日志；`compress` 请求即便强行把视频 `enabled` 置 true，内核也会再次钳制。
6. **资源与调度**：图片并行上限在鸿蒙侧降为 2（桌面 4）；`image` crate 只启用 jpeg/png，去掉 AVIF/EXR/TIFF 等用不到的编码器，显著减小 `libentry.so`。
7. **UI 适配**：`deviceTypes` 包含 `2in1`；`onAreaChange` 以 900vp 为断点，宽屏走「左 6 : 右 4」双区、窄屏纵向堆叠；系统 HarmonyOS Sans 自带中文字形，无需桌面版那套 TTC 字体探测。
8. **协议/版本**：启动握手校验 `pc_abi_version()` 与 `protocol_version`，不匹配直接报错，避免静态库与 ArkTS 版本错配导致字段误解。
9. **线程与取消**：`start_job` 立即返回 jobId，重活在 Rust std 线程；ArkTS 以 120ms 轮询事件队列（全部为同步、非阻塞的 N-API 调用，UI 线程不做重活）；取消沿用同一个 `AtomicBool`。
10. **合规**：鸿蒙包内不含任何 FFmpeg 二进制，也不自动下载，延续 v1.2.0 起的许可证策略。

---

## 3. 安装与启动方式

### 开发者构建（macOS / Windows，本机需装 DevEco Studio）

```bash
rustup target add aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos

# 1) 交叉编译 Rust 内核为双 ABI 静态库
./scripts/build-ohos.sh                 # 产物 ohos/entry/src/main/cpp/libs/<abi>/libpptx_compressor.a

# 2) 构建 HAP（自动配置 hvigor / ohpm / OHOS SDK / JBR）
./scripts/build-hap.sh                 # 产物 ohos/entry/build/default/outputs/default/entry-default-unsigned.hap

# 3) 产物自检
./scripts/verify-ohos.sh
```

也可直接用 DevEco Studio：打开 `ohos/` 目录（**必须是鸿蒙工程根，不要打开仓库根**）→ File → Sync → Build → Build App。

### 签名与安装

首轮构建产出的是**未签名 HAP**，只能用于产物检查。真机/模拟器安装前：

1. DevEco Studio → File → Project Structure → Signing Configs，用华为开发者账号自动生成证书与 Profile（会自动写入 `ohos/build-profile.json5` 的 `signingConfigs`，并在 `products[0]` 加 `signingConfig`）；
2. 设备开启开发者模式与 USB 调试，`hdc list targets` 能看到序列号；
3. 安装与启动：

```bash
hdc install -r ohos/entry/build/default/outputs/default/entry-default-signed.hap
hdc shell aa start -b com.gaowanlong.pptxcompressor -a EntryAbility
```

或在 DevEco Studio 直接点 Run（自动部署并拉起主窗口）。应用市场分发需按华为要求完成签名与应用审核；`2in1` 设备类型与键鼠交互已在 `module.json5` 声明。

### 用户使用（与桌面版一致）

选择 PPTX（可多选）→ 解析出媒体清单与体积构成 → 选预设/调滑杆/勾选媒体 → 开始压缩 → 查看进度与日志 → 导出 `<原名>_compressed.pptx`。取消不会留下输出文件；输出命名、`<原名>_compressed[_n].pptx` 去重规则、OOXML 结构与桌面版完全相同。

---

## 4. 验证要点

### 已在本机完成的自动化验证

| 项目 | 结果 |
|---|---|
| `cargo test --all-features` | 131 项通过、1 项按设计跳过（真实 FFmpeg 转码用例），含新增 3 项桥接 + 4 项 C ABI 测试；原有测试全部保持通过 |
| `cargo clippy --all-features --all-targets -- -D warnings` | 零告警 |
| OHOS arm64 / x86_64 交叉编译 | 双 ABI `libpptx_compressor.a` 均构建成功（纯 Rust，无 C 依赖链接失败） |
| HAP 构建 | `assembleApp` 成功；ArkTS 编译、CMake/Ninja 原生构建、资源打包全通过 |
| HAP 内容检查 | `libs/arm64-v8a/libentry.so`、`libs/x86_64/libentry.so` 均打进包；`llvm-nm` 可见导出符号 `pc_call` / `pc_abi_version` |
| 产物体积 | 双 ABI 静态库 51 MB / 53 MB，未签名 HAP 16 MB（关闭 `image` 默认 features 后原生库明显瘦身） |
| 协议一致性 | `PersistedState` JSON 键与旧偏好兼容；事件 `type` 为 snake_case；`default_settings` 返回 75/45 等桌面默认值 |
| 端到端（模拟鸿蒙沙箱） | inspect → compress → batch 全链路：视频被禁用并给出告警、输出 PPTX 可回读、批量 2/2 成功、汇总数据正确 |
| 错误注入 | 非法 JSON / 未知 op / 空指针 / 缓冲不足 / 不存在 job 均返回结构化错误，不 panic、不跨 FFI 展开 |

### 真机上必须人工确认的项

1. **签名与安装**：未签名包会被系统拒绝；确认 `hdc install` 返回 Success，桌面出现应用图标。
2. **窗口与断点**：自由窗口缩放到 <900vp 时切换为纵向布局；最大化/最小化/关闭正常；键鼠 hover、滚轮、快捷键不吞事件。
3. **文件选择器**：多选 `.pptx` 后能正确导入沙箱；中文/空格/超长文件名不丢；导出到用户目录后能被 WPS/PowerPoint 打开。
4. **核心链路**：纯图片 PPTX 压缩后体积下降且能正常打开；「变大不替换」仍成立；GIF 帧/透明/循环不变；带签名、宏、OLE 嵌入对象的文稿显示告警。
5. **视频降级**：含视频的文稿在鸿蒙版中视频保持原样、日志出现「不提供 FFmpeg」提示，输出仍是合法 PPTX；与桌面版输出逐字节对比时，差异应只来自视频未转码。
6. **批量与取消**：多文件队列失败隔离；压缩途中取消不产生输出文件；重复处理同一文件不会覆盖旧结果。
7. **偏好持久化**：杀进程重启后预设、质量、高级设置展开状态保留；把桌面版 `PersistedState` JSON 导入 preferences 后能正确解析。
8. **性能与内存**：大文稿（数百 MB、上千媒体）不 OOM；2 线程并行下 UI 不卡顿；`cacheDir/tmp` 在任务结束后被清理。
9. **数据格式对照**：同一 PPTX 分别用桌面版与鸿蒙版压缩，比较 `[Content_Types].xml`、`.rels`、媒体关系与 ZIP 条目集合，除视频外应完全一致。

---

## 5. 已知限制与后续可选项

- 视频转码暂不可用（沙箱不能跑外部 FFmpeg）。可选恢复方式：把 LGPL 版 `libavcodec/libavformat/...` 以动态库形式随 HAP 分发，改用 C API 在 Rust 内转码；需重新过许可证门禁并补充 HAP 侧合规声明。
- 当前未接入 HarmonyOS 的 Desktop Extension Kit（托盘）、跨设备接续与拖拽文件入窗；这些属于平台增强，不影响「与桌面一致」的现有交互。
- HAP 未签名，CI 只能做产物检查；真机分发需开发者证书。
- 无 PC 模拟器镜像时，界面级视觉回归仍需真机目视确认。
