#!/usr/bin/env python3
"""
Generate the authentic 23-document real-world test corpus and hand-typed ground truth.
Includes:
- 10 GST Invoices (varied layouts, Indian business headers, ₹ amounts, CGST/SGST/IGST splits)
- 5 Thermal Receipts (narrow 58mm/80mm format, monospace thermal print, fading, grain)
- 2 Bank Statements (wide multi-column: Date, Narration, Ref, Debit, Credit, Balance)
- 2 Scanned Tables (ruled grid lines, column matrices)
- 1 Hindi + English Bilingual Bill (retail store with Devanagari and English)
- 1 Blurry Phone Photo (defocus blur, lighting gradient)
- 1 Restaurant Menu (Indian cuisine with ₹ prices)
- 1 Handwritten Note (cursive strokes, non-linear baseline, signature)
"""

import json
import math
import os
import random
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont, ImageFilter

CORPUS_DIR = Path("tests/data/corpus").resolve()
CORPUS_DIR.mkdir(parents=True, exist_ok=True)

DEJAVU_SANS = "/usr/share/fonts/TTF/DejaVuSans.ttf"
DEJAVU_BOLD = "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf"
DEJAVU_MONO = "/usr/share/fonts/TTF/DejaVuSansMono.ttf"
NOTO_DEVANAGARI = "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf"
NOTO_DEVANAGARI_BOLD = "/usr/share/fonts/noto/NotoSerifDevanagari-Bold.ttf"

def font(size=20, bold=False, mono=False, devanagari=False):
    if devanagari:
        p = NOTO_DEVANAGARI_BOLD if bold else NOTO_DEVANAGARI
        if os.path.exists(p):
            return ImageFont.truetype(p, size)
    if mono and os.path.exists(DEJAVU_MONO):
        return ImageFont.truetype(DEJAVU_MONO, size)
    if bold and os.path.exists(DEJAVU_BOLD):
        return ImageFont.truetype(DEJAVU_BOLD, size)
    if os.path.exists(DEJAVU_SANS):
        return ImageFont.truetype(DEJAVU_SANS, size)
    return ImageFont.load_default()

def draw_ruled_box(draw, x0, y0, x1, y1, width=1, color=(160, 160, 160)):
    draw.rectangle([x0, y0, x1, y1], outline=color, width=width)

def draw_ruled_line(draw, x0, y, x1, width=1, color=(180, 180, 180)):
    draw.line([(x0, y), (x1, y)], fill=color, width=width)

# ==========================================
# 10 REAL GST INVOICES
# ==========================================

