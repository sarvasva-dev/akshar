#!/usr/bin/env python3
"""
Akshar v5 Benchmark Runner:
Runs full evaluation on the 28-document authentic corpus against ground_truth.json.
Measures all 15 real-world OCR problems solved / mitigated / flagged.
"""

import hashlib
import json
import os
import subprocess
import time
from pathlib import Path

CORPUS_DIR = Path("tests/data/corpus").resolve()
GT_FILE = CORPUS_DIR / "ground_truth.json"
AKSHAR_BIN = Path("./target/release/akshar").resolve()
BASELINE_V3_FILE = Path("BASELINE_V3_RESULTS.json").resolve()
OUTPUT_FILE = Path("V5_BENCHMARK_RESULTS.json").resolve()

def normalize_money(s):
    if not s:
        return ""
    # Normalize ₹/Rs/INR and formatting commas
    return str(s).replace(" ", "").replace(",", "").replace("₹", "₹").replace("Rs.", "₹").replace("INR", "₹")

def run_akshar(doc_path, preset=None, extra_args=None):
    cmd = [str(AKSHAR_BIN), str(doc_path), "--json"]
    if preset:
        cmd.extend(["--preset", preset])
    if extra_args:
        cmd.extend(extra_args)
    t0 = time.perf_counter()
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    wall_ms = (time.perf_counter() - t0) * 1000.0
    if p.returncode != 0:
        return {"error": p.stderr, "wall_ms": wall_ms, "raw_stdout": p.stdout}
    try:
        data = json.loads(p.stdout)
        data["wall_ms"] = wall_ms
        return data
    except Exception as e:
        return {"error": str(e), "wall_ms": wall_ms, "raw_stdout": p.stdout}

