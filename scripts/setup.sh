#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
ENGINE_DIR="$ROOT/assets/engine"
MODEL_DIR="$ROOT/assets/models"
mkdir -p "$ENGINE_DIR" "$MODEL_DIR"

ENGINE_VER="4.1.4"
BASE_URL="https://huggingface.co/Cactus-Compute/cactus/resolve/main/releases/$ENGINE_VER/python"
MODEL_URL="https://huggingface.co/Cactus-Compute/mirage/resolve/main/mirage.cact"
MODEL_SHA="4249a0f7515272ff76b9414bec17d7e7daf84b15b66c7fb4f17fce1e66f9f237"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS:$ARCH" in
  Linux:x86_64)
    WHL="cactus_runtime-$ENGINE_VER-py3-none-manylinux2014_x86_64.whl"
    WHL_SHA="314cd027e95cae1cb8537c478ca6cddaec3fd02165f80d960f1934399871fc33"
    LIB="libcactus.so"
    ;;
  Linux:aarch64|Linux:arm64)
    WHL="cactus_runtime-$ENGINE_VER-py3-none-manylinux2014_aarch64.whl"
    WHL_SHA="45b703ee71fd979d3155f945a733c1dd1f18637035314e9c96fdfd60ae96b787"
    LIB="libcactus.so"
    ;;
  Darwin:arm64)
    WHL="cactus_runtime-$ENGINE_VER-py3-none-macosx_11_0_arm64.whl"
    WHL_SHA="45eaae74b37291425bc8bedfc3fa0ff6984cf8ea1997d2ea1711387974d25e43"
    LIB="libcactus.dylib"
    ;;
  Darwin:x86_64)
    WHL="cactus_runtime-$ENGINE_VER-py3-none-macosx_11_0_x86_64.whl"
    WHL_SHA="bb04050273486582ef8484bd5c95cba3a43cb2db4510970b1f97cc6432f464cd"
    LIB="libcactus.dylib"
    ;;
  *)
    echo "Unsupported platform: $OS $ARCH" >&2
    exit 1
    ;;
esac

verify_sha() {
  file="$1"
  expected="$2"
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
  else
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
  fi
  [ "$actual" = "$expected" ] || { echo "Checksum mismatch for $file" >&2; exit 1; }
}

if [ ! -f "$ENGINE_DIR/$LIB" ]; then
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  curl -fL "$BASE_URL/$WHL" -o "$TMP/$WHL"
  verify_sha "$TMP/$WHL" "$WHL_SHA"
  unzip -p "$TMP/$WHL" "cactus/$LIB" > "$ENGINE_DIR/$LIB"
  echo "Installed $ENGINE_DIR/$LIB"
fi

if [ ! -f "$MODEL_DIR/ocr_v1.cact" ]; then
  curl -fL "$MODEL_URL" -o "$MODEL_DIR/ocr_v1.cact.tmp"
  verify_sha "$MODEL_DIR/ocr_v1.cact.tmp" "$MODEL_SHA"
  mv "$MODEL_DIR/ocr_v1.cact.tmp" "$MODEL_DIR/ocr_v1.cact"
  echo "Installed $MODEL_DIR/ocr_v1.cact"
fi

echo "Assets ready."
