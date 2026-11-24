#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "=== Building Akshar Release Artifacts ==="

DIST_DIR="$ROOT/dist"
mkdir -p "$DIST_DIR"
rm -f "$DIST_DIR"/*

# 1. Native release build (Linux x86_64 on this machine)
echo "Building target/release/akshar..."
cargo build --release

# Strip symbols to ensure <= 10MB footprint
strip target/release/akshar 2>/dev/null || true

SIZE_BYTES=$(stat -c%s target/release/akshar 2>/dev/null || stat -f%z target/release/akshar)
SIZE_MB=$(echo "scale=2; $SIZE_BYTES / 1048576" | bc)
echo "Built akshar binary: $SIZE_MB MB ($SIZE_BYTES bytes)"

if (( $(echo "$SIZE_MB > 10.0" | bc -l) )); then
  echo "ERROR: Release binary exceeded 10MB target! ($SIZE_MB MB)" >&2
  exit 1
fi

cp target/release/akshar "$DIST_DIR/akshar-linux-x86_64"
chmod +x "$DIST_DIR/akshar-linux-x86_64"

# 2. Package tarball with setup script and assets
TARBALL="akshar-v1.0.0-linux-x86_64.tar.gz"
echo "Creating archive $TARBALL..."
tar -czf "$DIST_DIR/$TARBALL" \
  -C target/release akshar \
  -C "$ROOT" scripts/setup.sh README.md LICENSE-APACHE LICENSE-MIT

# 3. Generate SHA256 Checksums
echo "Generating SHA256SUMS.txt..."
cd "$DIST_DIR"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum akshar-linux-x86_64 "$TARBALL" > SHA256SUMS.txt
else
  shasum -a 256 akshar-linux-x86_64 "$TARBALL" > SHA256SUMS.txt
fi

cat SHA256SUMS.txt
echo "=== Release build complete in $DIST_DIR ==="
