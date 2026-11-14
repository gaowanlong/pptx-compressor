# FFmpeg 安装说明（v1.2.0 起）

## 为什么不再随包提供

v1.1.0 及更早版本的发布包内置了 FFmpeg 二进制，其构建参数为
`--enable-gpl --enable-version3 --enable-nonfree`。这类构建**不允许再分发**，
与本项目 MIT 源码捆绑提供存在明确的许可证冲突；而 macOS 平台没有可直接
再分发的 LGPL 预编译版本。

因此 v1.2.0 起，发布包默认**不包含 FFmpeg**。图片、GIF、XML 注释与缩略图
压缩完全不受影响；只有嵌入视频的重编码需要 FFmpeg。

## 普通用户：准备 FFmpeg

| 平台 | 方式 |
| --- | --- |
| macOS | `brew install ffmpeg`，或下载官方构建后解压到 PATH |
| Windows | 下载 FFmpeg 官方构建或 [BtbN LGPL 构建](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest)，把 `ffmpeg.exe` 放到 PATH，或记住路径 |

程序启动时按以下顺序自动查找：

1. 你在界面里手动指定并保存的路径
2. 随包副本（仅当维护者用合规构建打包）
3. 用户数据目录 `pptx-compressor/ffmpeg/`
4. 系统 `PATH`

找不到时，右侧「环境说明」会显示红色提示和 **「选择本机 FFmpeg…」** 按钮，
也可以点击 **「重新检测」** 免重启刷新。路径会随偏好一起保存。

> FFmpeg 需要带有 H.264 编码器（`libx264` 或 `libopenh264`），否则视频项会
> 明确报错而不是静默跳过。程序会自动识别：有 `libx264` 用 CRF 质量模型，
> 只有 `libopenh264` 时自动切换为码率模型。

用户自行安装的 FFmpeg 由用户按其许可证使用；本项目不分发、不下载该二进制。

## 维护者：重新打包合规的随包 FFmpeg

如果确实希望恢复「开箱即用」，必须使用 LGPL 构建，并在打包时显式指定路径。
`package.sh` 会强制执行许可证门禁，拒绝任何 `gpl` / `version3` / `nonfree` 构建。

```bash
# Windows：下载 BtbN 的 win64 LGPL 静态构建（约 176 MB）
./scripts/fetch-ffmpeg-lgpl.sh windows
FFMPEG_WINDOWS=build/compliant/ffmpeg.exe ./package.sh windows

# macOS：没有现成 LGPL 构建，需从源码编译（libopenh264 替代 libx264）
./scripts/build-ffmpeg-lgpl-macos.sh
FFMPEG_MACOS=build/compliant/ffmpeg ./package.sh macos
```

单独校验任意二进制：

```bash
./scripts/ffmpeg-license-check.sh /path/to/ffmpeg
```

随包分发 LGPL 二进制时，还需附上对应版本的 LGPL 许可文本、FFmpeg 源码
获取方式与所用第三方库的许可信息，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
