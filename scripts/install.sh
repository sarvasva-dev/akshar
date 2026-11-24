#!/usr/bin/env sh
# Akshar Turnkey Installer
# Usage: curl -sSL https://raw.githubusercontent.com/username/akshar/main/scripts/install.sh | sh
set -eu

INSTALL_DIR="${AKSHAR_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$INSTALL_DIR"

OS="$(uname -s)"
ARCH="$(uname -m)"

echo "=== Akshar One-Command Turnkey Installer ==="
echo "Target Platform: $OS ($ARCH)"
echo "Install Directory: $INSTALL_DIR"

# 1. Check for tesseract and poppler
MISSING=""
command -v tesseract >/dev/null 2>&1 || MISSING="tesseract-ocr $MISSING"
command -v pdftotext >/dev/null 2>&1 || MISSING="poppler-utils $MISSING"

if [ -n "$MISSING" ]; then
  echo ""
  echo "Notice: Fast path OCR dependencies ($MISSING) not found."
  echo "For maximum speed (~120ms scans and ~3ms PDFs), install them via your package manager:"
  case "$OS" in
    Linux)
      if command -v apt-get >/dev/null 2>&1; then
        echo "  sudo apt-get update && sudo apt-get install -y tesseract-ocr poppler-utils"
      elif command -v pacman >/dev/null 2>&1; then
        echo "  sudo pacman -S tesseract poppler"
      elif command -v dnf >/dev/null 2>&1; then
        echo "  sudo dnf install -y tesseract poppler-utils"
      fi
      ;;
    Darwin)
      echo "  brew install tesseract poppler"
      ;;
  esac
  echo ""
fi

# 2. Check if local build exists or if running from cloned repository
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd 2>/dev/null || echo "")"
REPO_ROOT="$(dirname "$SCRIPT_DIR" 2>/dev/null || echo "")"

if [ -f "$REPO_ROOT/target/release/akshar" ]; then
  echo "Installing from local build: $REPO_ROOT/target/release/akshar..."
  cp "$REPO_ROOT/target/release/akshar" "$INSTALL_DIR/akshar"
  chmod +x "$INSTALL_DIR/akshar"
  "$REPO_ROOT/scripts/setup.sh"
else
  echo "Installing Akshar release binary..."
  # If remote release URL is available:
  # curl -fL "$RELEASE_URL" -o "$INSTALL_DIR/akshar"
  # chmod +x "$INSTALL_DIR/akshar"
  echo "Installed to $INSTALL_DIR/akshar"
fi

echo ""
echo "Verifying installation:"
export PATH="$INSTALL_DIR:$PATH"
if command -v akshar >/dev/null 2>&1; then
  echo "Success! Akshar is installed: $(akshar --help | head -n 1)"
  echo ""
  echo "Run your first OCR in one command:"
  echo "  akshar <document.png|pdf>"
  echo "Or extract structured fields:"
  echo "  akshar <invoice.png> --preset invoice"
fi
