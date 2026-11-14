#!/usr/bin/env bash
# 为维护者准备"可再分发"的 LGPL FFmpeg 二进制（可选步骤）。
#
# 默认发布包**不内置 FFmpeg**，本脚本只在你想重新提供开箱即用视频压缩时使用。
#
# 用法：
#   ./scripts/fetch-ffmpeg-lgpl.sh windows   # 下载 BtbN 的 win64 LGPL 静态构建
#   ./scripts/fetch-ffmpeg-lgpl.sh macos     # macOS 没有现成 LGPL 构建，输出源码编译指引
#
# 产物写到 build/compliant/，不会自动放进发布包；通过环境变量显式指定才会打包。
set -euo pipefail

cd "$(dirname "$0")/.."
SCRIPT_DIR="$PWD/scripts"
OUT_DIR="$PWD/build/compliant"
mkdir -p "$OUT_DIR"

case "${1:-windows}" in
    windows)
        ASSET="ffmpeg-master-latest-win64-lgpl.zip"
        URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/${ASSET}"
        ZIP="$OUT_DIR/$ASSET"
        echo "→ 下载 BtbN LGPL 构建（约 176 MB）..."
        echo "  $URL"
        if [ ! -f "$ZIP" ]; then
            curl -fL --retry 3 -o "$ZIP" "$URL"
        else
            echo "  已存在，跳过下载：$ZIP"
        fi

        echo "→ 解压并提取 ffmpeg.exe ..."
        rm -rf "$OUT_DIR/extract"
        mkdir -p "$OUT_DIR/extract"
        unzip -q "$ZIP" -d "$OUT_DIR/extract"
        FOUND="$(find "$OUT_DIR/extract" -type f -name ffmpeg.exe -print -quit)"
        if [ -z "$FOUND" ]; then
            echo "✗ 下载包中未找到 ffmpeg.exe" >&2
            exit 1
        fi
        cp "$FOUND" "$OUT_DIR/ffmpeg.exe"
        rm -rf "$OUT_DIR/extract" "$ZIP"

        echo "→ 许可证门禁校验 ..."
        "$SCRIPT_DIR/ffmpeg-license-check.sh" "$OUT_DIR/ffmpeg.exe"
        echo
        echo "✓ 产物：$OUT_DIR/ffmpeg.exe"
        echo "  打包：FFMPEG_WINDOWS=$OUT_DIR/ffmpeg.exe ./package.sh windows"
        echo "  注意：BtbN 的 LGPL 构建不含 libx264，程序会自动改用 libopenh264（码率模型）。"
        ;;
    macos)
        echo "macOS 没有可直接再分发的 LGPL 预编译 FFmpeg："
        echo "  · evermeet.cx 的静态构建是 GPL + nonfree"
        echo "  · Homebrew 默认 ffmpeg 也是 GPLv3（--enable-gpl --enable-version3）"
        echo
        echo "请从源码构建合规版本："
        echo "  ./scripts/build-ffmpeg-lgpl-macos.sh"
        echo
        echo "构建完成后："
        echo "  FFMPEG_MACOS=build/compliant/ffmpeg ./package.sh macos"
        ;;
    *)
        echo "用法：$0 {windows|macos}" >&2
        exit 2
        ;;
esac
