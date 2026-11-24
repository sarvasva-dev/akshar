<p align="center">
  <img src="assets/logo.svg" width="64" height="64" alt="Akshar logo" />
</p>

<h1 align="center">Akshar</h1>

<p align="center">
  Local document intelligence in Rust: OCR, multi-page PDFs, PII redaction, table/entity extraction, full-text search, and folder automation.
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

**Akshar** is a complete, ultra-fast offline document intelligence engine compiled to a single **9.8 MB** Rust binary. It solves the top 15 real-world OCR problems faced by developers and finance teams worldwide: zero silent wrong numbers via exact `DecimalAmount` arithmetic reconciliation, multi-column reading order preservation, 2D geometric table recovery, cursive handwriting & stamp isolation, Devanagari and Indian Rupee (`₹`) recognition, and resumable SQLite batch queues — 100% offline, 0 cloud API keys, and zero per-page fees.

> 🚀 **v5 Benchmark Highlights:** Tested across a 28-document authentic real-world corpus: **0 silent money errors**, **21/21 (100%)** invoice totals reconciled or flagged, **100% byte-exact determinism** across 3 runs, and **~580 ms** average CPU latency on 1.5 vCPU. See full verified numbers in [BENCHMARK.md](BENCHMARK.md).

## What Akshar Solves Beyond Traditional OCR

- **Zero Silent Money Errors (Arithmetic Reconciliation)** — stores money as exact cents (`i64`), audits character look-alikes (`O`→`0`, `l`→`1`), dynamically identifies `US_IN` vs `EU` decimal locales, and mathematically validates `subtotal + tax == total`. Mismatches or low-confidence entries are surfaced directly to a human review queue (`needs_review: true`).
- **Review Queue API & Audit Evidence (`/v1/review`)** — every extracted field includes pixel bounding boxes (`bbox`), normalized coordinates, extraction confidence, and engine provenance. Flagged documents are tracked in local SQLite review tables.
- **Blazing Fast Hybrid Execution (318 ms vs 35 s)** — digital PDF pages run in ~3 ms via `pdftotext -bbox-layout`, scanned pages process in ~150–200 ms with word-level bounding boxes, and complex natural scene photos seamlessly fall back to Mirage vision.
- **Multi-Column Reading Order Preservation** — detects central vertical gutters on academic papers and multi-column documents, guaranteeing the left column is completely read before the right column begins (no line interleaving).
- **Cursive Handwriting, Checkboxes & Stamp Isolation** — detects checkbox states (`[X]` checked vs `[ ]` unchecked), segments red/blue seals (e.g. customs stamps) so ink doesn't corrupt numbers, and flags cursive handwriting regions for human verification instead of hallucinating text.
- **Faded Ink Detection** — detects physically degraded or vanished thermal lines, returning an honest `not_visible` status rather than fabricating digits.
- **Resumable Large-Scale Batch Queue (`src/batch.rs`)** — ACID SQLite queue for 1,000+ page archives that supports pause, system crash recovery, and thread-safe cancellation without lost progress.
- **₹ (Rupee) & Devanagari Recognition** — resolves the hard vocabulary limit of Mirage (which lacks ₹ and Devanagari in its CTC reader) with context-aware currency repair and `hin+eng` confidence fusion.
- **2D Geometric Table Recovery** — automatically clusters word/line coordinates into clean column matrices (`headers`, `rows`, `cells` with pixel bounding boxes, and Markdown tables).
- **Deterministic Page Classification** — classifies each page (`invoice`, `receipt`, `table`, `id_card`, `letter`, `form`, `photo`, `document`, `other`) with confidence and explainable signals.
- **Auto-PII Redaction (`--redact` / `/v1/redact`)** — detects emails, phone numbers, Aadhaar/PAN IDs, card numbers, and custom keywords, paints solid-black or pixelated boxes onto the image, and returns a safe PNG + masked text.
- **Built-in SQLite FTS5 Search (`--search` / `Ctrl+K`)** — automatically indexes every scanned image, PDF, and watched document locally (`data/akshar.db`) for instant full-text search with snippets.
- **Live Folder Watcher & Webhooks** — monitors an inbox folder continuously, exports `.json`, `.txt`, `.md`, and `summary.csv`, archives processed files, and optionally triggers HTTP webhooks.

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

