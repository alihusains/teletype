#!/usr/bin/env bash
# Build llama-server (Metal on, no curl/OpenSSL) and install it where the
# desktop app and Tauri bundle expect it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LLAMA_DIR="${LLAMA_DIR:-/tmp/llama.cpp-teletype}"
BUILD_DIR="$LLAMA_DIR/build-teletype"
OUT_DIR="$ROOT/crates/teletype-desktop/binaries"
COMMIT="f95b0d95394d5e311ba8228689972843178c5e28"

if [[ ! -d "$LLAMA_DIR/.git" ]]; then
  git clone --filter=blob:none https://github.com/ggml-org/llama.cpp "$LLAMA_DIR"
fi
git -C "$LLAMA_DIR" checkout --detach "$COMMIT"

cmake -B "$BUILD_DIR" -S "$LLAMA_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DLLAMA_BUILD_TOOLS=ON \
  -DLLAMA_BUILD_EXAMPLES=OFF \
  -DLLAMA_BUILD_APP=OFF \
  -DLLAMA_USE_PREBUILT_UI=OFF \
  -DLLAMA_CURL=OFF \
  -DGGML_METAL=ON

# A prior prebuilt-UI extract can leave a broken tools/ui stamp; drop it.
rm -rf "$BUILD_DIR/tools/ui"

cmake --build "$BUILD_DIR" --target llama-server -j

mkdir -p "$OUT_DIR"
cp "$BUILD_DIR/bin/llama-server" "$OUT_DIR/llama-server"
chmod +x "$OUT_DIR/llama-server"

# Also drop a copy next to common dev binaries so PATH/exe discovery works
# without TELETYPE_LLAMA_SERVER in some setups.
if [[ -d "$ROOT/target/debug" ]]; then
  cp "$OUT_DIR/llama-server" "$ROOT/target/debug/llama-server"
fi

echo "Installed $OUT_DIR/llama-server"
"$OUT_DIR/llama-server" --version || true
