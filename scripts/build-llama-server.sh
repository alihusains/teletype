#!/usr/bin/env bash
# Build llama-server (Metal on, no curl/OpenSSL) and install it where the
# desktop app and Tauri bundle expect it.
#
# Cross-compiling to Intel from an Apple Silicon host is supported. Set
# TELETYPE_TARGET_TRIPLE=x86_64-apple-darwin and the build directory, the CMake
# architecture, the C/C++ flags and the CPU baseline all follow the target. The
# C and C++ compilers are clang on both sides, so only -arch differs; the flags
# are exported here rather than left to the caller, because a mismatch between
# the C++ objects and the Rust link is a link error several minutes in.
#
# There is exactly one output path, crates/teletype-desktop/binaries/llama-server,
# and it is not arch-specific: tauri.conf.json maps that one extensionless path
# and the bundle resolves it, so the Intel app needs the x86_64 binary at the
# same place the arm64 app needs its arm64 one. In CI that is fine, because each
# job gets a fresh checkout. Locally, a cross-build therefore replaces the
# native binary, so the script says so loudly at the end rather than leaving you
# to discover it as "why is my dev build suddenly a foreign executable".
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TRIPLE="${TELETYPE_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}"
LLAMA_DIR="${LLAMA_DIR:-/tmp/llama.cpp-teletype-$TRIPLE}"
BUILD_DIR="$LLAMA_DIR/build-teletype"
OUT_DIR="$ROOT/crates/teletype-desktop/binaries"
COMMIT="f95b0d95394d5e311ba8228689972843178c5e28"

# Deployment target floor. ggml-metal uses @available(macOS 15.0); building the
# C++ below 15.0 makes clang emit a strong reference to
# ___isPlatformVersionAtLeast that the final link cannot resolve.
DEPLOY_TARGET="${TELETYPE_DEPLOYMENT_TARGET:-15.0}"

case "$TRIPLE" in
  x86_64-apple-darwin|aarch64-apple-darwin) ;;
  *)
    echo "build-llama-server.sh only builds the macOS llama-server; got $TRIPLE" >&2
    echo "(Windows builds it inline in release.yml with the CPU backend.)" >&2
    exit 2
    ;;
esac

HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
# Rust and CMake call the Apple Silicon architecture `aarch64`; clang's -arch
# flag only accepts `arm64`. Passing the Rust name to clang gives
# "clang: error: invalid arch name '-arch aarch64'".
ARCH="${TRIPLE%%-*}"
CLANG_ARCH="$ARCH"
if [[ "$ARCH" == "aarch64" ]]; then
  CLANG_ARCH="arm64"
