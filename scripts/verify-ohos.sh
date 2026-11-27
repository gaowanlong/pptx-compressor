#!/usr/bin/env bash
# 鸿蒙版产物自检：静态库、HAP 内容、N-API 导出符号、协议版本一致性。
# 用法：./scripts/verify-ohos.sh [--skip-cargo]
set -euo pipefail

cd "$(dirname "$0")/.."
FAILED=0

pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1" >&2; FAILED=$((FAILED + 1)); }
check() {
  if eval "$2" >/dev/null 2>&1; then pass "$1"; else fail "$1"; fi
}

echo "== 1. 源码与协议一致性 =="
RUST_VER=$(grep -m1 'pub const PROTOCOL_VERSION' src/core/bridge.rs | grep -o '[0-9]\+' | tail -1)
ARKTS_VER=$(grep -m1 'export const PROTOCOL_VERSION' ohos/entry/src/main/ets/model/Protocol.ets | grep -o '[0-9]\+' | tail -1)
[ "$RUST_VER" = "$ARKTS_VER" ] && pass "协议版本一致 (v$RUST_VER)" || fail "协议版本不一致：Rust=$RUST_VER ArkTS=$ARKTS_VER"
check "桌面偏好键与鸿蒙一致" "grep -q 'pptx-compressor.state.v1' src/core/state.rs && grep -q 'pptx-compressor.state.v1' ohos/entry/src/main/ets/model/Protocol.ets"
check "2in1 设备类型已声明" "grep -q '2in1' ohos/entry/src/main/module.json5"
check "HAP 不含 FFmpeg 二进制" "! find ohos -type f \( -name 'ffmpeg' -o -name 'ffmpeg.exe' \) | grep -q ."

if [ "${1:-}" != "--skip-cargo" ]; then
  echo "== 2. Rust 测试 =="
  check "cargo test --all-features" "cargo test --all-features --quiet"
  echo "== 3. clippy =="
  check "clippy 零告警" "cargo clippy --all-features --all-targets --quiet -- -D warnings"
fi

echo "== 4. 双 ABI 静态库 =="
for abi in arm64-v8a x86_64; do
  lib="ohos/entry/src/main/cpp/libs/$abi/libpptx_compressor.a"
  [ -s "$lib" ] && pass "$abi: $(du -h "$lib" | cut -f1)" || fail "$abi 静态库缺失：$lib"
done

echo "== 5. HAP 产物 =="
HAP="ohos/entry/build/default/outputs/default/entry-default-unsigned.hap"
if [ -f "$HAP" ]; then
  pass "HAP 存在 ($(du -h "$HAP" | cut -f1))"
  for abi in arm64-v8a x86_64; do
    check "$abi/libentry.so 已打进 HAP" "unzip -l '$HAP' | grep -q 'libs/$abi/libentry.so'"
  done
  check "module.json 已打进 HAP" "unzip -l '$HAP' | grep -q 'module.json'"
else
  fail "HAP 缺失：$HAP（先执行 ./scripts/build-hap.sh）"
fi

SO="ohos/entry/build/default/intermediates/stripped_native_libs/default/arm64-v8a/libentry.so"
NDK="${OHOS_NDK_HOME:-/Applications/DevEco-Studio.app/Contents/sdk/default/openharmony/native}"
NM_BIN="$NDK/llvm/bin/llvm-nm"
if [ -x "$NM_BIN" ] && [ -f "$SO" ]; then
  echo "== 6. N-API 导出符号 =="
  check "pc_call 已导出" "$NM_BIN -D '$SO' | grep -q ' T pc_call'"
  check "pc_abi_version 已导出" "$NM_BIN -D '$SO' | grep -q ' T pc_abi_version'"
else
  echo "  · 跳过符号检查（缺 llvm-nm 或 stripped so）"
fi

echo
if [ "$FAILED" -gt 0 ]; then
  echo "✗ 自检失败：$FAILED 项" >&2
  exit 1
fi
echo "✓ 鸿蒙版产物自检全部通过"
