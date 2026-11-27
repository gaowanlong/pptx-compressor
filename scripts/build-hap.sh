#!/usr/bin/env bash
# 构建鸿蒙电脑 HAP（未签名）。
#
# 前置：
#   1. ./scripts/build-ohos.sh            # 交叉编译 Rust 静态库（两个 ABI）
#   2. 本机安装 DevEco Studio（提供 hvigor / ohpm / NDK / JBR）
#
# 用法：
#   ./scripts/build-hap.sh
#   DEVECO_HOME=/path/to/DevEco-Studio.app/Contents ./scripts/build-hap.sh
#
# 产物：ohos/entry/build/default/outputs/default/entry-default-unsigned.hap
# 真机安装前需在 DevEco Studio 中配置签名（见 HARMONYOS_PC.md）。
set -euo pipefail

cd "$(dirname "$0")/.."

DEVECO_HOME="${DEVECO_HOME:-/Applications/DevEco-Studio.app/Contents}"
if [ ! -x "$DEVECO_HOME/tools/hvigor/bin/hvigorw" ]; then
  echo "找不到 DevEco 工具链：$DEVECO_HOME" >&2
  echo "请设置 DEVECO_HOME 指向 DevEco-Studio.app/Contents" >&2
  exit 1
fi

HVIGOR="$DEVECO_HOME/tools/hvigor/bin/hvigorw"
OHPM="$DEVECO_HOME/tools/ohpm/bin/ohpm"
JBR="$DEVECO_HOME/jbr/Contents/Home"
NODE_BIN="$(command -v node || true)"
NODE_HOME="${NODE_HOME:-}"
if [ -z "$NODE_HOME" ] && [ -n "$NODE_BIN" ]; then
  NODE_HOME="$(dirname "$(dirname "$NODE_BIN")")"
fi

export DEVECO_SDK_HOME="$DEVECO_HOME/sdk"
export OHOS_BASE_SDK_HOME="$DEVECO_HOME/sdk/default/openharmony"
export NODE_HOME
if [ -x "$JBR/bin/java" ]; then
  export JAVA_HOME="$JBR"
  export PATH="$JBR/bin:$PATH"
fi

if [ -d ohos/oh_modules ]; then
  echo "→ oh_modules 已存在，跳过 ohpm install"
else
  echo "→ ohpm install"
  ( cd ohos && "$OHPM" install )
fi

echo "→ 检查 Rust 静态库"
for abi in arm64-v8a x86_64; do
  lib="ohos/entry/src/main/cpp/libs/$abi/libpptx_compressor.a"
  if [ ! -f "$lib" ]; then
    echo "缺少 $lib，请先执行 ./scripts/build-ohos.sh" >&2
    exit 1
  fi
done

echo "→ hvigor assembleApp"
( cd ohos && "$HVIGOR" assembleApp --mode project -p . --no-daemon )

HAP="ohos/entry/build/default/outputs/default/entry-default-unsigned.hap"
if [ ! -f "$HAP" ]; then
  echo "未生成 HAP：$HAP" >&2
  exit 1
fi
echo "✓ 构建完成：$HAP ($(du -h "$HAP" | cut -f1))"
echo "  真机安装前请用 DevEco Studio 配置签名（当前为未签名包）。"
