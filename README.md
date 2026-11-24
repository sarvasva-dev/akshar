<p align="center">
  <img src="assets/logo.svg" width="64" height="64" alt="Akshar logo" />
</p>

<h1 align="center">Akshar</h1>

<p align="center">
  Fast, local OCR, multi-page PDF text extraction, and folder watcher built in Rust.
</p>

<p align="center">
  <a href="https://github.com/sarvasva-dev/akshar/actions/workflows/ci.yml">
    <img src="https://github.com/sarvasva-dev/akshar/actions/workflows/ci.yml/badge.svg" alt="CI" />
  </a>
  <a href="LICENSE-APACHE">
    <img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg" alt="License: MIT / Apache-2.0" />
  </a>
  <img src="https://img.shields.io/badge/rust-edition%202024-orange.svg" alt="Rust" />
</p>

<p align="center">
  <img src="assets/ui-preview.png" alt="Akshar Web UI" width="840" />
</p>

---

**Akshar** extracts text from images and multi-page PDFs completely locally on CPU — no GPU, no cloud API keys, and no data leaving your machine. It compiles to a single **~8.4 MB** Rust binary backed by a **~21 MB** quantized vision model.

## Highlights

- **100% local on CPU** — runs offline in a single Rust binary (`axum` + `tokio` + direct C ABI binding).
- **Full-page tiled OCR** — slices tall/dense documents into overlapping strips and deduplicates boundary boxes to bypass single-pass line caps (`40/40` lines at `99.91%` character accuracy in testing).
- **Multi-page PDFs** — rasterizes and extracts text page-by-page using `pdftoppm`.
- **Smart auto routing** — switches automatically between standard OCR, full-page tiled OCR for tall documents, and photo scene/object description when an image contains no text.
- **Structured field extraction** — extract typed JSON fields using built-in presets (`invoice`, `receipt`, `id_card`, `business_card`) or custom JSON schemas.
- **Folder watcher & webhooks** — monitor an inbox directory continuously, export `.json`, `.txt`, `.md`, and `summary.csv` into an outbox, archive processed files, and optionally trigger HTTP webhooks.

## Quickstart

### 1. Download runtime library and model weights (~22 MB, SHA-256 verified)

```sh
sh scripts/setup.sh
```

### 2. Build

```sh
cargo build --release
```

### 3. Run the Web UI and API

```sh
./target/release/akshar --serve
```

Open **http://127.0.0.1:8000** to drop images or PDFs, paste from clipboard (`Ctrl+V`), or configure folder automation from the **Settings & Watch** drawer.

> PDF rasterization uses `pdftoppm` (`poppler-utils` on Debian/Ubuntu, `poppler` on Arch/macOS).

## CLI Usage

```sh
# Single image or multi-page PDF
./target/release/akshar invoice.png
./target/release/akshar report.pdf --json

# Force full-page tiled OCR on a tall document
./target/release/akshar statement.png --tiled

# Structured form extraction using a built-in preset
./target/release/akshar receipt.jpg --preset receipt

# Batch-process an entire folder once
./target/release/akshar ./docs/ --out ./outbox --formats json,txt,md

# Continuously watch an inbox directory (with optional webhook)
./target/release/akshar --watch ./inbox --out ./outbox --webhook https://example.com/hook
```

## HTTP API

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` | `/` | Web UI |
| `GET` | `/v1/health` | Service readiness & loaded model |
| `GET` | `/v1/config` | App settings (`config.json`) |
| `GET` | `/v1/presets` | Built-in structured form schemas |
| `POST` | `/v1/auto` | Automatic OCR / tiled OCR / PDF / photo description |
| `POST` | `/v1/ocr` | Single-pass or tiled OCR (`tiled=true`) |
| `POST` | `/v1/extract` | Constrained JSON-schema field extraction |
| `POST` | `/v1/describe` | Full scene description, objects, and OCR |
| `POST` | `/v1/batch` | Multi-file image + PDF batch processing |
| `GET` | `/v1/automation/status` | Folder watcher state and job log |
| `POST` | `/v1/automation/start` | Start or update folder watcher |
| `POST` | `/v1/automation/stop` | Stop folder watcher |
| `POST` | `/v1/automation/scan-now` | Run an immediate inbox scan |

Example request:

```sh
curl -F "file=@invoice.png" -F "mode=auto" http://127.0.0.1:8000/v1/auto
```

## Docker

```sh
docker build -t akshar .
docker run -p 8000:8000 akshar
```

## Architecture

```
Image / PDF
   │
   ├── PDF ──► pdftoppm (page rasterizer) ──┐
   │                                        ▼
   └── Image ──► Smart Router ──► Standard OCR / Tiled OCR (overlap + IoU/containment dedup)
                                      │
                                      ├── text found ──► Reading-order text + boxes + JSON/TXT/MD/CSV
                                      └── 0 text lines ─► Scene & object description fallback
```

## Configuration

Settings live in `config.json`, with optional environment overrides:

- `AKSHAR_CONFIG` — path to `config.json`
- `AKSHAR_LIB_PATH` — path to native shared library
- `AKSHAR_MODEL_PATH` — path to `.cact` model weights
- `AKSHAR_THREADS` — inference thread count (`0` = auto)

## Tests

```sh
# Fast unit tests (no model download required)
cargo test --lib

# Full end-to-end suite (OCR, 40-line tiling, 2-page PDF, folder watcher + webhook, HTTP routes)
cargo test --release -- --test-threads=1
```

## License & Attribution

Dual-licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for third-party model and runtime attributions.
