#!/usr/bin/env bash
# 打包门禁：拒绝把 GPL / version3 / nonfree 构建的 FFmpeg 放进发布包。
#
# 用法：
#   ./scripts/ffmpeg-license-check.sh /path/to/ffmpeg
#   ./scripts/ffmpeg-license-check.sh /path/to/ffmpeg.exe
#
# macOS/Linux 二进制直接执行 `-version` 读取 configuration；
# Windows .exe 无法在 macOS 上执行，改为从二进制中提取编译期配置字符串。
# 读取不到配置时保守拒绝，避免"不确定就打包"。
set -euo pipefail

path="${1:-}"
if [ -z "$path" ]; then
    echo "用法：$0 <ffmpeg 可执行文件>" >&2
    exit 2
fi
if [ ! -f "$path" ]; then
    echo "✗ 文件不存在：$path" >&2
    exit 1
fi

config=""
case "$path" in
    *.exe|*.EXE)
        if ! command -v strings >/dev/null 2>&1; then
            echo "✗ 缺少 strings，无法校验 Windows 二进制的构建配置" >&2
            exit 1
        fi
        # 只认真正带编译开关的那一行，避免命中 "%sconfiguration:" 这类格式串。
        config="$(strings -n 8 "$path" 2>/dev/null | grep -m1 'configuration: --' || true)"
        if [ -z "$config" ]; then
            config="$(strings -n 8 "$path" 2>/dev/null | grep -m1 -- '^--enable-' || true)"
        fi
        ;;
    *)
        if [ ! -x "$path" ]; then
            echo "✗ 文件不可执行：$path" >&2
            exit 1
        fi
        config="$("$path" -version 2>&1 | grep -m1 '^configuration:' || true)"
        ;;
esac

if [ -z "$config" ] || ! printf '%s' "$config" | grep -q -- '--'; then
    echo "✗ 无法读取 FFmpeg 的构建配置，出于合规保守起见拒绝打包" >&2
    exit 1
fi

if printf '%s' "$config" | grep -q -- '--enable-nonfree'; then
    echo "✗ 拒绝打包：nonfree 构建明确不允许再分发" >&2
    exit 1
fi
if printf '%s' "$config" | grep -Eq -- '--enable-gpl|--enable-version3'; then
    echo "✗ 拒绝打包：这是 GPLv2+/GPLv3+ 构建，与 MIT 源码捆绑分发需履行 GPL 义务" >&2
    echo "  请改用 LGPL 构建（见 scripts/fetch-ffmpeg-lgpl.sh）" >&2
    exit 1
fi

echo "✓ 许可证门禁通过：未启用 gpl / version3 / nonfree"
printf '  配置摘要：%.160s\n' "$config"
if ! printf '%s' "$config" | grep -q -- 'libopenh264'; then
    echo "  提示：配置中未见 libopenh264，请确认该构建带有可用的 H.264 编码器" >&2
fi
