# 第三方组件声明

本项目的**源码**以 MIT 许可证发布（见 `LICENSE`）。发布包中另包含以下第三方二进制，它们不受 MIT 许可证覆盖。

## FFmpeg

- 版本：FFmpeg 6.0
- 随包路径：
  - macOS：`PPTX Compressor.app/Contents/Resources/ffmpeg`
  - Windows：`ffmpeg.exe`
- 用途：把 PPTX 内嵌视频重编码为 H.264 / AAC。
- 构建配置（由 `ffmpeg -version` 读取）：

  ```
  --enable-gpl --enable-version3 --enable-nonfree --enable-libx264
  --enable-libx265 --enable-libvpx --enable-libaom --enable-libass ...
  ```

- 许可证：因启用 `--enable-gpl` 与 `--enable-version3`，该二进制按 **GPLv3** 授权；同时启用了 `--enable-nonfree`，FFmpeg 官方明确说明 nonfree 构建**不可再分发**。
- 来源：macOS 构建来自 evermeet.cx 静态包，Windows 构建来自 gyan.dev essentials 包。
- 源码与许可证文本：<https://ffmpeg.org/download.html> 、<https://www.gnu.org/licenses/gpl-3.0.html>

### 分发风险与建议

GPLv3 要求随二进制提供对应源码或有效的源码获取方式，且 nonfree 构建本身不允许再分发。当前发布包附带本声明并不等于已完全满足合规要求。**建议后续版本改用仅启用 LGPL 组件的 FFmpeg 构建**（移除 `--enable-gpl` / `--enable-nonfree` 与 x264，改用 libopenh264 等 LGPL 兼容编码器），或改为运行时由用户自行提供 FFmpeg。

在替换之前，使用者与再分发者需自行评估并承担相应的许可证义务。项目维护者已在 `CHANGELOG.md` 的「已知限制」中记录该项。

## 其他 Rust 依赖

所有 Rust crate 通过 Cargo 从 crates.io 获取，各自的许可证见 `Cargo.lock` 对应仓库；主要 GUI 依赖 egui / eframe 为 MIT / Apache-2.0 双许可。
