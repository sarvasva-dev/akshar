use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

use crate::table::ExtractedTable;
use crate::tiling::OCRLine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldEvidence {
    pub value: Value,
    pub raw_text: String,
    pub confidence: f64,
    pub bbox: [i32; 4],
    pub bbox_norm: [i32; 4],
    pub page: usize,
    pub engine: String,
    pub repairs_applied: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecimalAmount {
    pub raw: String,
    pub cents: i64,
    pub currency: String,
    pub locale: String, // "US_IN" or "EU" or "AMBIGUOUS"
    pub formatted: String,
}

impl DecimalAmount {
    pub fn parse(input: &str, is_indian_hint: bool) -> Option<(Self, Vec<String>)> {
        let mut repairs = Vec::new();
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Currency detection
        let mut currency = String::new();
        if trimmed.contains('₹') || trimmed.to_uppercase().contains("INR") || trimmed.to_uppercase().contains("RS") {
            currency = "₹".into();
        } else if trimmed.contains('$') || trimmed.to_uppercase().contains("USD") {
            currency = if is_indian_hint {
                repairs.push("repaired currency symbol '$' to '₹' based on Indian GST document context".into());
                "₹".into()
            } else {
                "$".into()
            };
        } else if trimmed.contains('€') || trimmed.to_uppercase().contains("EUR") {
            currency = if is_indian_hint {
                repairs.push("repaired currency symbol '€' to '₹' based on Indian GST document context".into());
                "₹".into()
            } else {
                "€".into()
            };
        } else if trimmed.contains('£') || trimmed.to_uppercase().contains("GBP") {
            currency = "£".into();
        } else if trimmed.starts_with('%') || (trimmed.contains('%') && is_indian_hint) {
            repairs.push("repaired OCR misread '%' to '₹'".into());
            currency = "₹".into();
        }

        // Strip common prefix labels (e.g. "Total:", "Rs.", "INR", "USD", etc.)
        let mut work = trimmed;
        let prefixes = [
            "TOTAL AMOUNT:", "TOTAL AMOUNT", "TOTAL DUE:", "TOTAL DUE", "GRAND TOTAL:", "GRAND TOTAL",
            "TOTAL:", "TOTAL", "BALANCE DUE:", "BALANCE DUE", "BALANCE:", "BALANCE", "RS.", "RS ", "RS", "INR.", "INR ", "INR",
            "USD.", "USD ", "USD", "EUR.", "EUR ", "EUR", "₹", "$", "€", "£", "%",
        ];
        for p in prefixes {
            if let Some(pos) = work.to_uppercase().find(p) {
                if pos == 0 {
                    work = work[p.len()..].trim();
                }
            }
        }
        work = work.trim_start_matches(|c: char| c == ':' || c == '₹' || c == '$' || c == '€' || c == '£' || c == '%' || c == ' ' || c == '-');

        // Extract numeric portion: must start with digit or lookalike O/o/l/I
        static RE_NUM: OnceLock<Regex> = OnceLock::new();
        let re_num = RE_NUM.get_or_init(|| Regex::new(r"[\dOolI][\dOolIS.,\s]*").unwrap());
        let Some(m) = re_num.find(work) else {
            return None;
        };

        let mut num_str = m.as_str().trim().replace(' ', "");
        if num_str.is_empty() {
            return None;
        }

        // Look-alike hardening inside amount string
        if num_str.contains('O') || num_str.contains('o') {
            repairs.push("repaired look-alike 'O'/'o' to '0' inside numeric amount".into());
            num_str = num_str.replace(['O', 'o'], "0");
        }
        if num_str.contains('l') || num_str.contains('I') {
            repairs.push("repaired look-alike 'l'/'I' to '1' inside numeric amount".into());
            num_str = num_str.replace(['l', 'I'], "1");
        }
        if num_str.contains('S') {
            repairs.push("repaired look-alike 'S' to '5' inside numeric amount".into());
            num_str = num_str.replace('S', "5");
        }

        // Determine locale format: US/IN (1,234.56) vs EU (1.234,56)
        let has_comma = num_str.contains(',');
        let has_dot = num_str.contains('.');
        let locale: String;
        let normalized_dec: String;

        if has_comma && has_dot {
            let last_comma = num_str.rfind(',').unwrap_or(0);
            let last_dot = num_str.rfind('.').unwrap_or(0);
            if last_dot > last_comma {
                // Standard US/IN: 1,234.56
                locale = "US_IN".into();
                normalized_dec = num_str.replace(',', "");
            } else {
                // European: 1.234,56
                locale = "EU".into();
                normalized_dec = num_str.replace('.', "").replace(',', ".");
            }
        } else if has_comma {
            // Check if comma is decimal (e.g. "12,50" -> 2 digits after comma) or thousands ("12,000" -> 3 digits)
            let parts: Vec<&str> = num_str.split(',').collect();
            if parts.len() == 2 && parts[1].len() == 2 {
                locale = "EU".into();
                normalized_dec = num_str.replace(',', ".");
            } else if parts.len() >= 2 && parts.last().map(|s| s.len()).unwrap_or(0) == 3 {
                locale = "US_IN".into();
                normalized_dec = num_str.replace(',', "");
            } else {
                locale = "AMBIGUOUS".into();
                repairs.push("flagged ambiguous decimal separator in amount".into());
                normalized_dec = num_str.replace(',', "");
            }
        } else if has_dot {
            // Check if dot is thousands ("12.000") or decimal ("12.50")
            let parts: Vec<&str> = num_str.split('.').collect();
            if parts.len() == 2 && parts[1].len() == 3 && parts[0].len() <= 3 {
                locale = "EU".into();
                normalized_dec = num_str.replace('.', "");
            } else {
                locale = "US_IN".into();
                normalized_dec = num_str;
            }
        } else {
            locale = "US_IN".into();
            normalized_dec = num_str;
        }

        let Ok(val_float) = normalized_dec.parse::<f64>() else {
            return None;
        };
        let cents = (val_float * 100.0).round() as i64;
        let formatted = if currency.is_empty() {
            format!("{:.2}", val_float)
        } else {
            format!("{} {:.2}", currency, val_float)
        };

        Some((
            DecimalAmount {
                raw: input.to_string(),
                cents,
                currency,
                locale,
                formatted,
            },
            repairs,
        ))
    }

    pub fn to_f64(&self) -> f64 {
        (self.cents as f64) / 100.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArithmeticValidation {
    pub reconciled: bool,
    pub subtotal_extracted: Option<f64>,
    pub tax_extracted: Option<f64>,
    pub total_extracted: Option<f64>,
    pub line_items_sum: Option<f64>,
    pub discrepancy: Option<f64>,
    pub needs_review: bool,
    pub review_reasons: Vec<String>,
}

pub fn validate_invoice_arithmetic(
    _lines: &[OCRLine],
    tables: &[ExtractedTable],
    subtotal_str: Option<&str>,
    tax_str: Option<&str>,
    total_str: Option<&str>,
    is_indian: bool,
) -> ArithmeticValidation {
    let mut out = ArithmeticValidation::default();

    let sub_dec = subtotal_str.and_then(|s| DecimalAmount::parse(s, is_indian)).map(|(d, _)| d.to_f64());
    let tax_dec = tax_str.and_then(|s| DecimalAmount::parse(s, is_indian)).map(|(d, _)| d.to_f64());
    let tot_dec = total_str.and_then(|s| DecimalAmount::parse(s, is_indian)).map(|(d, _)| d.to_f64());

    out.subtotal_extracted = sub_dec;
    out.tax_extracted = tax_dec;
    out.total_extracted = tot_dec;

    // Check 1: Subtotal + Tax == Total
    if let (Some(sub), Some(tax), Some(tot)) = (sub_dec, tax_dec, tot_dec) {
        let expected = sub + tax;
        let diff = (expected - tot).abs();
        out.discrepancy = Some((diff * 100.0).round() / 100.0);

        if diff < 0.05 {
            out.reconciled = true;
        } else {
            out.reconciled = false;
            out.needs_review = true;
            out.review_reasons.push(format!(
                "arithmetic_mismatch: subtotal ({:.2}) + tax ({:.2}) = {:.2}, but total is {:.2} (diff: {:.2})",
                sub, tax, expected, tot, diff
            ));
        }
    }

    // Check 2: Sum of line items in table if present
    let mut item_sum = 0.0;
    let mut found_items = false;

    for tbl in tables {
        for row in &tbl.rows {
            if let Some(last_cell) = row.last() {
                if let Some((d, _)) = DecimalAmount::parse(last_cell, is_indian) {
                    let v = d.to_f64();
                    if v > 0.0 && v < tot_dec.unwrap_or(f64::MAX) {
                        item_sum += v;
                        found_items = true;
                    }
                }
            }
        }
    }

    if found_items && item_sum > 0.0 {
        let rounded_sum = (item_sum * 100.0).round() / 100.0;
        out.line_items_sum = Some(rounded_sum);

        if let Some(sub) = sub_dec {
            let diff = (rounded_sum - sub).abs();
            if diff >= 0.05 && diff < (sub * 0.9) {
                // Only flag if mismatch is notable
                out.needs_review = true;
                out.review_reasons.push(format!(
                    "line_items_sum_mismatch: table items sum to {:.2}, but subtotal is {:.2}",
                    rounded_sum, sub
                ));
            }
        }
    }

    // Check 3: Missing total
    if tot_dec.is_none() {
        out.needs_review = true;
        out.review_reasons.push("missing_total_amount: document has no verified grand total".into());
    }

    out
}

pub fn validate_bank_statement_arithmetic(
    tables: &[ExtractedTable],
    is_indian: bool,
) -> ArithmeticValidation {
    let mut out = ArithmeticValidation::default();
    let mut opening: Option<f64> = None;
    let mut closing: Option<f64> = None;
    let mut total_withdrawals = 0.0;
    let mut total_deposits = 0.0;

    for tbl in tables {
        for row in &tbl.rows {
            let row_str = row.join(" ").to_uppercase();
            if row_str.contains("OPENING") || row_str.contains("B/F") {
                if let Some(cell) = row.last() {
                    if let Some((d, _)) = DecimalAmount::parse(cell, is_indian) {
                        opening = Some(d.to_f64());
                    }
                }
            }

            // In typical bank statements: Date | Narration | Ref | Debit | Credit | Balance
            if row.len() >= 5 {
                let dr_idx = row.len() - 3;
                let cr_idx = row.len() - 2;
                let bal_idx = row.len() - 1;

                if let Some((d, _)) = DecimalAmount::parse(&row[dr_idx], is_indian) {
                    total_withdrawals += d.to_f64();
                }
                if let Some((d, _)) = DecimalAmount::parse(&row[cr_idx], is_indian) {
                    total_deposits += d.to_f64();
                }
                if let Some((d, _)) = DecimalAmount::parse(&row[bal_idx], is_indian) {
                    closing = Some(d.to_f64());
                }
            }
        }
    }

    if closing.is_none() {
        for tbl in tables {
            for row in &tbl.rows {
                let row_str = row.join(" ").to_lowercase();
                if row_str.contains("balance") || row_str.contains("total") {
                    for cell in row.iter().rev() {
                        if let Some((d, _)) = DecimalAmount::parse(cell, is_indian) {
                            if d.to_f64() > 0.0 {
                                closing = Some(d.to_f64());
                                break;
                            }
                        }
                    }
                }
                if closing.is_some() {
                    break;
                }
            }
        }
    }

    out.total_extracted = closing;

    if let (Some(op), Some(cl)) = (opening, closing) {
        let expected_cl = op + total_deposits - total_withdrawals;
        let diff = (expected_cl - cl).abs();
        out.discrepancy = Some((diff * 100.0).round() / 100.0);

        if diff < 0.05 {
            out.reconciled = true;
            out.needs_review = false;
        } else {
            out.reconciled = false;
            out.needs_review = true;
            out.review_reasons.push(format!(
                "statement_reconciliation_failed: opening ({:.2}) + deposits ({:.2}) - withdrawals ({:.2}) = {:.2}, closing is {:.2}",
                op, total_deposits, total_withdrawals, expected_cl, cl
            ));
        }
    } else if closing.is_some() {
        out.reconciled = true;
        out.needs_review = false;
    } else {
        out.needs_review = true;
        out.review_reasons.push("missing_total_amount: bank statement balance could not be extracted".into());
    }

    out
}
