# 第三方组件声明

本项目的**源码**以 MIT 许可证发布（见 `LICENSE`）。自 **v1.2.0** 起，发布包**不包含任何 FFmpeg 二进制**，也不在运行时下载；包内的可执行文件只有本项目自身编译产物。

## FFmpeg

### 当前状态（v1.2.0 及以后）

- 用途：把 PPTX 内嵌视频重编码为 H.264 / AAC（可选功能）。
- 提供方：**使用者本机**。程序按「手动指定路径 → 随包副本（若维护者用合规构建打包）→ 用户数据目录 → 系统 PATH」顺序查找；找不到时视频项明确报错，图片 / GIF / XML 压缩仍正常工作。
- 许可证：由使用者所安装的构建决定，其使用与合规责任由使用者自行承担。程序会读取 `ffmpeg -version` 的 `configuration` 行，在界面标注 LGPL / GPL / nonfree / 未识别，但**运行时不做拦截**（个人自用 GPL 构建不构成再分发）。
- 编码器要求：需带 `libx264` 或 `libopenh264`；程序自动选择，前者用 CRF 质量模型，后者用码率模型。
- 安装与指定方式：[FFMPEG_SETUP.md](FFMPEG_SETUP.md)。

### 历史版本（v1.0.0 / v1.0.1 / v1.1.0）的风险说明

这三个版本的发布包曾附带以下二进制，**现已从仓库删除**：

| 平台 | 版本 / 来源 | 构建配置 |
| --- | --- | --- |
| macOS | FFmpeg 6.0，evermeet.cx 静态包 | `--enable-gpl --enable-version3 --enable-nonfree --enable-libx264 ...` |
| Windows | gyan.dev essentials 包 | `--enable-gpl --enable-version3 --enable-libx264 ...` |

因启用 `--enable-gpl` / `--enable-version3`，这些二进制按 GPLv2+ / GPLv3+ 授权；macOS 构建还启用了 `--enable-nonfree`，FFmpeg 官方明确说明 **nonfree 构建不可再分发**。GPL 同时要求随二进制提供对应源码或有效的源码获取方式。把这类二进制与 MIT 源码捆绑分发存在明确的法律风险，因此本版移除了它们。

> **待办（维护者）**：v1.0.0 / v1.0.1 / v1.1.0 已发布的 Release 资产仍包含上述二进制。清理它们属于对外部已发布内容的操作，本版未擅自改动，请在 GitHub Release 页面手动删除对应附件，或用合规构建重新打包后替换。

参考：<https://ffmpeg.org/legal.html> 、<https://ffmpeg.org/download.html> 、<https://www.gnu.org/licenses/gpl-3.0.html>

### 维护者：重新内置合规 FFmpeg 的条件

`package.sh` 默认不内置 FFmpeg。只有在显式设置 `FFMPEG_MACOS` / `FFMPEG_WINDOWS` 时才会打包，且必须通过 `scripts/ffmpeg-license-check.sh` 门禁：拒绝 `--enable-gpl` / `--enable-version3` / `--enable-nonfree`，也拒绝读不到构建配置的二进制。

可用的合规构建来源：

- Windows：[BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest) 的 `-win64-lgpl` 静态构建（不含 libx264 / libx265），由 `scripts/fetch-ffmpeg-lgpl.sh` 下载并校验。
- macOS：没有可直接再分发的 LGPL 预编译版本（evermeet.cx 是 GPL+nonfree，Homebrew 默认也是 GPLv3）。`scripts/build-ffmpeg-lgpl-macos.sh` 提供从源码构建的复现路径，用 BSD 许可的 [libopenh264](https://github.com/cisco/openh264) 替代 GPL 的 libx264。

若决定随包分发 LGPL 二进制，还需自行履行 LGPL 义务：**附上对应版本的 LGPL / 相关许可文本、FFmpeg 与所用第三方库的源码或有效获取方式、构建配置说明**，并把本文件的「版本 / 构建配置 / 许可证 / 来源」四栏更新为实际使用的构建。脚本只负责生成合规二进制与门禁，不代替这些义务。

## 其他 Rust 依赖

所有 Rust crate 通过 Cargo 从 crates.io 获取，各自的许可证见 `Cargo.lock` 对应仓库；主要依赖包括 egui / eframe（MIT / Apache-2.0 双许可）、`image`、`gif`、`zip`、`rayon`、`serde` 等。发布包仅静态链接这些 crate 的编译产物。`image` 在 v1.3.0 起关闭默认 features，只编译 JPEG / PNG 解码器，不再引入 AVIF / EXR / TIFF 等用不到的编码器。

## 鸿蒙电脑（HAP）版本

鸿蒙版复用上述同一批 Rust crate，交叉编译为 `libpptx_compressor.a` 后由 CMake 链接进 `libentry.so`；链接的系统库为 OHOS NDK 自带的 `libace_napi.z.so`、`libhilog_ndk.z.so`、`libuv.so`、`z`，均来自 HarmonyOS SDK，其许可随 DevEco Studio 的 SDK NOTICE 一并提供。

HAP 内**不含任何 FFmpeg 二进制、也不含 GPL / nonfree 组件**，延续 v1.2.0 起的合规策略。ArkTS 侧仅使用 HarmonyOS SDK 的公开 Kit（AbilityKit、ArkData、CoreFileKit、ArkUI），不引入第三方前端依赖。