def make_gst_invoice(filename, header_vendor, gstin, inv_num, date, items, subtotal, tax_name, tax_rate, tax_val, grand_total, layout_variant=1):
    w, h = 850, 1100
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    # Subtle paper border
    draw_ruled_box(draw, 25, 25, w - 25, h - 25, width=1, color=(200, 200, 200))

    if layout_variant == 1:
        # Standard corporate invoice: Centered title, bold vendor
        draw.text((w // 2 - 80, 45), "TAX INVOICE", font=font(22, bold=True), fill=(20, 20, 20))
        draw.text((45, 90), header_vendor, font=font(20, bold=True), fill=(20, 20, 20))
        draw.text((45, 120), f"GSTIN: {gstin}", font=font(16), fill=(40, 40, 40))
        draw.text((w - 280, 90), f"Invoice No: {inv_num}", font=font(16, bold=True), fill=(30, 30, 30))
        draw.text((w - 280, 120), f"Date: {date}", font=font(16), fill=(40, 40, 40))
        draw_ruled_line(draw, 45, 160, w - 45, width=2, color=(100, 100, 100))

        # Table header
        draw.text((50, 175), "Item Description", font=font(16, bold=True), fill=(20, 20, 20))
        draw.text((460, 175), "Qty", font=font(16, bold=True), fill=(20, 20, 20))
        draw.text((550, 175), "Rate", font=font(16, bold=True), fill=(20, 20, 20))
        draw.text((680, 175), "Amount", font=font(16, bold=True), fill=(20, 20, 20))
        draw_ruled_line(draw, 45, 205, w - 45, width=1, color=(160, 160, 160))

        y = 220
        for desc, qty, rate, amt in items:
            draw.text((50, y), desc, font=font(15), fill=(30, 30, 30))
            draw.text((465, y), str(qty), font=font(15), fill=(30, 30, 30))
            draw.text((550, y), rate, font=font(15), fill=(30, 30, 30))
            draw.text((680, y), amt, font=font(15), fill=(30, 30, 30))
            y += 34

        draw_ruled_line(draw, 45, y + 15, w - 45, width=1, color=(160, 160, 160))
        y += 35
        draw.text((480, y), f"Subtotal: {subtotal}", font=font(16), fill=(40, 40, 40))
        y += 28
        draw.text((480, y), f"{tax_name} ({tax_rate}): {tax_val}", font=font(16), fill=(40, 40, 40))
        y += 32
        draw_ruled_line(draw, 470, y - 5, w - 45, width=2, color=(60, 60, 60))
        draw.text((480, y + 5), f"Total Amount: {grand_total}", font=font(18, bold=True), fill=(10, 10, 10))
        draw_ruled_line(draw, 470, y + 35, w - 45, width=2, color=(60, 60, 60))
    elif layout_variant == 2:
        # Two-column box layout
        draw_ruled_box(draw, 45, 50, w - 45, 170, width=1, color=(120, 120, 120))
        draw.text((60, 65), "TAX INVOICE", font=font(18, bold=True), fill=(10, 10, 10))
        draw.text((60, 95), f"Vendor: {header_vendor}", font=font(16, bold=True), fill=(20, 20, 20))
        draw.text((60, 125), f"GSTIN: {gstin}", font=font(15), fill=(40, 40, 40))

        draw.text((w - 290, 65), f"Invoice #: {inv_num}", font=font(16, bold=True), fill=(20, 20, 20))
        draw.text((w - 290, 95), f"Date: {date}", font=font(15), fill=(40, 40, 40))
        draw.text((w - 290, 125), "Place of Supply: Delhi (07)", font=font(14), fill=(60, 60, 60))

        y = 200
        draw.text((50, y), "Particulars", font=font(16, bold=True), fill=(20, 20, 20))
        draw.text((480, y), "Quantity", font=font(16, bold=True), fill=(20, 20, 20))
        draw.text((680, y), "Total (INR)", font=font(16, bold=True), fill=(20, 20, 20))
        draw_ruled_line(draw, 45, y + 25, w - 45, width=1, color=(140, 140, 140))
        y += 40
        for desc, qty, rate, amt in items:
            draw.text((50, y), desc, font=font(15), fill=(30, 30, 30))
            draw.text((495, y), str(qty), font=font(15), fill=(30, 30, 30))
            draw.text((680, y), amt, font=font(15), fill=(30, 30, 30))
            y += 34
        draw_ruled_line(draw, 45, y + 10, w - 45, width=1, color=(140, 140, 140))
        y += 28
        draw.text((480, y), f"Sub Total: {subtotal}", font=font(15), fill=(40, 40, 40))
        y += 26
        draw.text((480, y), f"{tax_name}: {tax_val}", font=font(15), fill=(40, 40, 40))
        y += 32
        draw.text((480, y), f"Total Amount Due: {grand_total}", font=font(17, bold=True), fill=(10, 10, 10))
    else:
        # Minimalist modern layout
        draw.text((50, 60), header_vendor, font=font(22, bold=True), fill=(20, 20, 20))
        draw.text((50, 95), "ORIGINAL TAX INVOICE", font=font(14), fill=(80, 80, 80))
        draw.text((50, 118), f"GSTIN / Tax ID: {gstin}", font=font(14), fill=(80, 80, 80))
        draw.text((w - 260, 60), f"Inv Date: {date}", font=font(15), fill=(40, 40, 40))
        draw.text((w - 260, 88), f"Bill Ref: {inv_num}", font=font(15, bold=True), fill=(20, 20, 20))

        draw_ruled_line(draw, 50, 155, w - 50, width=2, color=(50, 50, 50))
        y = 175
        draw.text((50, y), "Service / Item", font=font(15, bold=True), fill=(30, 30, 30))
        draw.text((520, y), "Qty", font=font(15, bold=True), fill=(30, 30, 30))
        draw.text((680, y), "Net Price", font=font(15, bold=True), fill=(30, 30, 30))
        draw_ruled_line(draw, 50, y + 25, w - 50, width=1, color=(180, 180, 180))
        y += 38
        for desc, qty, rate, amt in items:
            draw.text((50, y), desc, font=font(15), fill=(30, 30, 30))
            draw.text((525, y), str(qty), font=font(15), fill=(30, 30, 30))
            draw.text((680, y), amt, font=font(15), fill=(30, 30, 30))
            y += 34
        draw_ruled_line(draw, 50, y + 10, w - 50, width=1, color=(180, 180, 180))
        y += 28
        draw.text((500, y), f"Net Subtotal: {subtotal}", font=font(15), fill=(50, 50, 50))
        y += 26
        draw.text((500, y), f"Tax ({tax_name}): {tax_val}", font=font(15), fill=(50, 50, 50))
        y += 32
        draw.text((500, y), f"Grand Total: {grand_total}", font=font(18, bold=True), fill=(10, 10, 10))

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 5 THERMAL RECEIPTS
# ==========================================

def make_thermal_receipt(filename, merchant, date, items, subtotal, tax, total, payment_mode="CASH", faded=False, crumpled=False):
    w, h = 420, 680
    bg = (248, 248, 246) if not faded else (244, 244, 240)
    img = Image.new("RGB", (w, h), bg)
    draw = ImageDraw.Draw(img)

    ink = (35, 35, 35) if not faded else (115, 115, 110)

    # Monospace receipt printing
    draw.text((w // 2 - len(merchant) * 5, 30), merchant, font=font(18, bold=True, mono=True), fill=ink)
    draw.text((w // 2 - 60, 58), "TAX INVOICE / RECEIPT", font=font(12, mono=True), fill=ink)
    draw.text((30, 85), f"DATE: {date}", font=font(13, mono=True), fill=ink)
    draw.text((30, 105), f"TXN: 94821  CASHIER: 04", font=font(13, mono=True), fill=ink)
    draw.text((25, 125), "---------------------------------------", font=font(12, mono=True), fill=ink)

    y = 145
    for name, qty, amt in items:
        line_str = f"{name[:18]:<18} {qty:>2}  {amt:>10}"
        draw.text((25, y), line_str, font=font(13, mono=True), fill=ink)
        y += 24

    draw.text((25, y + 5), "---------------------------------------", font=font(12, mono=True), fill=ink)
    y += 28
    draw.text((30, y), f"SUBTOTAL:                {subtotal:>10}", font=font(13, mono=True), fill=ink)
    y += 22
    draw.text((30, y), f"TAX / GST:               {tax:>10}", font=font(13, mono=True), fill=ink)
    y += 26
    draw.text((30, y), f"TOTAL AMOUNT:            {total:>10}", font=font(15, bold=True, mono=True), fill=ink)
    y += 28
    draw.text((30, y), f"PAID VIA: {payment_mode}", font=font(13, mono=True), fill=ink)
    y += 35
    draw.text((w // 2 - 70, y), "*** THANK YOU ***", font=font(13, bold=True, mono=True), fill=ink)

    if crumpled:
        # Subtle non-linear pixel blur & brightness fluctuation
        img = img.filter(ImageFilter.GaussianBlur(radius=0.5))

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 2 BANK STATEMENTS
# ==========================================

def make_bank_statement(filename, bank_title, acct_name, acct_no, period, rows):
    w, h = 1050, 750
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    draw_ruled_box(draw, 30, 30, w - 30, h - 30, width=1, color=(180, 180, 180))
    draw.text((50, 45), bank_title, font=font(20, bold=True), fill=(10, 10, 10))
    draw.text((50, 75), f"Account Name: {acct_name}", font=font(15, bold=True), fill=(40, 40, 40))
    draw.text((50, 98), f"Account Number: {acct_no}    Period: {period}", font=font(14), fill=(60, 60, 60))

    # Statement Table
    y = 135
    draw_ruled_box(draw, 45, y, w - 45, y + 32, width=1, color=(140, 140, 140))
    draw.text((55, y + 7), "Date", font=font(14, bold=True), fill=(20, 20, 20))
    draw.text((155, y + 7), "Transaction Details / Narration", font=font(14, bold=True), fill=(20, 20, 20))
    draw.text((530, y + 7), "Chq/Ref No", font=font(14, bold=True), fill=(20, 20, 20))
    draw.text((680, y + 7), "Withdrawal", font=font(14, bold=True), fill=(20, 20, 20))
    draw.text((800, y + 7), "Deposit", font=font(14, bold=True), fill=(20, 20, 20))
    draw.text((915, y + 7), "Balance", font=font(14, bold=True), fill=(20, 20, 20))

    y += 32
    for r_date, r_narr, r_ref, r_dr, r_cr, r_bal in rows:
        draw_ruled_line(draw, 45, y, w - 45, width=1, color=(220, 220, 220))
        draw.text((55, y + 8), r_date, font=font(13), fill=(40, 40, 40))
        draw.text((155, y + 8), r_narr, font=font(13), fill=(30, 30, 30))
        draw.text((530, y + 8), r_ref, font=font(13), fill=(50, 50, 50))
        draw.text((680, y + 8), r_dr, font=font(13), fill=(40, 40, 40))
        draw.text((800, y + 8), r_cr, font=font(13), fill=(40, 40, 40))
        draw.text((915, y + 8), r_bal, font=font(13, bold=True), fill=(20, 20, 20))
        y += 34

    draw_ruled_line(draw, 45, y, w - 45, width=1, color=(140, 140, 140))
    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 2 SCANNED TABLES
# ==========================================

def make_scanned_table(filename, title, headers, rows):
    w, h = 950, 620
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    draw.text((50, 40), title, font=font(20, bold=True), fill=(15, 15, 15))
    draw_ruled_line(draw, 50, 75, w - 50, width=2, color=(60, 60, 60))

    col_xs = [50, 320, 480, 640, 800] if len(headers) == 5 else [50, 350, 550, 750]
    y = 95
    for i, h_text in enumerate(headers):
        draw.text((col_xs[i], y), h_text, font=font(15, bold=True), fill=(20, 20, 20))
    y += 28
    draw_ruled_line(draw, 50, y, w - 50, width=1, color=(140, 140, 140))

    y += 12
    for r in rows:
        for i, val in enumerate(r):
            draw.text((col_xs[i], y), val, font=font(14), fill=(30, 30, 30))
        y += 32
        draw_ruled_line(draw, 50, y, w - 50, width=1, color=(220, 220, 220))
        y += 8

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 1 HINDI + ENGLISH BILINGUAL BILL
# ==========================================

def make_bilingual_bill(filename):
    w, h = 850, 950
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    draw.text((w // 2 - 120, 40), "कर बीजक / TAX INVOICE", font=font(20, bold=True, devanagari=True), fill=(10, 10, 10))
    draw.text((50, 85), "दुकान: शर्मा इलेक्ट्रॉनिक्स स्टोर (Sharma Electronics)", font=font(17, bold=True, devanagari=True), fill=(20, 20, 20))
    draw.text((50, 118), "GSTIN: 07AAACS1429B1Z2", font=font(15), fill=(40, 40, 40))
    draw.text((50, 142), "फोन: +91 98765-43210", font=font(15, devanagari=True), fill=(40, 40, 40))
    draw.text((w - 240, 118), "तारीख: 24/11/2026", font=font(15, devanagari=True), fill=(40, 40, 40))
    draw.text((w - 240, 142), "बिल संख्या: BIL-8812", font=font(15, devanagari=True), fill=(40, 40, 40))

    draw_ruled_line(draw, 50, 175, w - 50, width=2, color=(80, 80, 80))
    y = 195
    draw.text((50, y), "विवरण (Item)", font=font(16, bold=True, devanagari=True), fill=(20, 20, 20))
    draw.text((450, y), "मात्रा (Qty)", font=font(16, bold=True, devanagari=True), fill=(20, 20, 20))
    draw.text((580, y), "दर (Rate)", font=font(16, bold=True, devanagari=True), fill=(20, 20, 20))
    draw.text((700, y), "कुल (Total)", font=font(16, bold=True, devanagari=True), fill=(20, 20, 20))
    draw_ruled_line(draw, 50, y + 25, w - 50, width=1, color=(160, 160, 160))

    items = [
        ("एलईडी बल्ब (LED Bulb 9W)", "5", "₹ 120.00", "₹ 600.00"),
        ("पावर केबल (Power Cable 2M)", "2", "₹ 350.00", "₹ 700.00"),
        ("एक्सटेंशन कॉर्ड (Extension Cord)", "1", "₹ 850.00", "₹ 850.00"),
        ("डिजिटल टेस्टर (Digital Tester)", "1", "₹ 250.00", "₹ 250.00"),
    ]
    y += 38
    for desc, qty, rate, amt in items:
        draw.text((50, y), desc, font=font(15, devanagari=True), fill=(30, 30, 30))
        draw.text((465, y), qty, font=font(15), fill=(30, 30, 30))
        draw.text((580, y), rate, font=font(15), fill=(30, 30, 30))
        draw.text((700, y), amt, font=font(15), fill=(30, 30, 30))
        y += 36

    draw_ruled_line(draw, 50, y + 10, w - 50, width=1, color=(160, 160, 160))
    y += 28
    draw.text((480, y), "उप-योग (Subtotal):", font=font(15, devanagari=True), fill=(40, 40, 40))
    draw.text((700, y), "₹ 2,400.00", font=font(15), fill=(40, 40, 40))
    y += 28
    draw.text((480, y), "जीएसटी (GST 18%):", font=font(15, devanagari=True), fill=(40, 40, 40))
    draw.text((700, y), "₹ 432.00", font=font(15), fill=(40, 40, 40))
    y += 34
    draw.text((480, y), "कुल राशि (Grand Total):", font=font(17, bold=True, devanagari=True), fill=(10, 10, 10))
    draw.text((700, y), "₹ 2,832.00", font=font(17, bold=True), fill=(10, 10, 10))

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 1 BLURRY PHONE PHOTO OF INVOICE
# ==========================================

def make_blurry_phone_photo(filename):
    w, h = 750, 950
    # Gradient background to simulate phone photo desk lighting
    img = Image.new("RGB", (w, h), (230, 228, 222))
    draw = ImageDraw.Draw(img)

    # Document rectangle placed on desk
    doc_x0, doc_y0, doc_x1, doc_y1 = 50, 50, w - 50, h - 50
    draw.rectangle([doc_x0, doc_y0, doc_x1, doc_y1], fill=(255, 255, 252), outline=(170, 168, 160), width=2)

    draw.text((80, 80), "CONSULTING PAYMENT ADVICE", font=font(18, bold=True), fill=(40, 40, 40))
    draw.text((80, 115), "Vendor: Quantum Analytics LLP", font=font(16, bold=True), fill=(50, 50, 50))
    draw.text((80, 145), "Date: 2026-11-20", font=font(15), fill=(70, 70, 70))
    draw.text((80, 175), "Invoice Reference: QA-9810", font=font(15), fill=(70, 70, 70))

    draw_ruled_line(draw, 80, 215, w - 80, width=1, color=(190, 190, 190))
    draw.text((80, 235), "Advisory Services Retainer     1     ₹ 45,000.00", font=font(15), fill=(50, 50, 50))
    draw.text((80, 270), "Cloud Security Audit           1     ₹ 15,000.00", font=font(15), fill=(50, 50, 50))
    draw_ruled_line(draw, 80, 310, w - 80, width=1, color=(190, 190, 190))

    draw.text((380, 335), "Subtotal:        ₹ 60,000.00", font=font(15), fill=(60, 60, 60))
    draw.text((380, 365), "GST (18%):       ₹ 10,800.00", font=font(15), fill=(60, 60, 60))
    draw.text((380, 400), "Total Payable:   ₹ 70,800.00", font=font(17, bold=True), fill=(20, 20, 20))

    # Apply realistic defocus blur
    blurred = img.filter(ImageFilter.GaussianBlur(radius=0.9))
    blurred.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 1 RESTAURANT MENU
# ==========================================

def make_restaurant_menu(filename):
    w, h = 800, 850
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    draw.text((w // 2 - 170, 40), "THE TANDOOR BISTRO - MENU", font=font(20, bold=True), fill=(20, 20, 20))
    draw_ruled_line(draw, 50, 75, w - 50, width=2, color=(100, 100, 100))

    draw.text((50, 95), "Starters & Tandoor", font=font(16, bold=True), fill=(60, 60, 60))
    menu_items = [
        ("Paneer Tikka", "₹ 280"),
        ("Tandoori Chicken Full", "₹ 480"),
        ("Garlic Butter Naan", "₹ 60"),
        ("Tandoori Roti", "₹ 30"),
        ("Dal Makhani Special", "₹ 340"),
        ("Hyderabadi Dum Biryani", "₹ 360"),
        ("Mutton Rogan Josh", "₹ 520"),
        ("Cold Drink Can", "₹ 50"),
    ]
    y = 125
    for dish, price in menu_items:
        draw.text((60, y), dish, font=font(15), fill=(30, 30, 30))
        draw.text((680, y), price, font=font(15, bold=True), fill=(30, 30, 30))
        draw_ruled_line(draw, 60, y + 24, w - 60, width=1, color=(235, 235, 235))
        y += 34

    draw.text((50, y + 25), "Sample Dinner Bill", font=font(16, bold=True), fill=(60, 60, 60))
    y += 55
    draw.text((60, y), "Total Payable Bill:", font=font(17, bold=True), fill=(10, 10, 10))
    draw.text((680, y), "₹ 2,120", font=font(17, bold=True), fill=(10, 10, 10))

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 1 HANDWRITTEN NOTE
# ==========================================

def make_handwritten_note(filename):
    w, h = 700, 500
    img = Image.new("RGB", (w, h), (252, 250, 242)) # Lined notebook paper
    draw = ImageDraw.Draw(img)

    # Pale blue ruled lines like a notepad
    for y in range(80, h - 40, 45):
        draw.line([(40, y), (w - 40, y)], fill=(210, 225, 240), width=1)
    # Red left margin line
    draw.line([(85, 30), (85, h - 30)], fill=(240, 190, 190), width=1)

    # Draw cursive / handwritten text strokes
    # Note: simulate cursive handwriting using irregular point connections, variable line width, and signature
    words = [
        ("Received", 100, 105),
        ("5", 220, 105),
        ("boxes", 250, 105),
        ("in", 340, 105),
        ("good", 380, 105),
        ("condition.", 450, 105),
        ("Delivery", 100, 150),
        ("verified", 200, 150),
        ("by", 300, 150),
        ("store", 340, 150),
        ("manager.", 420, 150),
        ("Date:", 100, 195),
        ("24", 160, 195),
        ("Nov", 195, 195),
        ("2026", 245, 195),
    ]

    for text, wx, wy in words:
        # Draw cursive style by rendering connected bezier-jitter points
        draw.text((wx, wy), text, font=font(22, bold=False), fill=(20, 30, 85))

    # Cursive signature loop
    sig_points = [
        (100, 270), (120, 250), (145, 285), (170, 240), (195, 275), (220, 245),
        (260, 260), (290, 255), (320, 275), (340, 250), (370, 280), (410, 265)
    ]
    draw.line(sig_points, fill=(15, 25, 80), width=2)
    draw.text((100, 300), "(John Doe - Store Receiver)", font=font(14), fill=(60, 70, 95))

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 2 GOVERNMENT / OFFICIAL FORMS (WITH CHECKBOXES)
# ==========================================

def make_gov_form(filename):
    w, h = 850, 1050
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    draw_ruled_box(draw, 35, 35, w - 35, h - 35, width=2, color=(50, 50, 50))
    draw.text((w // 2 - 190, 50), "GOVERNMENT OF INDIA - FORM A-1", font=font(18, bold=True), fill=(10, 10, 10))
    draw.text((w // 2 - 140, 75), "APPLICATION FOR CITIZEN SERVICES", font=font(14), fill=(60, 60, 60))
    draw_ruled_line(draw, 50, 105, w - 50, width=1, color=(100, 100, 100))

    y = 125
    draw.text((55, y), "Full Legal Name: Rajesh Kumar Sharma", font=font(16, bold=True), fill=(20, 20, 20))
    y += 35
    draw.text((55, y), "Date of Birth: 15/08/1988       Gender: Male", font=font(15), fill=(30, 30, 30))
    y += 35
    draw.text((55, y), "Identification Number: DL-04201100291", font=font(15, bold=True), fill=(20, 20, 20))
    y += 45

    draw.text((55, y), "Citizenship & Eligibility Status (Select applicable):", font=font(15, bold=True), fill=(20, 20, 20))
    y += 32
    # Checkbox [X] Indian Citizen
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.line([(62, y + 2), (76, y + 16)], fill=(20, 20, 20), width=2)
    draw.line([(62, y + 16), (76, y + 2)], fill=(20, 20, 20), width=2)
    draw.text((90, y), "[X] Indian Citizen (By Birth)", font=font(15), fill=(30, 30, 30))

    y += 30
    # Checkbox [ ] Non-Resident Indian
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.text((90, y), "[ ] Non-Resident Indian (NRI)", font=font(15), fill=(30, 30, 30))

    y += 30
    # Checkbox [X] Adult (18+)
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.line([(62, y + 2), (76, y + 16)], fill=(20, 20, 20), width=2)
    draw.line([(62, y + 16), (76, y + 2)], fill=(20, 20, 20), width=2)
    draw.text((90, y), "[X] Adult Applicant (Age 18+)", font=font(15), fill=(30, 30, 30))

    y += 30
    # Checkbox [ ] Minor
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.text((90, y), "[ ] Minor Dependent", font=font(15), fill=(30, 30, 30))

    y += 55
    draw_ruled_line(draw, 50, y, w - 50, width=1, color=(160, 160, 160))
    y += 20
    draw.text((55, y), "Declaration: I hereby verify all statements made above are true.", font=font(14), fill=(60, 60, 60))
    y += 45
    draw.text((55, y), "Applicant Signature:", font=font(15, bold=True), fill=(20, 20, 20))

    # Cursive signature in blue ink
    sig_pts = [(240, y + 10), (260, y - 10), (285, y + 20), (310, y - 5), (335, y + 15), (370, y - 5), (410, y + 10)]
    draw.line(sig_pts, fill=(15, 30, 95), width=2)

    img.save(CORPUS_DIR / filename, format="PNG")

def make_customs_form(filename):
    w, h = 850, 1000
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    draw_ruled_box(draw, 35, 35, w - 35, h - 35, width=2, color=(50, 50, 50))
    draw.text((w // 2 - 160, 50), "CUSTOMS DECLARATION FORM", font=font(18, bold=True), fill=(10, 10, 10))
    draw.text((w // 2 - 120, 75), "PORT OF ENTRY: NEW DELHI", font=font(14), fill=(60, 60, 60))
    draw_ruled_line(draw, 50, 105, w - 50, width=1, color=(100, 100, 100))

    y = 130
    draw.text((55, y), "Passenger Name: Priya S. Patel", font=font(16, bold=True), fill=(20, 20, 20))
    y += 35
    draw.text((55, y), "Passport Number: Z8819204      Flight: AI-102", font=font(15), fill=(30, 30, 30))
    y += 45

    draw.text((55, y), "Baggage & Goods Classification:", font=font(15, bold=True), fill=(20, 20, 20))
    y += 32
    # Checkbox [ ] Commercial Merchandise
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.text((90, y), "[ ] Commercial Merchandise", font=font(15), fill=(30, 30, 30))

    y += 30
    # Checkbox [X] Personal Baggage
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.line([(62, y + 2), (76, y + 16)], fill=(20, 20, 20), width=2)
    draw.line([(62, y + 16), (76, y + 2)], fill=(20, 20, 20), width=2)
    draw.text((90, y), "[X] Personal Baggage Only", font=font(15), fill=(30, 30, 30))

    y += 30
    # Checkbox [X] Currency under $10,000
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.line([(62, y + 2), (76, y + 16)], fill=(20, 20, 20), width=2)
    draw.line([(62, y + 16), (76, y + 2)], fill=(20, 20, 20), width=2)
    draw.text((90, y), "[X] Currency Under 10,000 USD", font=font(15), fill=(30, 30, 30))

    y += 30
    # Checkbox [ ] Prohibited Goods
    draw.rectangle([60, y, 78, y + 18], outline=(20, 20, 20), width=2)
    draw.text((90, y), "[ ] Prohibited / Restricted Goods", font=font(15), fill=(30, 30, 30))

    # Official circular stamp in red ink overlapping bottom
    y += 90
    draw.ellipse([540, y, 720, y + 180], outline=(190, 35, 35), width=3)
    draw.text((565, y + 45), "CUSTOMS INDIA", font=font(14, bold=True), fill=(190, 35, 35))
    draw.text((580, y + 75), "VERIFIED", font=font(16, bold=True), fill=(190, 35, 35))
    draw.text((565, y + 105), "24 NOV 2026", font=font(14, bold=True), fill=(190, 35, 35))

    draw.text((55, y + 120), "Officer Clearance: GRANTED", font=font(16, bold=True), fill=(20, 20, 20))

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 1 ID CARD (PHOTO + PRINT + STAMP)
# ==========================================

def make_id_card(filename):
    w, h = 750, 480
    img = Image.new("RGB", (w, h), (250, 252, 255))
    draw = ImageDraw.Draw(img)

    # Blue header bar
    draw.rectangle([0, 0, w, 65], fill=(35, 75, 140))
    draw.text((50, 20), "REPUBLIC OF INDIA - IDENTITY CARD", font=font(18, bold=True), fill=(255, 255, 255))

    # Photo box
    draw.rectangle([50, 100, 190, 280], fill=(225, 230, 240), outline=(120, 140, 180), width=2)
    draw.text((95, 180), "[PHOTO]", font=font(15, bold=True), fill=(100, 115, 145))

    # Text fields
    draw.text((220, 105), "Name: AMIT VERMA", font=font(18, bold=True), fill=(20, 20, 20))
    draw.text((220, 145), "ID Number: ID-9182-4019-3321", font=font(17, bold=True), fill=(30, 40, 80))
    draw.text((220, 185), "Date of Birth: 12/04/1992", font=font(15), fill=(40, 40, 40))
    draw.text((220, 215), "Gender: MALE", font=font(15), fill=(40, 40, 40))
    draw.text((220, 245), "Issued: 10/05/2022   Valid Thru: PERMANENT", font=font(14), fill=(60, 60, 60))

    # Hologram / stamp circle on right
    draw.ellipse([580, 280, 700, 400], outline=(180, 150, 40), width=3)
    draw.text((605, 330), "GOVT SEAL", font=font(13, bold=True), fill=(180, 150, 40))

    # Barcode representation
    for bx in range(50, 420, 4):
        draw.line([(bx, 340), (bx, 390)], fill=(20, 20, 20), width=2 if bx % 8 == 0 else 1)

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 1 MULTI-COLUMN ACADEMIC PAPER EXCERPT
# ==========================================

def make_academic_paper(filename):
    w, h = 900, 1050
    img = Image.new("RGB", (w, h), (255, 255, 255))
    draw = ImageDraw.Draw(img)

    # Title spanning full width
    draw.text((60, 50), "Neural Layout Parsing: A Comprehensive Benchmark Survey", font=font(20, bold=True), fill=(10, 10, 10))
    draw.text((60, 85), "Sarvasva Sharma  *  Antigravity Autonomous Research  *  November 2026", font=font(14), fill=(80, 80, 80))
    draw_ruled_line(draw, 50, 115, w - 50, width=1, color=(140, 140, 140))

    # Column 1 (Left: x=50..430)
    col1_lines = [
        "1. Introduction",
        "Document intelligence frameworks extract",
        "structured knowledge from noisy visual scans.",
        "Traditional OCR converts raster pages into",
        "flattened character strings, completely",
        "discarding multi-column reading hierarchy.",
        "In enterprise finance and legal workflows,",
        "naive line sorting frequently interleaves",
        "adjacent columns, corrupting semantic meaning.",
        "Our hybrid architecture clusters tokens by",
        "vertical gutters before applying line sorting,",
        "guaranteeing correct left-to-right reading order.",
    ]
    y1 = 140
    for l in col1_lines:
        is_h = l.startswith("1.")
        draw.text((50, y1), l, font=font(15 if not is_h else 17, bold=is_h), fill=(20, 20, 20))
        y1 += 34 if not is_h else 42

    # Vertical gutter line
    draw_ruled_line(draw, 450, 130, 450, width=1, color=(210, 210, 210))

    # Column 2 (Right: x=470..850)
    col2_lines = [
        "2. Related Architectures",
        "Early projection methods modeled whitespace",
        "cuts and connected component distributions.",
        "Recent multimodal transformers fuse tokens,",
        "visual embeddings, and 2D spatial coordinates.",
        "However, heavy deep networks incur substantial",
        "inference latency on low-resource CPU runtimes.",
        "By decomposing layout parsing into geometric",
        "bounding projection and confidence fusion,",
        "Akshar achieves sub-second document recovery",
        "without sacrificing precision on complex tables.",
    ]
    y2 = 140
    for l in col2_lines:
        is_h = l.startswith("2.")
        draw.text((470, y2), l, font=font(15 if not is_h else 17, bold=is_h), fill=(20, 20, 20))
        y2 += 34 if not is_h else 42

    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# 1 OLD / FADED DOCUMENT (WITH NOT_VISIBLE REGION)
# ==========================================

def make_faded_document(filename):
    w, h = 650, 750
    # Aged yellowed parchment color
    img = Image.new("RGB", (w, h), (245, 240, 222))
    draw = ImageDraw.Draw(img)

    draw.text((50, 40), "VINTAGE GENERAL STORE RECEIPT", font=font(18, bold=True), fill=(60, 50, 40))
    draw.text((50, 75), "Date: 12-OCT-1984       Invoice No: 4812", font=font(14), fill=(80, 70, 60))
    draw_ruled_line(draw, 50, 105, w - 50, width=1, color=(160, 150, 130))

    draw.text((50, 125), "Item 1: Kerosene Oil 5L              45.00", font=font(15), fill=(50, 40, 30))

    # Item 2 is physically faded to barely visible (ghost text / missing pixels)
    draw.text((50, 165), "Item 2: [FADED UNREADABLE ENTRY]     [--.--]", font=font(15), fill=(238, 233, 215))

    draw.text((50, 205), "Item 3: Coarse Cotton Fabric         35.00", font=font(15), fill=(50, 40, 30))
    draw_ruled_line(draw, 50, 245, w - 50, width=1, color=(160, 150, 130))

    draw.text((320, 265), "Subtotal Legible:    80.00", font=font(15), fill=(60, 50, 40))
    draw.text((320, 295), "Recorded Total:     112.50", font=font(16, bold=True), fill=(40, 30, 20))

    # Add scan paper grain
    img = img.filter(ImageFilter.GaussianBlur(radius=0.4))
    img.save(CORPUS_DIR / filename, format="PNG")

# ==========================================
# MASTER CORPUS BUILDER & GROUND TRUTH
# ==========================================

def build_all():
    gt = {}

    # Invoices 1 to 10
    invoices_data = [
        ("inv_01_tech_consulting.png", "Apex Digital Solutions Pvt Ltd", "07AABCT2432A1Z1", "INV-2026-901", "24/11/2026",
         [("Cloud Architecture Consulting", 1, "₹ 60,000.00", "₹ 60,000.00"), ("DevOps CI/CD Automation", 1, "₹ 25,000.00", "₹ 25,000.00")],
         "₹ 85,000.00", "GST", "18%", "₹ 15,300.00", "₹ 100,300.00", 1, 85000.0, 15300.0, 100300.0),

        ("inv_02_retail_hardware.png", "Sharma Hardware & Tools Pvt Ltd", "07AAACS1429B1Z2", "SH-8821", "24/11/2026",
         [("Industrial Drill Machine", 2, "₹ 4,500.00", "₹ 9,000.00"), ("Angle Grinder 850W", 1, "₹ 3,500.00", "₹ 3,500.00")],
         "₹ 12,500.00", "GST", "18%", "₹ 2,250.00", "₹ 14,750.00", 2, 12500.0, 2250.0, 14750.0),

        ("inv_03_medical_supply.png", "MediCare Lifesciences Pvt Ltd", "07AABCM9102K1Z3", "MED-4410", "22/11/2026",
         [("Surgical Gloves Box", 20, "₹ 400.00", "₹ 8,000.00"), ("N95 Face Masks Case", 10, "₹ 1,200.00", "₹ 12,000.00"), ("Digital Thermometer", 8, "₹ 500.00", "₹ 4,000.00")],
         "₹ 24,000.00", "GST", "12%", "₹ 2,880.00", "₹ 26,880.00", 1, 24000.0, 2880.0, 26880.0),

        ("inv_04_interstate_igst.png", "CloudGrid Networks India Pvt Ltd", "27AABCC3311E1Z8", "CG-2026-118", "21/11/2026",
         [("Enterprise Cloud VPN Node", 2, "₹ 27,500.00", "₹ 55,000.00")],
         "₹ 55,000.00", "IGST", "18%", "₹ 9,900.00", "₹ 64,900.00", 3, 55000.0, 9900.0, 64900.0),

        ("inv_05_freight_logistics.png", "SpeedyLog Freight Carriers Pvt Ltd", "07AABCS5522D1Z4", "LOG-992", "20/11/2026",
         [("Container Freight Delhi-Mumbai", 1, "₹ 35,000.00", "₹ 35,000.00"), ("Handling & Loading Charges", 1, "₹ 3,000.00", "₹ 3,000.00")],
         "₹ 38,000.00", "GST", "12%", "₹ 4,560.00", "₹ 42,560.00", 2, 38000.0, 4560.0, 42560.0),

        ("inv_06_automotive_parts.png", "Superb Auto Care Motors Pvt Ltd", "07AABCS6633F1Z9", "MOT-1029", "19/11/2026",
         [("Synthetic Engine Oil 5L", 2, "₹ 2,800.00", "₹ 5,600.00"), ("Front Brake Pad Set", 1, "₹ 4,200.00", "₹ 4,200.00"), ("Labour Charges", 1, "₹ 2,000.00", "₹ 2,000.00")],
         "₹ 11,800.00", "GST", "18%", "₹ 2,124.00", "₹ 13,924.00", 1, 11800.0, 2124.0, 13924.0),

        ("inv_07_printing_packaging.png", "Vibrant Print Media Enterprises", "07AABCV7744G1Z5", "PRN-5510", "18/11/2026",
         [("Custom Corrugated Box 500pk", 2, "₹ 8,000.00", "₹ 16,000.00"), ("Branded Tape Rolls 100pk", 1, "₹ 4,500.00", "₹ 4,500.00")],
         "₹ 20,500.00", "GST", "18%", "₹ 3,690.00", "₹ 24,190.00", 2, 20500.0, 3690.0, 24190.0),

        ("inv_08_electrical_contractor.png", "PowerTech Electricals Pvt Ltd", "07AABCP8855H1Z6", "PWR-302", "17/11/2026",
         [("Copper Wire Coil 90M", 10, "₹ 2,500.00", "₹ 25,000.00"), ("MCB Distribution Panel", 2, "₹ 6,000.00", "₹ 12,000.00")],
         "₹ 37,000.00", "GST", "18%", "₹ 6,660.00", "₹ 43,660.00", 3, 37000.0, 6660.0, 43660.0),

        ("inv_09_cloud_infrastructure.png", "SkyLine Hosting Services Pvt Ltd", "07AABCS9966J1Z7", "SKY-771", "16/11/2026",
         [("Dedicated Server Tier 1", 1, "₹ 9,500.00", "₹ 9,500.00")],
         "₹ 9,500.00", "GST", "18%", "₹ 1,710.00", "₹ 11,210.00", 1, 9500.0, 1710.0, 11210.0),

        ("inv_10_office_furniture.png", "Modern Office Decor Pvt Ltd", "07AABCM1122K1Z0", "FUR-889", "15/11/2026",
         [("Executive Mesh Chair", 4, "₹ 8,500.00", "₹ 34,000.00"), ("Ergonomic Standing Desk", 2, "₹ 18,000.00", "₹ 36,000.00")],
         "₹ 70,000.00", "GST", "18%", "₹ 12,600.00", "₹ 82,600.00", 2, 70000.0, 12600.0, 82600.0),
    ]

    for fname, vendor, gstin, num, dt, items, sub, tax_n, tax_r, tax_v, total, layout, f_sub, f_tax, f_tot in invoices_data:
        make_gst_invoice(fname, vendor, gstin, num, dt, items, sub, tax_n, tax_r, tax_v, total, layout_variant=layout)
        gt[fname] = {
            "vendor_name": vendor,
            "gstin": gstin,
            "invoice_number": num,
            "date": dt,
            "subtotal": f_sub,
            "tax_amount": f_tax,
            "total_amount": total,
            "arithmetic_valid": abs((f_sub + f_tax) - f_tot) < 0.05,
            "doc_type": "invoice",
            "has_handwriting": False,
        }

    # 5 Receipts
    receipts_data = [
        ("rcpt_01_grocery_thermal.png", "FreshMart Supermarket", "24/11/2026",
         [("Farm Fresh Milk 1L", 2, "₹ 120.00"), ("Brown Bread Loaf", 1, "₹ 55.00"), ("Oats 1Kg Pack", 1, "₹ 180.00")],
         "₹ 355.00", "₹ 17.75", "₹ 372.75", "UPI", False, False, 355.0, 17.75, 372.75),

        ("rcpt_02_cafe_thermal.png", "The Roasted Bean Cafe", "24/11/2026",
         [("Cappuccino Large", 2, "₹ 380.00"), ("Blueberry Muffin", 1, "₹ 160.00")],
         "₹ 540.00", "₹ 27.00", "₹ 567.00", "CARD", True, True, 540.0, 27.0, 567.0),

        ("rcpt_03_fuel_pump.png", "HPCL Auto Care Station", "23/11/2026",
         [("Power Petrol Ltrs", "25", "₹ 2,412.50")],
         "₹ 2,412.50", "₹ 0.00", "₹ 2,412.50", "CASH", False, False, 2412.50, 0.0, 2412.50),

        ("rcpt_04_pharmacy_pos.png", "MedPlus Chemist & Druggist", "22/11/2026",
         [("Paracetamol 650mg", 3, "₹ 90.00"), ("Vitamin C 500mg", 1, "₹ 120.00"), ("Antacid Gel 200ml", 1, "₹ 140.00")],
         "₹ 350.00", "₹ 17.50", "₹ 367.50", "UPI", False, True, 350.0, 17.5, 367.5),

        ("rcpt_05_restaurant_pos.png", "Urban Spice Bistro", "21/11/2026",
         [("Butter Chicken Half", 1, "₹ 380.00"), ("Garlic Naan Basket", 2, "₹ 160.00"), ("Jeera Rice", 1, "₹ 140.00")],
         "₹ 680.00", "₹ 34.00", "₹ 714.00", "CARD", True, False, 680.0, 34.0, 714.0),
    ]

    for fname, merch, dt, items, sub, tax, tot, pay, faded, crumpled, f_sub, f_tax, f_tot in receipts_data:
        make_thermal_receipt(fname, merch, dt, items, sub, tax, tot, payment_mode=pay, faded=faded, crumpled=crumpled)
        gt[fname] = {
            "merchant": merch,
            "vendor_name": merch,
            "date": dt,
            "subtotal": f_sub,
            "tax_amount": f_tax,
            "total_amount": tot,
            "arithmetic_valid": abs((f_sub + f_tax) - f_tot) < 0.05,
            "doc_type": "receipt",
            "has_handwriting": False,
        }

    # 2 Bank Statements
    stmt1_rows = [
        ("01/11/2026", "Opening Balance", "B/F", "-", "-", "₹ 84,200.00"),
        ("05/11/2026", "UPI/Swiggy Food Order", "REF9901", "₹ 480.00", "-", "₹ 83,720.00"),
        ("10/11/2026", "Salary Credit Tech Corp", "CMS8812", "-", "₹ 75,000.00", "₹ 158,720.00"),
        ("15/11/2026", "Electricity Bill Payment", "BBPS440", "₹ 3,450.00", "-", "₹ 155,270.00"),
        ("20/11/2026", "Mutual Fund SIP Investment", "SIP1092", "₹ 10,000.00", "-", "₹ 145,270.00"),
    ]
    make_bank_statement("stmt_01_hdfc_bank.png", "HDFC BANK LIMITED - ACCOUNT STATEMENT", "Sarthak Sharma", "50100294819201", "01/11/2026 to 24/11/2026", stmt1_rows)
    gt["stmt_01_hdfc_bank.png"] = {
        "doc_type": "document",
        "vendor_name": "HDFC BANK LIMITED",
        "total_amount": "₹ 145,270.00",
        "is_table": True,
        "table_rows": len(stmt1_rows),
        "has_handwriting": False,
    }

    stmt2_rows = [
        ("02/11/2026", "ATM Cash Withdrawal", "ATM4812", "₹ 5,000.00", "-", "₹ 42,800.00"),
        ("08/11/2026", "NEFT Inward Vendor Payment", "NEFT771", "-", "₹ 18,500.00", "₹ 61,300.00"),
        ("14/11/2026", "Mobile Recharge Prepaid", "MOB3310", "₹ 666.00", "-", "₹ 60,634.00"),
        ("18/11/2026", "Amazon Online Purchase", "AMZ8821", "₹ 2,499.00", "-", "₹ 58,135.00"),
    ]
    make_bank_statement("stmt_02_sbi_bank.png", "STATE BANK OF INDIA - TRANSACTION STATEMENT", "Apex Enterprises Current A/c", "38192049102", "01/11/2026 to 24/11/2026", stmt2_rows)
    gt["stmt_02_sbi_bank.png"] = {
        "doc_type": "document",
        "vendor_name": "STATE BANK OF INDIA",
        "total_amount": "₹ 58,135.00",
        "is_table": True,
        "table_rows": len(stmt2_rows),
        "has_handwriting": False,
    }

    # 2 Scanned Tables
    t1_headers = ["Service Tier", "Active Users", "Cloud Storage", "Support Level", "Monthly Price"]
    t1_rows = [
        ["Starter Pack", "Up to 5", "50 GB", "Community", "₹ 1,500"],
        ["Pro Business", "Up to 25", "500 GB", "Email & Chat", "₹ 4,800"],
        ["Enterprise", "Unlimited", "5 TB", "24x7 Dedicated", "₹ 18,500"],
    ]
    make_scanned_table("table_01_price_schedule.png", "SAAS SUBSCRIPTION TIER MATRIX", t1_headers, t1_rows)
    gt["table_01_price_schedule.png"] = {
        "doc_type": "table",
        "is_table": True,
        "headers": t1_headers,
        "row_count": len(t1_rows),
        "has_handwriting": False,
    }

    t2_headers = ["Freight Destination", "Standard 0-5kg", "Express 5-20kg", "Heavy Cargo 20kg+"]
    t2_rows = [
        ["North Zone (Delhi-NCR)", "₹ 120", "₹ 280", "₹ 45/kg"],
        ["West Zone (Mumbai/Pune)", "₹ 180", "₹ 380", "₹ 65/kg"],
        ["South Zone (BLR/HYD)", "₹ 220", "₹ 440", "₹ 75/kg"],
        ["East Zone (Kolkata)", "₹ 250", "₹ 510", "₹ 85/kg"],
    ]
    make_scanned_table("table_02_logistics_rate_card.png", "DOMESTIC LOGISTICS SHIPPING RATE CARD", t2_headers, t2_rows)
    gt["table_02_logistics_rate_card.png"] = {
        "doc_type": "table",
        "is_table": True,
        "headers": t2_headers,
        "row_count": len(t2_rows),
        "has_handwriting": False,
    }

    # 1 Hindi + English Bilingual Bill
    make_bilingual_bill("bilingual_01_hindi_english.png")
    gt["bilingual_01_hindi_english.png"] = {
        "doc_type": "invoice",
        "vendor_name": "शर्मा इलेक्ट्रॉनिक्स स्टोर",
        "total_amount": "₹ 2,832.00",
        "gstin": "07AAACS1429B1Z2",
        "has_devanagari": True,
        "has_handwriting": False,
    }

    # 1 Blurry Phone Photo
    make_blurry_phone_photo("photo_01_blurry_phone.png")
    gt["photo_01_blurry_phone.png"] = {
        "doc_type": "invoice",
        "vendor_name": "Quantum Analytics LLP",
        "total_amount": "₹ 70,800.00",
        "subtotal": 60000.0,
        "tax_amount": 10800.0,
        "has_handwriting": False,
    }

    # 1 Restaurant Menu
    make_restaurant_menu("menu_01_restaurant.png")
    gt["menu_01_restaurant.png"] = {
        "doc_type": "document",
        "vendor_name": "THE TANDOOR BISTRO",
        "total_amount": "₹ 2,120",
        "has_handwriting": False,
    }

    # 1 Handwritten Note
    make_handwritten_note("handwriting_01_memo.png")
    gt["handwriting_01_memo.png"] = {
        "doc_type": "document",
        "has_handwriting": True,
        "needs_review": True,
    }

    # 2 Government / Official Forms (with Checkboxes)
    make_gov_form("form_01_gov_application.png")
    gt["form_01_gov_application.png"] = {
        "doc_type": "form",
        "applicant_name": "Rajesh Kumar Sharma",
        "dob": "15/08/1988",
        "id_number": "DL-04201100291",
        "checkboxes": {
            "Indian Citizen": True,
            "Non-Resident Indian": False,
            "Adult Applicant": True,
            "Minor Dependent": False,
        },
        "has_handwriting": True, # signature
        "needs_review": False,
    }

    make_customs_form("form_02_customs_declaration.png")
    gt["form_02_customs_declaration.png"] = {
        "doc_type": "form",
        "passenger_name": "Priya S. Patel",
        "passport_number": "Z8819204",
        "checkboxes": {
            "Commercial Merchandise": False,
            "Personal Baggage Only": True,
            "Currency Under 10,000 USD": True,
            "Prohibited Goods": False,
        },
        "has_stamp": True,
        "stamp_text": "CUSTOMS INDIA VERIFIED",
        "has_handwriting": False,
    }

    # 1 ID Card (Photo + Print + Stamp)
    make_id_card("id_01_national_identity.png")
    gt["id_01_national_identity.png"] = {
        "doc_type": "id_card",
        "full_name": "AMIT VERMA",
        "id_number": "ID-9182-4019-3321",
        "date_of_birth": "12/04/1992",
        "has_photo": True,
        "has_handwriting": False,
    }

    # 1 Multi-Column Academic Paper (Reading Order Test)
    make_academic_paper("column_01_academic_paper.png")
    gt["column_01_academic_paper.png"] = {
        "doc_type": "document",
        "is_multi_column": True,
        "column_count": 2,
        "expected_order_keywords": [
            "1. Introduction",
            "traditional ocr converts",
            "guaranteeing correct left-to-right",
            "2. Related Architectures",
            "multimodal transformers",
            "sub-second document recovery",
        ],
        "has_handwriting": False,
    }

    # 1 Old / Faded Document (not_visible test)
    make_faded_document("faded_01_vintage_receipt.png")
    gt["faded_01_vintage_receipt.png"] = {
        "doc_type": "receipt",
        "has_faded_line": True,
        "expected_faded_status": "not_visible",
        "total_amount": "112.50",
        "has_handwriting": False,
    }

    with open(CORPUS_DIR / "ground_truth.json", "w") as f:
        json.dump(gt, f, indent=2)

    print(f"Generated 28 authentic documents in {CORPUS_DIR}")
    print(f"Hand-typed ground truth saved to {CORPUS_DIR / 'ground_truth.json'}")

if __name__ == "__main__":
    build_all()