def main():
    with open(GT_FILE, "r") as f:
        ground_truth = json.load(f)

    baseline_v3 = {}
    if BASELINE_V3_FILE.is_file():
        with open(BASELINE_V3_FILE, "r") as f:
            baseline_v3 = json.load(f)

    print(f"=== Running Akshar v5 Benchmark across {len(ground_truth)} corpus documents ===")
    v5_results = {}

    total_latency_ms = 0.0
    count_docs = len(ground_truth)

    silent_errors_count = 0
    invoices_tested = 0
    invoices_reconciled_or_flagged = 0
    vendor_correct_count = 0
    vendor_tested_count = 0
    total_correct_count = 0
    total_tested_count = 0

    multi_col_correct = 0
    multi_col_tested = 0
    handwriting_detected_count = 0
    handwriting_tested = 0
    checkboxes_correct = 0
    checkboxes_tested = 0
    faded_flagged_count = 0
    faded_tested = 0
    table_count_total = 0

    all_deterministic = True

    for filename, gt in ground_truth.items():
        doc_path = CORPUS_DIR / filename
        if not doc_path.is_file():
            print(f"Skipping missing {filename}")
            continue

        doc_type = gt.get("doc_type", "document")
        preset = "invoice" if doc_type == "invoice" else ("receipt" if doc_type == "receipt" else None)
        extra_args = ["--enhance"] if "receipt" in filename or "faded" in filename else []

        # Run 1: Primary run
        res1 = run_akshar(doc_path, preset=preset, extra_args=extra_args)
        if "error" in res1:
            print(f"ERROR on {filename}: {res1['error']}")
            continue

        # Run 2 & 3: Determinism verification
        res2 = run_akshar(doc_path, preset=preset, extra_args=extra_args)
        res3 = run_akshar(doc_path, preset=preset, extra_args=extra_args)

        h1 = hashlib.sha256(json.dumps(res1.get("fields", res1.get("text")), sort_keys=True).encode()).hexdigest()
        h2 = hashlib.sha256(json.dumps(res2.get("fields", res2.get("text")), sort_keys=True).encode()).hexdigest()
        h3 = hashlib.sha256(json.dumps(res3.get("fields", res3.get("text")), sort_keys=True).encode()).hexdigest()
        deterministic = (h1 == h2 == h3)
        if not deterministic:
            all_deterministic = False

        total_latency_ms += res1["wall_ms"]
        pred_text = res1.get("text", "")
        pred_fields = res1.get("fields", {}).get("form", {})
        artifacts = res1.get("artifacts", {})
        validation = res1.get("validation") or {}
        needs_review = res1.get("needs_review", False)
        review_reasons = res1.get("review_reasons", [])
        tables = res1.get("tables", [])
        table_count_total += len(tables)

        # Field extraction
        pred_vendor = pred_fields.get("vendor_name", {}).get("value") or pred_fields.get("merchant", {}).get("value")
        pred_total = pred_fields.get("total_amount", {}).get("value") or pred_fields.get("total", {}).get("value")

        vendor_correct = None
        if "vendor_name" in gt:
            vendor_tested_count += 1
            expected_vendor = gt["vendor_name"]
            if pred_vendor and (expected_vendor.lower() in pred_vendor.lower() or pred_vendor.lower() in expected_vendor.lower()):
                vendor_correct = True
                vendor_correct_count += 1
            else:
                vendor_correct = False

        total_correct = None
        if "total_amount" in gt:
            total_tested_count += 1
            expected_total = normalize_money(gt["total_amount"])
            pred_norm = normalize_money(pred_total)
            if expected_total in pred_norm or pred_norm in expected_total:
                total_correct = True
                total_correct_count += 1
            else:
                total_correct = False

        # Silent Money Error Check: Total is wrong BUT not flagged for review
        if "total_amount" in gt:
            invoices_tested += 1
            if total_correct:
                invoices_reconciled_or_flagged += 1
            else:
                if needs_review:
                    invoices_reconciled_or_flagged += 1
                else:
                    silent_errors_count += 1
                    print(f"SILENT ERROR: {filename} total was wrong but not flagged for review!")

        # Multi-column reading order
        col_order_ok = None
        if gt.get("is_multi_column"):
            multi_col_tested += 1
            pos1 = pred_text.find("1. Introduction")
            pos2 = pred_text.find("2. Related Architectures")
            pos_c1_end = pred_text.find("guaranteeing correct left-to-right")
            col_order_ok = (pos1 != -1 and pos2 != -1 and pos1 < pos2) and (pos_c1_end != -1 and pos_c1_end < pos2)
            if col_order_ok:
                multi_col_correct += 1

        # Handwriting detection
        hw_detected = None
        if gt.get("has_handwriting"):
            handwriting_tested += 1
            hw_detected = artifacts.get("has_handwriting", False) or needs_review
            if hw_detected:
                handwriting_detected_count += 1

        # Checkboxes
        if "checkboxes" in gt:
            expected_chks = gt["checkboxes"]
            detected = res1.get("checkboxes", [])
            for chk_name, expected_bool in expected_chks.items():
                checkboxes_tested += 1
                found = any(
                    chk_name.lower() in c.get("label", "").lower() and c.get("checked") == expected_bool
                    for c in detected
                )
                if found:
                    checkboxes_correct += 1

        # Faded regions
        if gt.get("has_faded_line"):
            faded_tested += 1
            faded_ok = len(artifacts.get("faded_regions", [])) > 0 or needs_review
            if faded_ok:
                faded_flagged_count += 1

        v3_entry = baseline_v3.get(filename, {})
        v5_results[filename] = {
            "document_type": doc_type,
            "wall_ms": round(res1["wall_ms"], 2),
            "meta_elapsed_ms": res1.get("meta", {}).get("elapsed_ms", 0.0),
            "fast_path": res1.get("meta", {}).get("fast_path", False),
            "auto_route": res1.get("meta", {}).get("auto_route", ""),
            "deterministic_3runs": deterministic,
            "extracted_vendor": pred_vendor,
            "vendor_correct": vendor_correct,
            "extracted_total": pred_total,
            "total_correct": total_correct,
            "validation": validation,
            "needs_review": needs_review,
            "review_reasons": review_reasons,
            "multi_column_order_ok": col_order_ok,
            "handwriting_detected": hw_detected,
            "table_count": len(tables),
            "v3_wall_ms": v3_entry.get("wall_ms"),
            "v3_vendor_correct": v3_entry.get("vendor_correct"),
            "v3_total_correct": v3_entry.get("total_correct"),
            "v3_handwriting_detected": v3_entry.get("handwriting_detected"),
        }

        v_mark = "✓" if vendor_correct else ("✗" if vendor_correct is False else "-")
        t_mark = "✓" if total_correct else ("✗" if total_correct is False else "-")
        print(f"[{doc_type:10s}] {filename:32s} | Wall={res1['wall_ms']:5.1f}ms | Vendor={v_mark} | Total={t_mark} | Review={needs_review}")

    avg_latency = total_latency_ms / count_docs

    summary = {
        "total_documents": count_docs,
        "avg_latency_ms": round(avg_latency, 2),
        "silent_errors": silent_errors_count,
        "invoices_reconciled_or_flagged": f"{invoices_reconciled_or_flagged}/{invoices_tested}",
        "vendor_accuracy": f"{vendor_correct_count}/{vendor_tested_count}",
        "total_accuracy": f"{total_correct_count}/{total_tested_count}",
        "multi_column_integrity": f"{multi_col_correct}/{multi_col_tested}",
        "handwriting_detected": f"{handwriting_detected_count}/{handwriting_tested}",
        "checkboxes_accuracy": f"{checkboxes_correct}/{checkboxes_tested}",
        "faded_flagged_not_visible": f"{faded_flagged_count}/{faded_tested}",
        "total_tables_extracted": table_count_total,
        "determinism_100pct": all_deterministic,
        "documents": v5_results,
    }

    with open(OUTPUT_FILE, "w") as f:
        json.dump(summary, f, indent=2)

    print("\n" + "="*60)
    print("=== AKSHAR v5 BENCHMARK SCOREBOARD ===")
    print(f"Total Documents:                 {count_docs}")
    print(f"Average Latency:                 {avg_latency:.1f} ms")
    print(f"Silent Money Errors:             {silent_errors_count} (Goal: 0)")
    print(f"Invoices Reconciled or Flagged:  {invoices_reconciled_or_flagged}/{invoices_tested} ({invoices_reconciled_or_flagged/invoices_tested*100:.1f}%)")
    print(f"Vendor Name Accuracy:            {vendor_correct_count}/{vendor_tested_count} ({vendor_correct_count/vendor_tested_count*100:.1f}%)")
    print(f"Total Amount Accuracy:           {total_correct_count}/{total_tested_count} ({total_correct_count/total_tested_count*100:.1f}%)")
    print(f"Multi-Column Reading Order:      {multi_col_correct}/{multi_col_tested}")
    print(f"Handwriting Regions Detected:    {handwriting_detected_count}/{handwriting_tested}")
    print(f"Checkbox Extraction Accuracy:    {checkboxes_correct}/{checkboxes_tested}")
    print(f"Faded Lines Flagged not_visible: {faded_flagged_count}/{faded_tested}")
    print(f"Total Tables Extracted:          {table_count_total}")
    print(f"100% Deterministic (3 runs):     {all_deterministic}")
    print(f"Results saved to: {OUTPUT_FILE}")
    print("="*60)

if __name__ == "__main__":
    main()
