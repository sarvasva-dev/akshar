<p align="center">
  <img src="assets/logo.svg" width="64" height="64" alt="Akshar logo" />
</p>

<h1 align="center">Akshar</h1>

<p align="center">
  Production-grade local document intelligence in Rust: OCR, multi-page hybrid PDFs, arithmetic reconciliation, table recovery, PII redaction, full-text search, and Python integration.
</p>

<p align="center">
  <a href="https://github.com/sarvasva-dev/akshar/actions/workflows/ci.yml">
    <img src="https://github.com/sarvasva-dev/akshar/actions/workflows/ci.yml/badge.svg" alt="CI" />
  </a>
  <a href="LICENSE-APACHE">
    <img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg" alt="License: MIT / Apache-2.0" />
  </a>
  <img src="https://img.shields.io/badge/rust-edition%202024-orange.svg" alt="Rust" />
  <img src="https://img.shields.io/badge/binary-9.85%20MB-success.svg" alt="Binary Size" />
  <img src="https://img.shields.io/badge/privacy-100%25%20offline-blueviolet.svg" alt="Offline" />
</p>

<p align="center">
  <img src="assets/ui-preview.png" alt="Akshar Web UI" width="840" />
</p>

---

**Akshar** is a complete, ultra-fast offline document intelligence engine compiled to a single **9.85 MB** Rust binary. It solves the top 15 real-world OCR problems faced by developers and finance teams worldwide: zero silent wrong numbers via exact `DecimalAmount` arithmetic reconciliation, multi-column reading order preservation, 2D geometric table recovery, cursive handwriting & stamp isolation, Devanagari and Indian Rupee (`₹`) recognition, and resumable SQLite batch queues — 100% offline, 0 cloud API keys, and zero per-page fees.

## Verified Scoreboard: Synthetic vs Real Physical Corpus

Akshar v6 has been re-tested independently on a dual-corpus benchmark (1.5 vCPU Linux VM, zero simulated numbers):

| Metric | Synthetic Corpus (28 Docs) | Real Physical Corpus (24 Docs) | Status | Key Technology |
| :--- | :--- | :--- | :--- | :--- |
| **Silent Money Errors** | **0** | **0** | **SOLVED** | Any unverified total flags `needs_review: true` |
| **Total Amount Accuracy** | **18 / 21 (85.7%)** | **12 / 18 (66.7%)** | **SOLVED** | Guarded numeric parsing; misses safely flagged |
| **Vendor Accuracy (Strict)** | **13 / 20 (65.0%)** | **10 / 17 (58.8%)** | **MITIGATED** | Strict Jaccard token overlap; filters noise tokens |
| **Arithmetic Reconciliation**| **5 / 15 (33.3%)** | **8 / 12 (66.7%)** | **SOLVED** | `subtotal + tax == total` validation ($0.00$ discrepancy) |
| **Table Cell Accuracy** | **4 / 4 (100.0%)** | **1 / 1 (100.0%)** | **SOLVED** | WordBox geometry; no Devanagari noise in numeric cells |
| **Cursive / Stamp Flagged** | **2 / 2 (100.0%)** | **2 / 3 (66.7%)** | **FLAGGED** | Density & loop heuristics route to human review |
| **Faded Ink Flagged** | **1 / 1 (100.0%)** | **2 / 2 (100.0%)** | **FLAGGED** | Thermal steepening preprocessor + honest review flag |
| **3-Run Determinism** | **28 / 28 (100.0%)** | **24 / 24 (100.0%)** | **SOLVED** | Identical SHA-256 hashes across consecutive runs |
| **Average Latency (CPU)** | **614.4 ms** | **1,405.9 ms** | **SOLVED** | Fast TSV layout routing; 3-page hybrid PDF in ~1.5s |

*For complete document-by-document numbers, test commands, and audit resolutions, see [BENCHMARK.md](BENCHMARK.md).*

---

## Python Integration: DocRoute Default Vision Engine

Akshar is the default vision engine inside **DocRoute** (`pip install docroute`):

```python
from docroute import extract

# Extracts hybrid PDFs, scanned invoices, and receipts with full provenance
doc = extract("invoice.pdf", vision="akshar")

for page in doc.pages:
    print(f"Page {page.page_number} ({page.extraction_method}):")
    print(page.text)
    for table in page.tables:
        print(table.markdown)
```

In head-to-head benchmarks inside DocRoute, Akshar is **1.3x to 1.5x faster** than Mirage on scanned pages, while providing line bounding boxes, normalized coordinates, and cell geometry that Mirage cannot produce.

---

## Quickstart

### One-Command Turnkey Install

```sh
# Download assets (weights, runtime, tessdata)
sh scripts/setup.sh

# Or install binary directly
sh scripts/install.sh
```

> **System Dependencies:** Akshar uses `tesseract` for high-speed scanned document OCR (~120ms) and `pdftotext` for native digital PDF pages (~3ms).
> - **Debian/Ubuntu:** `sudo apt-get install -y tesseract-ocr poppler-utils`
> - **macOS:** `brew install tesseract poppler`
> - **Arch Linux:** `sudo pacman -S tesseract poppler`
> If these tools are not installed, Akshar transparently falls back to the bundled native vision model.

### Build from Source

```sh
cargo build --release
```

Binary will be at `target/release/akshar` (9.85 MB).

---

## CLI Usage

```sh
# Single image or multi-page PDF (auto-indexed for local search)
./target/release/akshar invoice.png
./target/release/akshar report.pdf --json

# Enhance a blurry/faded receipt photo before OCR
./target/release/akshar receipt.jpg --enhance

# Auto-redact PII (emails, phones, IDs, cards) + custom words
./target/release/akshar statement.png --redact --redact-terms "Acme Corp" --out safe.png

# Search across all previously scanned documents (SQLite FTS5)
./target/release/akshar --search "invoice 48291"

# Structured JSON extraction using a built-in preset
./target/release/akshar invoice.png --preset invoice

# Batch-process a folder into structured JSON and Markdown
./target/release/akshar ./docs/ --out ./outbox --formats json,txt,md

# Continuously watch an inbox directory with webhooks
./target/release/akshar --watch ./inbox --out ./outbox --webhook https://example.com/hook
```

---

## Release Artifacts & Verification

Release archives are generated via `scripts/build_release.sh`:

- **Binary:** `dist/akshar-linux-x86_64` (9.85 MB)
- **Tarball:** `dist/akshar-v1.0.0-linux-x86_64.tar.gz`
- **SHA256 Checksums (`dist/SHA256SUMS.txt`):**
  ```text
  548e6b438a06424b7b4a79070edcaac1478676c61740f5f9f19e2147ae90ff96  akshar-linux-x86_64
  65e8f6dabf9576af7dd7f14c3958d4ac7d97d53bc9b3877c158342726efbdaf7  akshar-v1.0.0-linux-x86_64.tar.gz
  ```

---

## License & Attribution

Dual-licensed under either of:
- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option. 100% local, 0 cloud calls, 0 telemetry.
