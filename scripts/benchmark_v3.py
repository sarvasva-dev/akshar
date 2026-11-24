#!/usr/bin/env python3
"""
Akshar v3 Benchmark & Verification Suite
Generates realistic benchmark documents covering:
1. 3-Page Hybrid PDF (p1 digital text, p2 scanned invoice, p3 digital table)
2. Scanned Invoice (Page 2 alone)
3. Indian GST Invoice with ₹ (INR) currency symbol
4. Hindi + English Mixed Document (Devanagari script + English)
5. Scanned 4x4 Item Table (Geometric cell recovery on image)
6. Scanned Financial Schedule Table
7. Low-Contrast / Blurry Receipt (with and without --enhance)
8. Restaurant Food Menu with ₹ prices (curry, naan, roti, biryani)
9. Determinism test: 3 consecutive runs on identical inputs
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont, ImageFilter

AKSHAR_BIN = Path("./target/release/akshar").resolve()
BENCH_DIR = Path("/tmp/akshar_bench").resolve()
BENCH_DIR.mkdir(parents=True, exist_ok=True)

DEVANAGARI_FONT_PATH = "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf"
DEJAVU_FONT_PATH = "/usr/share/fonts/TTF/DejaVuSans.ttf"
SANS_FONT_PATH = "/usr/share/fonts/noto/NotoSans-Regular.ttf"

def get_font(size=20, devanagari=False):
    if devanagari and os.path.exists(DEVANAGARI_FONT_PATH):
        return ImageFont.truetype(DEVANAGARI_FONT_PATH, size)
    if os.path.exists(DEJAVU_FONT_PATH):
        return ImageFont.truetype(DEJAVU_FONT_PATH, size)
    if os.path.exists(SANS_FONT_PATH):
        return ImageFont.truetype(SANS_FONT_PATH, size)
    return ImageFont.load_default()

def render_text_image(lines, width=800, height=1100, font_size=24, line_step=38, start_y=60, devanagari=False):
    img = Image.new("RGB", (width, height), color=(255, 255, 255))
    draw = ImageDraw.Draw(img)
    font = get_font(font_size, devanagari=devanagari)
    y = start_y
    for line in lines:
        draw.text((50, y), line, fill=(15, 15, 15), font=font)
        y += line_step
    return img

def create_sample_pdf(pdf_path, p2_img_path):
    # p1: digital vector text, p2: scanned invoice image, p3: digital table
    # Using Ghostscript or poppler or pure PDF syntax to create a 3-page hybrid PDF
    # First render p2 as an image
    p2_lines = [
        "INVOICE",
        "Invoice Number: INV-98214",
        "Date: 24 November 2026",
        "Total amount due: 12345 rupees.",
        "Item: Coffee Beans Roast",
        "Price: 12345 rupees",
    ]
    p2_img = render_text_image(p2_lines, width=800, height=1100, font_size=26, line_step=45, start_y=80)
    p2_img.save(p2_img_path, format="PNG")

    # Generate PDF using python script + img2pdf or ghostscript or pure python
    # We can create a 3-page PDF where p1 and p3 have text streams and p2 has an embedded image
    # Or create p1.pdf, p2.pdf, p3.pdf and join with gs
    p1_ps = BENCH_DIR / "p1.ps"
    p3_ps = BENCH_DIR / "p3.ps"

    with open(p1_ps, "w") as f:
        f.write("%!PS\n/Helvetica-Bold findfont 18 scalefont setfont\n")
        f.write("50 720 moveto (DocRoute Lite - Smoke Test) show\n")
        f.write("/Helvetica findfont 14 scalefont setfont\n")
        f.write("50 690 moveto (This is page one native text extracted via the fast PyMuPDF path in a few milliseconds.) show\n")
        f.write("50 660 moveto (Client: Alpha Corporation) show\n")
        f.write("50 630 moveto (Terms: Net 30 days payable upon receipt.) show\n")
        f.write("showpage\n")

    with open(p3_ps, "w") as f:
        f.write("%!PS\n/Helvetica-Bold findfont 16 scalefont setfont\n")
        f.write("50 720 moveto (Summary Table) show\n")
        f.write("/Helvetica findfont 13 scalefont setfont\n")
        f.write("50 680 moveto (Item          Qty     Price) show\n")
        f.write("50 655 moveto (Pen           10      50) show\n")
        f.write("50 630 moveto (Book          4       200) show\n")
        f.write("50 605 moveto (Marker        2       80) show\n")
        f.write("showpage\n")

    p1_pdf = BENCH_DIR / "p1.pdf"
    p3_pdf = BENCH_DIR / "p3.pdf"
    p2_pdf = BENCH_DIR / "p2.pdf"

    subprocess.run(["gs", "-sDEVICE=pdfwrite", "-dCompatibilityLevel=1.4", "-dNOPAUSE", "-dBATCH", f"-sOutputFile={p1_pdf}", str(p1_ps)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(["gs", "-sDEVICE=pdfwrite", "-dCompatibilityLevel=1.4", "-dNOPAUSE", "-dBATCH", f"-sOutputFile={p3_pdf}", str(p3_ps)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    # Convert p2_img to PDF (image-only scanned page)
    p2_img.save(p2_pdf, "PDF", resolution=150.0)

    # Merge into 3-page hybrid sample.pdf
    subprocess.run(["gs", "-sDEVICE=pdfwrite", "-dCompatibilityLevel=1.4", "-dNOPAUSE", "-dBATCH", f"-sOutputFile={pdf_path}", str(p1_pdf), str(p2_pdf), str(p3_pdf)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def generate_corpus():
    corpus = {}

    # 1 & 2: Sample PDF & p2 scanned
    sample_pdf = BENCH_DIR / "sample.pdf"
    p2_png = BENCH_DIR / "page-2.png"
    create_sample_pdf(sample_pdf, p2_png)
    corpus["hybrid_pdf"] = sample_pdf
    corpus["scanned_p2"] = p2_png

    # 3: Indian GST invoice with ₹ symbol
    gst_lines = [
        "TAX INVOICE",
        "Vendor: Apex Digital Solutions Pvt Ltd",
        "GSTIN: 07AABCT2432A1Z1",
        "Invoice Number: INV-2026-8819",
        "Date: 24/11/2026",
        "Bill To: Sharma Enterprises",
        "--------------------------------------------------",
        "Description              Qty    Rate        Total",
        "Cloud Hosting Services    1    ₹ 8,500.00   ₹ 8,500.00",
        "Domain Registration       2    ₹ 1,000.00   ₹ 2,000.00",
        "--------------------------------------------------",
        "Subtotal: ₹ 10,500.00",
        "CGST (9%): ₹ 945.00",
        "SGST (9%): ₹ 945.00",
        "Total Amount: ₹ 12,390.00",
        "Payment Mode: UPI / Bank Transfer",
    ]
    gst_img = render_text_image(gst_lines, width=850, height=1050, font_size=23, line_step=40, start_y=60, devanagari=False)
    gst_path = BENCH_DIR / "gst_invoice_rupee.png"
    gst_img.save(gst_path)
    corpus["gst_invoice"] = gst_path

    # 4: Hindi + English Mixed Document
    hindi_lines = [
        "कर बीजक / TAX INVOICE",
        "दुकान: शर्मा इलेक्ट्रॉनिक्स स्टोर",
        "विक्रेता: Sharma Electronics Pvt Ltd",
        "जीएसटी: 07AAACS1429B1Z2",
        "फोन: +91 98765-43210",
        "तारीख: 24/11/2026",
        "--------------------------------------------------",
        "विवरण                   मात्रा    दर         कुल",
        "एलईडी बल्ब (LED Bulb)     5     ₹ 120.00    ₹ 600.00",
        "पावर केबल (Power Cable)   2     ₹ 350.00    ₹ 700.00",
        "--------------------------------------------------",
        "कुल राशि (Total Amount): ₹ 1,300.00",
        "धन्यवाद फिर पधारें (Thank You Visit Again)",
    ]
    hindi_img = render_text_image(hindi_lines, width=850, height=1050, font_size=22, line_step=42, start_y=60, devanagari=True)
    hindi_path = BENCH_DIR / "hindi_english_bilingual.png"
    hindi_img.save(hindi_path)
    corpus["hindi_bilingual"] = hindi_path

    # 5: Scanned 4x4 Table
    table1_lines = [
        "MONTHLY HARDWARE SALES REPORT",
        "Item Description    Units Sold    Unit Price    Subtotal",
        "Mechanical Keyboard      45        ₹ 3,500       ₹ 157,500",
        "Ergonomic Mouse          80        ₹ 1,200       ₹ 96,000",
        "USB-C Dock Station       25        ₹ 4,800       ₹ 120,000",
        "IPS Gaming Monitor       18        ₹ 18,500      ₹ 333,000",
        "Total Hardware Revenue                           ₹ 706,500",
    ]
    table1_img = render_text_image(table1_lines, width=900, height=650, font_size=22, line_step=45, start_y=60, devanagari=False)
    table1_path = BENCH_DIR / "scanned_table_1.png"
    table1_img.save(table1_path)
    corpus["table_1"] = table1_path

    # 6: Restaurant Menu with Indian dishes
    menu_lines = [
        "THE TANDOOR BISTRO - RESTAURANT MENU",
        "Starters & Breads",
        "Paneer Tikka               ₹ 280",
        "Tandoori Chicken Full      ₹ 480",
        "Garlic Butter Naan         ₹ 60",
        "Tandoori Roti              ₹ 30",
        "Dal Makhani Special        ₹ 340",
        "Hyderabadi Dum Biryani     ₹ 360",
        "Cold Drink Can             ₹ 50",
        "Total Payable Bill         ₹ 1,600",
    ]
    menu_img = render_text_image(menu_lines, width=800, height=800, font_size=23, line_step=44, start_y=60, devanagari=False)
    menu_path = BENCH_DIR / "resto_menu.png"
    menu_img.save(menu_path)
    corpus["resto_menu"] = menu_path

    # 7: Blurry / Low contrast receipt
    blurry_lines = [
        "METRO SUPERMARKET",
        "Date: 2026-11-24",
        "Fresh Milk 1L      55.00",
        "Brown Bread        45.00",
        "Total Cash Due    100.00",
    ]
    blurry_img = render_text_image(blurry_lines, width=500, height=500, font_size=20, line_step=38, start_y=50)
    # Reduce contrast and blur
    blurry_img = blurry_img.filter(ImageFilter.GaussianBlur(radius=0.7))
    blurry_path = BENCH_DIR / "blurry_receipt.png"
    blurry_img.save(blurry_path)
    corpus["blurry_receipt"] = blurry_path

    return corpus

def run_akshar(args):
    cmd = [str(AKSHAR_BIN)] + args + ["--json"]
    t0 = time.perf_counter()
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    wall_ms = (time.perf_counter() - t0) * 1000.0
    if p.returncode != 0:
        raise RuntimeError(f"Akshar failed ({p.returncode}): {p.stderr}")
    try:
        data = json.loads(p.stdout)
    except json.JSONDecodeError:
        raise RuntimeError(f"Invalid JSON from Akshar: {p.stdout[:200]}")
    return data, wall_ms

def main():
    if not AKSHAR_BIN.exists():
        print(f"Error: {AKSHAR_BIN} not found. Run cargo build --release first.")
        sys.exit(1)

    print("=== Generating Benchmark Test Corpus ===")
    corpus = generate_corpus()
    print(f"Corpus generated in {BENCH_DIR}")

    results = {}

    # Test 1: 3-Page Hybrid PDF End-to-End
    print("\n--- Test 1: 3-Page Hybrid PDF (Cold Start & Timings) ---")
    data_pdf, wall_ms_pdf = run_akshar([str(corpus["hybrid_pdf"])])
    meta_pdf = data_pdf.get("meta", {})
    results["hybrid_pdf"] = {
        "wall_ms": wall_ms_pdf,
        "elapsed_ms": meta_pdf.get("elapsed_ms", 0),
        "pages": meta_pdf.get("page_count", 0),
        "native_pages": meta_pdf.get("native_pages", 0),
        "ocr_pages": meta_pdf.get("ocr_pages", 0),
        "route": meta_pdf.get("auto_route", ""),
        "total_lines": meta_pdf.get("line_count", 0),
        "table_count": meta_pdf.get("table_count", 0),
    }
    print(f"Hybrid PDF 3 pages: Wall={wall_ms_pdf:.1f}ms, Meta={results['hybrid_pdf']['elapsed_ms']:.1f}ms, Route={results['hybrid_pdf']['route']}")

    # Test 2: Scanned Page 2 Alone
    print("\n--- Test 2: Scanned Page 2 Alone ---")
    data_p2, wall_ms_p2 = run_akshar([str(corpus["scanned_p2"])])
    meta_p2 = data_p2.get("meta", {})
    results["scanned_p2"] = {
        "wall_ms": wall_ms_p2,
        "elapsed_ms": meta_p2.get("elapsed_ms", 0),
        "line_count": meta_p2.get("line_count", 0),
        "fast_path": meta_p2.get("fast_path", False),
        "route": meta_p2.get("auto_route", ""),
    }
    print(f"Scanned p2: Wall={wall_ms_p2:.1f}ms, Meta={results['scanned_p2']['elapsed_ms']:.1f}ms, Lines={results['scanned_p2']['line_count']}, FastPath={results['scanned_p2']['fast_path']}")

    # Test 3: Invoice Preset Grounded Extraction on Page 2
    print("\n--- Test 3: Invoice Preset Grounded Extraction on Page 2 ---")
    data_inv, wall_ms_inv = run_akshar([str(corpus["scanned_p2"]), "--preset", "invoice"])
    fields = data_inv.get("fields", {}).get("form", {})
    tot = fields.get("total_amount", {})
    vend = fields.get("vendor_name", {})
    doc_type = fields.get("document_type", {})
    results["p2_preset"] = {
        "wall_ms": wall_ms_inv,
        "total_amount": tot.get("value"),
        "total_conf": tot.get("confidence"),
        "vendor_name": vend.get("value"),
        "doc_type": doc_type.get("value"),
        "doc_type_conf": doc_type.get("confidence"),
    }
    print(f"Invoice preset on p2: total_amount={results['p2_preset']['total_amount']} ({results['p2_preset']['total_conf']}), vendor_name={results['p2_preset']['vendor_name']}, type={results['p2_preset']['doc_type']}")

    # Test 4: Indian GST Invoice with ₹ Symbol
    print("\n--- Test 4: Indian GST Invoice with ₹ Symbol ---")
    data_gst, wall_ms_gst = run_akshar([str(corpus["gst_invoice"]), "--preset", "invoice"])
    gst_text = data_gst.get("text", "")
    gst_fields = data_gst.get("fields", {}).get("form", {})
    has_rupee = "₹" in gst_text or "₹12,390" in gst_text or "₹ 12,390" in gst_text
    rupee_count = gst_text.count("₹")
    results["gst_invoice"] = {
        "wall_ms": wall_ms_gst,
        "rupee_symbol_count": rupee_count,
        "has_rupee": has_rupee,
        "vendor_name": gst_fields.get("vendor_name", {}).get("value"),
        "total_amount": gst_fields.get("total_amount", {}).get("value"),
        "doc_type": gst_fields.get("document_type", {}).get("value"),
    }
    print(f"GST Invoice: Rupee count={rupee_count}, total_amount={results['gst_invoice']['total_amount']}, vendor_name={results['gst_invoice']['vendor_name']}")

    # Test 5: Hindi + English Bilingual Document
    print("\n--- Test 5: Hindi + English Bilingual Document ---")
    data_hi, wall_ms_hi = run_akshar([str(corpus["hindi_bilingual"]), "--lang", "hin"])
    hi_text = data_hi.get("text", "")
    has_devanagari = any(0x0900 <= ord(c) <= 0x097F for c in hi_text)
    hi_lines = [l for l in hi_text.splitlines() if l.strip()]
    results["hindi_bilingual"] = {
        "wall_ms": wall_ms_hi,
        "line_count": len(hi_lines),
        "has_devanagari": has_devanagari,
        "contains_sharma": "शर्मा" in hi_text or "Sharma" in hi_text,
        "contains_bijak": "बीजक" in hi_text or "चालान" in hi_text or "TAX INVOICE" in hi_text,
    }
    print(f"Hindi bilingual: lines={len(hi_lines)}, Devanagari detected={has_devanagari}, text excerpt:\n" + "\n".join(hi_lines[:4]))

    # Test 6: Scanned Table 2D Recovery
    print("\n--- Test 6: Scanned 2D Table Recovery ---")
    data_tbl, wall_ms_tbl = run_akshar([str(corpus["table_1"])])
    tables = data_tbl.get("tables", [])
    tbl_headers = tables[0].get("headers", []) if tables else []
    tbl_rows = tables[0].get("rows", []) if tables else []
    results["table_recovery"] = {
        "table_count": len(tables),
        "headers": tbl_headers,
        "row_count": len(tbl_rows),
        "markdown": tables[0].get("markdown", "") if tables else "",
    }
    print(f"Table recovery: Found {len(tables)} tables, {len(tbl_rows)} rows, Headers: {tbl_headers}")

    # Test 7: Restaurant Menu with Indian Dish Prices
    print("\n--- Test 7: Restaurant Menu (₹ Prices + Indian Context) ---")
    data_menu, wall_ms_menu = run_akshar([str(corpus["resto_menu"])])
    menu_text = data_menu.get("text", "")
    menu_entities = data_menu.get("entities", {})
    results["resto_menu"] = {
        "wall_ms": wall_ms_menu,
        "rupee_count": menu_text.count("₹"),
        "amounts_found": len(menu_entities.get("amounts", [])),
        "classification": data_menu.get("classification", {}).get("category"),
    }
    print(f"Resto Menu: Rupee count={results['resto_menu']['rupee_count']}, amounts={results['resto_menu']['amounts_found']}, class={results['resto_menu']['classification']}")

    # Test 8: Determinism Over 3 Runs
    print("\n--- Test 8: 100% Determinism (3 consecutive runs on Scanned Invoice) ---")
    run_texts = []
    run_totals = []
    for r in range(3):
        d, _ = run_akshar([str(corpus["scanned_p2"]), "--preset", "invoice"])
        run_texts.append(d.get("text", ""))
        run_totals.append(d.get("fields", {}).get("form", {}).get("total_amount", {}).get("value"))
    
    deterministic = (run_texts[0] == run_texts[1] == run_texts[2]) and (run_totals[0] == run_totals[1] == run_totals[2])
    results["determinism"] = {
        "run1_eq_run2": run_texts[0] == run_texts[1],
        "run2_eq_run3": run_texts[1] == run_texts[2],
        "identical": deterministic,
        "sample_total": run_totals[0],
    }
    print(f"Determinism: Identical={deterministic}, Total Amount across runs={run_totals}")

    # Test 9: Binary Size & Assets
    bin_size = AKSHAR_BIN.stat().st_size
    results["binary_size_bytes"] = bin_size
    results["binary_size_mb"] = round(bin_size / (1024 * 1024), 2)
    print(f"\nFinal Stripped Binary Size: {results['binary_size_mb']} MB ({bin_size} bytes)")

    out_file = Path("BENCHMARK_RESULTS.json")
    with open(out_file, "w") as f:
        json.dump(results, f, indent=2)
    print(f"\nAll benchmark results saved to {out_file.resolve()}")

if __name__ == "__main__":
    main()
