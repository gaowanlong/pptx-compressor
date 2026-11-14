#!/usr/bin/env bash
# 打包 PPTX 压缩器发布版。
#
# 用法：
#   ./package.sh                  # macOS release（默认）
#   ./package.sh macos            # macOS release
#   ./package.sh macos debug      # macOS debug
#   ./package.sh windows          # Windows 交叉编译 + 打包
#   ./package.sh all              # 两个平台
#
# 替换 FFmpeg（例如换成 LGPL 合规构建）：
#   FFMPEG_MACOS=/path/to/lgpl-ffmpeg ./package.sh macos
#   FFMPEG_WINDOWS=/path/to/lgpl-ffmpeg.exe ./package.sh windows
set -euo pipefail

cd "$(dirname "$0")"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$VERSION" ]; then
    echo "无法从 Cargo.toml 读取版本号" >&2
    exit 1
fi

FFMPEG_MACOS="${FFMPEG_MACOS:-resources/ffmpeg/macos/ffmpeg}"
FFMPEG_WINDOWS="${FFMPEG_WINDOWS:-resources/ffmpeg/windows/ffmpeg.exe}"

require_file() {
    if [ ! -f "$1" ]; then
        echo "缺少必需文件：$1" >&2
        exit 1
    fi
}

make_app_bundle() {
    local app_bundle="$1"
    local macos_dir="$app_bundle/Contents/MacOS"
    local resources_dir="$app_bundle/Contents/Resources"
    mkdir -p "$macos_dir" "$resources_dir"

    cat > "$app_bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>pptx-compressor</string>
    <key>CFBundleIdentifier</key><string>com.gaowanlong.pptx-compressor</string>
    <key>CFBundleName</key><string>PPTX Compressor</string>
    <key>CFBundleDisplayName</key><string>PPTX 压缩器</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleIconFile</key><string>AppIcon.icns</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

    if [ -f assets/AppIcon.icns ]; then
        cp assets/AppIcon.icns "$resources_dir/AppIcon.icns"
    fi
}

package_macos() {
    local profile="${1:-release}"
    local app_bundle="target/PPTX Compressor.app"
    local binary="target/$profile/pptx-compressor"
    local archive="target/PPTX-Compressor-v${VERSION}-macOS.zip"

    echo "→ [macOS] 构建（${profile}）..."
    cargo build --"$profile"
    require_file "$binary"
    require_file "$FFMPEG_MACOS"
    require_file THIRD_PARTY_NOTICES.md

    echo "→ [macOS] 创建 .app 骨架..."
    rm -rf "$app_bundle"
    make_app_bundle "$app_bundle"
    cp "$binary" "$app_bundle/Contents/MacOS/pptx-compressor"
    cp "$FFMPEG_MACOS" "$app_bundle/Contents/Resources/ffmpeg"
    chmod +x "$app_bundle/Contents/MacOS/pptx-compressor" "$app_bundle/Contents/Resources/ffmpeg"
    cp README.md THIRD_PARTY_NOTICES.md CHANGELOG.md "$app_bundle/Contents/Resources/"

    echo "→ [macOS] 重新签名..."
    codesign --force --deep --sign - "$app_bundle"

    echo "→ [macOS] 生成 ZIP..."
    rm -f "$archive"
    ditto -c -k --sequesterRsrc --keepParent "$app_bundle" "$archive"
    echo "✓ [macOS] 完成：$archive"
}

package_windows() {
    local profile="${1:-release}"
    local exe="target/x86_64-pc-windows-msvc/$profile/pptx-compressor.exe"
    local out_dir="target/windows-dist"
    local archive="target/PPTX-Compressor-v${VERSION}-Windows.zip"

    echo "→ [Windows] 交叉编译（${profile}）..."
    cargo xwin build --target x86_64-pc-windows-msvc --"$profile"
    require_file "$exe"
    require_file "$FFMPEG_WINDOWS"
    require_file THIRD_PARTY_NOTICES.md

    echo "→ [Windows] 打包..."
    rm -rf "$out_dir"
    mkdir -p "$out_dir"
    cp "$exe" "$out_dir/"
    cp "$FFMPEG_WINDOWS" "$out_dir/ffmpeg.exe"
    cp README.md THIRD_PARTY_NOTICES.md CHANGELOG.md "$out_dir/"

    local prev="$PWD"
    (cd "$out_dir" && zip -q -9 -r "$prev/$archive" .)
    rm -rf "$out_dir"
    echo "✓ [Windows] 完成：$archive"
}

case "${1:-macos}" in
    macos)   package_macos "${2:-release}" ;;
    windows) package_windows "${2:-release}" ;;
    all)     package_macos "${2:-release}"; package_windows "${2:-release}" ;;
    *)       echo "用法：$0 {macos|windows|all} [release|debug]"; exit 1 ;;
esac