Open **http://127.0.0.1:8000** — drop images or PDFs, press `Ctrl+V` to paste, press `Ctrl+K` to search all past scans, click **Redact PII** to black out sensitive info, or open **Settings & Watch** for folder automation.

> PDF rasterization uses `pdftoppm` (`poppler-utils` on Debian/Ubuntu, `poppler` on Arch/macOS).

## CLI Usage

```sh
# Single image or multi-page PDF (auto-indexed for local search)
./target/release/akshar invoice.png
./target/release/akshar report.pdf --json

# Enhance a blurry/dark receipt photo before OCR
./target/release/akshar receipt.jpg --enhance

# Auto-redact PII (emails, phones, IDs, cards) + custom words
./target/release/akshar statement.png --redact --redact-terms "Acme Corp" --out safe.png

# Search across all previously scanned documents (SQLite FTS5)
./target/release/akshar --search "invoice 48291"

# Force full-page tiled OCR on a long document
./target/release/akshar statement.png --tiled

# Structured JSON extraction using a built-in preset
./target/release/akshar receipt.jpg --preset receipt

# Batch-process a folder once
./target/release/akshar ./docs/ --out ./outbox --formats json,txt,md

# Continuously watch an inbox directory (with optional webhook)
./target/release/akshar --watch ./inbox --out ./outbox --webhook https://example.com/hook
```

## HTTP API

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` | `/` | Web UI |
| `GET` | `/v1/health` | Service status, loaded model, indexed document count |
| `GET` | `/v1/config` | App settings (`config.json`) |
| `GET` | `/v1/presets` | Built-in structured form schemas |
| `POST` | `/v1/auto` | Automatic OCR / tiled OCR / PDF / photo description + entities |
| `POST` | `/v1/ocr` | Single-pass or tiled OCR (`tiled=true`, `enhance=true`) |
| `POST` | `/v1/redact` | PII auto-redaction returning redacted PNG (base64) + report |
| `POST` | `/v1/extract` | Constrained JSON-schema field extraction |
| `POST` | `/v1/describe` | Full scene description, objects, and OCR |
| `POST` | `/v1/batch` | Multi-file image + PDF batch processing |
| `GET` | `/v1/search?q=...` | Full-text SQLite FTS5 search over indexed documents |
| `DELETE` | `/v1/search/{id}` | Remove a document from the search index |
| `GET` | `/v1/review` | Retrieve documents flagged for human review (audit queue) |
| `DELETE` | `/v1/review/{id}` | Resolve a flagged item in the review queue |
| `GET` | `/v1/automation/status` | Folder watcher state and job log |
| `POST` | `/v1/automation/start` | Start or update folder watcher |
| `POST` | `/v1/automation/stop` | Stop folder watcher |
| `POST` | `/v1/automation/scan-now` | Run an immediate inbox scan |

## Docker

```sh
docker build -t akshar .
docker run -p 8000:8000 akshar
```

## Architecture

```
Image / PDF
   │
   ├── (optional) Pre-Enhancer ──► Auto-Contrast + Unsharp Mask
   ├── PDF ──► pdftoppm (page rasterizer) ──┐
   │                                        ▼
   └── Image ──► Smart Router ──► Standard / Tiled OCR (overlap + IoU/containment dedup)
                                      │
                                      ├── Entity & Table Engine ──► emails, amounts, dates, Markdown tables
                                      ├── PII Redactor ──────────► black/pixelated safe PNG + masked text
                                      ├── SQLite FTS5 Index ─────► instant Ctrl+K / --search archive
                                      └── 0 text lines ──────────► Scene & object description fallback
```

## Tests

```sh
# Fast unit tests (tiling, entity extraction, PII redaction, contrast enhancement, FTS5 search)
cargo test --lib

# Full end-to-end suite (OCR, 40-line tiling, 2-page PDF, PII redaction, folder watcher + webhook, HTTP routes)
cargo test --release -- --test-threads=1
```

## License & Attribution

Dual-licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for third-party model and runtime attributions.
