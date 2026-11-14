#!/usr/bin/env bash
# 从源码构建可再分发的 LGPL FFmpeg（macOS / Apple Silicon）。
#
# 关键点：
#   · 不带 --enable-gpl / --enable-version3 / --enable-nonfree
#   · 用 BSD 许可的 libopenh264 提供 H.264 编码，替代 GPL 的 libx264
#   · 静态链接，产物是单个可执行文件
#
# 用法：./scripts/build-ffmpeg-lgpl-macos.sh
# 产物：build/compliant/ffmpeg
#
# 参考：https://ffmpeg.org/legal.html 、https://github.com/cisco/openh264
set -euo pipefail

cd "$(dirname "$0")/.."
BUILD_DIR="$PWD/build/ffmpeg-lgpl-src"
OUT_DIR="$PWD/build/compliant"
mkdir -p "$BUILD_DIR" "$OUT_DIR"

for tool in brew git make pkg-config nasm; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "缺少工具：$tool（先执行 brew install openh264 pkg-config nasm）" >&2
        exit 1
    fi
done

if ! pkg-config --exists openh264; then
    echo "未检测到 openh264，先执行：brew install openh264" >&2
    exit 1
fi

if [ ! -d "$BUILD_DIR/ffmpeg/.git" ]; then
    echo "→ 拉取 FFmpeg 源码 ..."
    git clone --depth 1 https://git.ffmpeg.org/ffmpeg.git "$BUILD_DIR/ffmpeg"
fi

PREFIX="$BUILD_DIR/install"
echo "→ configure（LGPL，仅 libopenh264）..."
(cd "$BUILD_DIR/ffmpeg" && ./configure \
    --prefix="$PREFIX" \
    --disable-gpl \
    --disable-nonfree \
    --enable-libopenh264 \
    --enable-pthreads \
    --disable-doc \
    --disable-debug \
    --pkg-config-flags=--static)

echo "→ make ..."
make -C "$BUILD_DIR/ffmpeg" -j"$(sysctl -n hw.ncpu)"
make -C "$BUILD_DIR/ffmpeg" install

cp "$PREFIX/bin/ffmpeg" "$OUT_DIR/ffmpeg"
chmod +x "$OUT_DIR/ffmpeg"

echo "→ 许可证门禁校验 ..."
"$PWD/scripts/ffmpeg-license-check.sh" "$OUT_DIR/ffmpeg"
"$OUT_DIR/ffmpeg" -hide_banner -encoders | grep -E 'libopenh264|libx264' || true

echo
echo "✓ 产物：$OUT_DIR/ffmpeg"
echo "  打包：FFMPEG_MACOS=$OUT_DIR/ffmpeg ./package.sh macos"
echo "  随分发包请附上 LGPL 许可文本与源码获取方式（见 THIRD_PARTY_NOTICES.md）。"
