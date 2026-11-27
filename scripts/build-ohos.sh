#!/usr/bin/env bash
# 交叉编译 PPTX 压缩内核为鸿蒙电脑 HAP 可链接的静态库。
#
# 用法：
#   ./scripts/build-ohos.sh                 # arm64-v8a + x86_64（真机 + PC 模拟器）
#   ./scripts/build-ohos.sh arm64-v8a      # 只编真机 ABI
#   OHOS_NDK_HOME=/path/to/native ./scripts/build-ohos.sh
#
# 前置：
#   - DevEco Studio 自带 OHOS NDK（默认自动探测 macOS 路径）
#   - rustup target add aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos
#
# 产物：ohos/entry/src/main/cpp/libs/<abi>/libpptx_compressor.a
# 之后用 DevEco Studio / hvigorw 构建 HAP（见 HARMONYOS_PC.md）。
set -euo pipefail

cd "$(dirname "$0")/.."

NDK="${OHOS_NDK_HOME:-}"
if [ -z "$NDK" ]; then
  for candidate in \
    "/Applications/DevEco-Studio.app/Contents/sdk/default/openharmony/native" \
    "$HOME/Library/Huawei/Sdk/openharmony/native" \
    "$HOME/ohos-sdk/linux/native"; do
    if [ -d "$candidate/llvm/bin" ]; then
      NDK="$candidate"
      break
    fi
  done
fi
if [ ! -d "$NDK/llvm/bin" ]; then
  echo "找不到 OHOS NDK，请设置 OHOS_NDK_HOME（需包含 llvm/bin 与 sysroot）" >&2
  exit 1
fi
echo "→ OHOS NDK: $NDK"

ABIS="${*:-arm64-v8a x86_64}"
CRATE="libpptx_compressor.a"
OUT_ROOT="ohos/entry/src/main/cpp/libs"

build_one() {
  local abi="$1" target linker ar out_dir
  case "$abi" in
    arm64-v8a) target="aarch64-unknown-linux-ohos" ;;
    x86_64)    target="x86_64-unknown-linux-ohos" ;;
    *) echo "未知 ABI: $abi" >&2; exit 1 ;;
  esac
  linker="$NDK/llvm/bin/${target}-clang"
  ar="$NDK/llvm/bin/llvm-ar"
  if [ ! -x "$linker" ]; then
    echo "缺少链接器: $linker" >&2
    exit 1
  fi
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "缺少 Rust target: $target" >&2
    echo "先执行: rustup target add $target" >&2
    exit 1
  fi

  local cargo_env_var
  cargo_env_var="CARGO_TARGET_$(echo "$target" | tr '[:lower:]-' '[:upper:]_')_LINKER"
  local ar_env_var
  ar_env_var="CARGO_TARGET_$(echo "$target" | tr '[:lower:]-' '[:upper:]_')_AR"
  export "$cargo_env_var=$linker"
  export "$ar_env_var=$ar"

  echo "→ [$abi] cargo build --release --lib --target $target --no-default-features --features ohos"
  cargo build --release --lib --target "$target" --no-default-features --features ohos

  local artifact="target/$target/release/$CRATE"
  if [ ! -f "$artifact" ]; then
    echo "未生成静态库: $artifact" >&2
    exit 1
  fi
  out_dir="$OUT_ROOT/$abi"
  mkdir -p "$out_dir"
  cp "$artifact" "$out_dir/$CRATE"
  echo "✓ [$abi] $(du -h "$out_dir/$CRATE" | cut -f1) → $out_dir/$CRATE"
}

for abi in $ABIS; do
  build_one "$abi"
done

echo "✓ 静态库就绪。现在可在 DevEco Studio 打开 ohos/ 构建 HAP。"
