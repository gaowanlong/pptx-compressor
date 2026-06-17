#!/usr/bin/env bash
# Package PPTX Compressor for distribution.
#
# Usage:
#   ./package.sh                  # macOS release (default)
#   ./package.sh macos            # macOS release
#   ./package.sh macos debug      # macOS debug
#   ./package.sh windows          # Windows cross-compile + package
#   ./package.sh all              # Both platforms
set -euo pipefail

cd "$(dirname "$0")"

package_macos() {
    local profile="${1:-release}"
    local app_bundle="target/PPTX Compressor.app"
    local binary

    if [ "$profile" = "debug" ]; then
        binary="target/debug/pptx-compressor"
    else
        binary="target/release/pptx-compressor"
    fi

    echo "→ [macOS] Building ($profile)..."
    cargo build --"$profile"

    echo "→ [macOS] Copying binary..."
    cp "$binary" "$app_bundle/Contents/MacOS/pptx-compressor"

    echo "→ [macOS] Copying FFmpeg..."
    mkdir -p "$app_bundle/Contents/Resources"
    cp resources/ffmpeg/macos/ffmpeg "$app_bundle/Contents/Resources/ffmpeg"

    echo "→ [macOS] Re-signing..."
    codesign --force --deep --sign - "$app_bundle"

    echo "→ [macOS] Creating ZIP..."
    ditto -c -k --sequesterRsrc --keepParent "$app_bundle" "target/PPTX-Compressor-macOS.zip"

    echo "✓ [macOS] Done: target/PPTX-Compressor-macOS.zip"
}

package_windows() {
    local profile="${1:-release}"

    echo "→ [Windows] Cross-compiling ($profile)..."
    cargo xwin build --target x86_64-pc-windows-msvc --"$profile"

    local out_dir="target/windows-dist"
    echo "→ [Windows] Packaging..."
    rm -rf "$out_dir"
    mkdir -p "$out_dir"

    if [ "$profile" = "debug" ]; then
        cp "target/x86_64-pc-windows-msvc/debug/pptx-compressor.exe" "$out_dir/"
    else
        cp "target/x86_64-pc-windows-msvc/release/pptx-compressor.exe" "$out_dir/"
    fi
    cp resources/ffmpeg/windows/ffmpeg.exe "$out_dir/"

    cd "$out_dir"
    zip -9 -r ../PPTX-Compressor-Windows.zip .
    cd "$OLDPWD"
    rm -rf "$out_dir"

    echo "✓ [Windows] Done: target/PPTX-Compressor-Windows.zip"
}

case "${1:-macos}" in
    macos)
        package_macos "${2:-release}"
        ;;
    windows)
        package_windows "${2:-release}"
        ;;
    all)
        package_macos "${2:-release}"
        package_windows "${2:-release}"
        ;;
    *)
        echo "Usage: $0 {macos|windows|all} [release|debug]"
        exit 1
        ;;
esac