fi
export CC_${TRIPLE//-/_}=clang
export CXX_${TRIPLE//-/_}=clang++
export CFLAGS_${TRIPLE//-/_}="-arch $CLANG_ARCH -mmacosx-version-min=$DEPLOY_TARGET"
export CXXFLAGS_${TRIPLE//-/_}="-arch $CLANG_ARCH -mmacosx-version-min=$DEPLOY_TARGET"
# CMake turns CMAKE_OSX_ARCHITECTURES into `-arch <value>` verbatim, so it needs
# the clang spelling here too: with `aarch64` it emits `-arch aarch64` and every
# compile fails with "invalid arch name".
export CMAKE_OSX_ARCHITECTURES="$CLANG_ARCH"
export CMAKE_OSX_DEPLOYMENT_TARGET="$DEPLOY_TARGET"

echo "Building llama-server for $TRIPLE (arch $CLANG_ARCH, deployment target $DEPLOY_TARGET)"

# ggml's GGML_NATIVE probes the *host* CPU and bakes the answer into the compile
# flags. Cross-compiling from this Apple Silicon host, that writes
# `-mcpu=apple-m4` into an x86_64 compile, which clang rejects outright:
#   error: unknown target CPU 'apple-m4'
# ggml-metal itself cross-compiles fine; this is the CPU backend only. So for a
# cross-build, switch the probe off and name the baseline explicitly.
#
# Scoped to cross-builds on purpose: the arm64 build is a native build, its
# probe is correct, and changing its flags would change the shipped Apple
# Silicon binary for no reason.
CMAKE_EXTRA=()
if [[ "$TRIPLE" != "$HOST_TRIPLE" ]]; then
  CMAKE_EXTRA+=(-DGGML_NATIVE=OFF)
  case "$CLANG_ARCH" in
    x86_64)
      # The oldest Mac that runs macOS 15 is a 2018 model, so AVX2 is available
      # on every machine that can install this build. Leaving it off would be
      # correct but needlessly slow; leaving GGML_NATIVE on is not correct.
      CMAKE_EXTRA+=(-DGGML_AVX2=ON)
      ;;
    aarch64)
      # NEON is baseline on Apple Silicon, so there is nothing to ask for.
      ;;
  esac
  echo "Cross-building from $HOST_TRIPLE: ${CMAKE_EXTRA[*]:-no extra flags}"
fi

if [[ ! -d "$LLAMA_DIR/.git" ]]; then
  git clone --filter=blob:none https://github.com/ggml-org/llama.cpp "$LLAMA_DIR"
fi
git -C "$LLAMA_DIR" checkout --detach "$COMMIT"

# BUILD_SHARED_LIBS=OFF is required: the default shared build produces an
# llama-server that only runs via an LC_RPATH into this temp build dir, and
# tauri.conf.json bundles the single binary only (no sibling dylibs). Static
# linking yields one self-contained executable for dev and for the .app.
# LLAMA_OPENSSL=OFF drops the Homebrew libssl/libcrypto dylib deps: the app
# only ever calls this server over 127.0.0.1 HTTP and does its own model
# downloads, so HTTPS support here is unused and unbundleable.
# A CMakeCache.txt from an earlier run keeps flags that may no longer be right
# (a previous GGML_NATIVE probe, a different arch). A failed cross-build leaves
# -mcpu=apple-m4 cached, and a cached value survives a reconfigure, so the fix
# would appear not to work. Drop the cache before configuring, not after.
rm -f "$BUILD_DIR/CMakeCache.txt"
rm -rf "$BUILD_DIR/CMakeFiles"

cmake -B "$BUILD_DIR" -S "$LLAMA_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF \
  -DLLAMA_OPENSSL=OFF \
  -DLLAMA_BUILD_TOOLS=ON \
  -DLLAMA_BUILD_EXAMPLES=OFF \
  -DLLAMA_BUILD_APP=OFF \
  -DLLAMA_USE_PREBUILT_UI=OFF \
  -DLLAMA_CURL=OFF \
  -DGGML_METAL="${TELETYPE_GGML_METAL:-ON}" \
  -DCMAKE_OSX_ARCHITECTURES="$CLANG_ARCH" \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$DEPLOY_TARGET" \
  ${CMAKE_EXTRA[@]+"${CMAKE_EXTRA[@]}"}

# A prior prebuilt-UI extract can leave a broken tools/ui stamp; drop it.
rm -rf "$BUILD_DIR/tools/ui"


cmake --build "$BUILD_DIR" --target llama-server -j

mkdir -p "$OUT_DIR"

# Install by writing a temporary file and renaming it into place. Never `cp`
# over the destination directly.
#
# `cp` writes into the *existing* inode. When a llama-server is running from
# that path -- which is the normal case, because a dev loop leaves one holding
# a model between dictations -- overwriting the inode leaves a file macOS
# refuses to execute. The kill is `SIGKILL` with the reason
# `Code Signature Invalid`, it happens before the program writes a single line,
# so the server log is 0 bytes, and the app reports it as
# "llama-server exited early with signal: 9" and then tells the user
# "Polish skipped: no model loaded". Reproduced and confirmed: re-signing the
# same bytes makes it run again, and `cp`-ing onto a running binary
# reproduces it from a clean file.
#
# `mv` within a filesystem is a rename: the destination gets a new inode, the
# running process keeps the old one, and the path is immediately executable.
install_binary() {
  local src="$1" dest="$2"
  local tmp
  tmp="$(mktemp "${dest}.XXXXXX")"
  cp "$src" "$tmp"
  chmod +x "$tmp"
  mv -f "$tmp" "$dest"
}

install_binary "$BUILD_DIR/bin/llama-server" "$OUT_DIR/llama-server"

# Also drop a copy next to common dev binaries so PATH/exe discovery works
# without TELETYPE_LLAMA_SERVER in some setups. Only for a native build: a
# cross-built binary cannot run on this host, so copying it there would break
# the arm64 dev loop that this path exists to serve.
if [[ "$TRIPLE" == "$(rustc -vV | sed -n 's/^host: //p')" && -d "$ROOT/target/debug" ]]; then
  install_binary "$OUT_DIR/llama-server" "$ROOT/target/debug/llama-server"
fi

# The bundled placeholder is 74 bytes. If that is what we are looking at, the
# build produced nothing and every user who selects a local model gets
# "llama-server binary not found" instead of a working app. Fail here instead.
size=$(wc -c < "$OUT_DIR/llama-server" | tr -d ' ')
if [[ "$size" -lt 1000000 ]]; then
  echo "ERROR: $OUT_DIR/llama-server is $size bytes, so the build produced no" >&2
  echo "       real binary. Refusing to leave the placeholder in place." >&2
  exit 1
fi

echo "Installed $OUT_DIR/llama-server ($size bytes, $TRIPLE)"
file "$OUT_DIR/llama-server"

# Prove the installed file can actually be executed, rather than trusting that
# the install worked. A binary that will not exec is exactly the failure this
# script previously caused, and `file` happily reports a perfectly good Mach-O
# either way. A zero-byte server log and "no model loaded" in the app are the
# only symptoms, and they name neither this script nor this line.
if ! out=$("$OUT_DIR/llama-server" --version 2>&1); then
  cat >&2 <<EOF

ERROR: the freshly installed llama-server cannot be executed (exit $?):
$out

       The most likely cause is a stale executable inode: something replaced
       this file's contents while a llama-server was running from it, which
       leaves a file macOS refuses to exec (SIGKILL, "Code Signature
       Invalid"). The install above renames into place to avoid that, so this
       means the destination was already in that state.

       Fix it with:
           codesign --force --sign - $OUT_DIR/llama-server
       and check for a llama-server still running from an old build:
           pgrep -fl llama-server
EOF
  exit 1
fi
echo "$out"

# A cross-build replaced a binary the host could not have produced. Say so,
# with the command that undoes it, because the symptom otherwise shows up much
# later as a dev build that refuses to run.
if [[ "$TRIPLE" != "$HOST_TRIPLE" ]]; then
  cat >&2 <<EOF

WARNING: that binary is for $TRIPLE, but this host is $HOST_TRIPLE.
         It replaced the native llama-server at $OUT_DIR/llama-server, so a
         native 'cargo tauri dev' on this machine will now fail to exec it.
         To get the native one back:
             ./scripts/build-llama-server.sh
EOF
fi
