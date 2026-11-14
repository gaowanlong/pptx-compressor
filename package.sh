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

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$VERSION" ]; then
    echo "Cannot read version from Cargo.toml" >&2
    exit 1
fi

require_file() {
    if [ ! -f "$1" ]; then
        echo "Missing required file: $1" >&2
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
    <key>CFBundleDisplayName</key><string>PPTX Compressor</string>
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

    echo "→ [macOS] Building ($profile)..."
    cargo build --"$profile"
    require_file "$binary"
    require_file resources/ffmpeg/macos/ffmpeg
    require_file THIRD_PARTY_NOTICES.md

    echo "→ [macOS] Creating app bundle..."
    rm -rf "$app_bundle"
    make_app_bundle "$app_bundle"
    cp "$binary" "$app_bundle/Contents/MacOS/pptx-compressor"
    cp resources/ffmpeg/macos/ffmpeg "$app_bundle/Contents/Resources/ffmpeg"
    chmod +x "$app_bundle/Contents/MacOS/pptx-compressor" "$app_bundle/Contents/Resources/ffmpeg"
    cp README.md THIRD_PARTY_NOTICES.md CHANGELOG.md "$app_bundle/Contents/Resources/"

    echo "→ [macOS] Re-signing..."
    codesign --force --deep --sign - "$app_bundle"

    echo "→ [macOS] Creating ZIP..."
    rm -f "$archive"
    ditto -c -k --sequesterRsrc --keepParent "$app_bundle" "$archive"
    echo "✓ [macOS] Done: $archive"
}

package_windows() {
    local profile="${1:-release}"
    local exe="target/x86_64-pc-windows-msvc/$profile/pptx-compressor.exe"
    local out_dir="target/windows-dist"
    local archive="target/PPTX-Compressor-v${VERSION}-Windows.zip"

    echo "→ [Windows] Cross-compiling ($profile)..."
    cargo xwin build --target x86_64-pc-windows-msvc --"$profile"
    require_file "$exe"
    require_file resources/ffmpeg/windows/ffmpeg.exe
    require_file THIRD_PARTY_NOTICES.md

    echo "→ [Windows] Packaging..."
    rm -rf "$out_dir"
    mkdir -p "$out_dir"
    cp "$exe" "$out_dir/"
    cp resources/ffmpeg/windows/ffmpeg.exe "$out_dir/"
    cp README.md THIRD_PARTY_NOTICES.md CHANGELOG.md "$out_dir/"

    local prev="$PWD"
    (cd "$out_dir" && zip -q -9 -r "$prev/$archive" .)
    rm -rf "$out_dir"
    echo "✓ [Windows] Done: $archive"
}

case "${1:-macos}" in
    macos)   package_macos "${2:-release}" ;;
    windows) package_windows "${2:-release}" ;;
    all)     package_macos "${2:-release}"; package_windows "${2:-release}" ;;
    *)       echo "Usage: $0 {macos|windows|all} [release|debug]"; exit 1 ;;
esac
