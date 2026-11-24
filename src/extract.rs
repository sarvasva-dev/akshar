use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use crate::classify::PageClassification;
use crate::tiling::OCRLine;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedEntities {
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub urls: Vec<String>,
    pub dates: Vec<String>,
    pub amounts: Vec<String>,
    pub ids: Vec<String>,
    pub key_values: BTreeMap<String, String>,
    pub table_rows: Vec<Vec<String>>,
    pub markdown_table: String,
    pub reconstructed_text: String,
}

fn re_email() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap())
}

fn re_phone() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:\+?\d{1,3}[\s\-]?)?(?:\(?\d{3,5}\)?[\s\-]?\d{3,5}(?:[\s\-]?\d{3,5})?|\b\d{10,12}\b)")
            .unwrap()
    })
}

fn re_url() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(?:https?://|www\.)[^\s,;<>]+").unwrap())
}

fn re_date() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:\d{1,2}[/\-\.]\d{1,2}[/\-\.]\d{2,4}|\d{4}[/\-]\d{1,2}[/\-]\d{1,2}|\d{1,2}\s+(?:jan|feb|mar|apr|may|jun|jul|aug|sep|sept|oct|nov|dec)[a-z]*[\s,]+\d{2,4}|(?:jan|feb|mar|apr|may|jun|jul|aug|sep|sept|oct|nov|dec)[a-z]*\s+\d{1,2},?\s+\d{2,4})\b",
        )
        .unwrap()
    })
}

fn re_amount() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:(?:₹|rs\.?|inr|\$|usd|€|eur|£|gbp)\s*\d[\d,]*(?:\.\d{1,2})?|\b\d[\d,]*(?:\.\d{1,2})?\s*(?:rupees?|inr|usd|eur|gbp|rs\.?)\b)",
        )
        .unwrap()
    })
}

/// Deterministic post-normalization for common CTC character confusion (e.g. `87.o0` -> `87.00`, `INVoICE` -> `INVOICE`).
pub fn normalize_ocr_line(input: &str) -> String {
    static RE_NUM_O: OnceLock<Regex> = OnceLock::new();
    static RE_UPPER_O: OnceLock<Regex> = OnceLock::new();
    let re_num_o = RE_NUM_O.get_or_init(|| Regex::new(r"(\d[.,\d]*)[oO](\d)").unwrap());
    let re_num_trail = Regex::new(r"(\d)[oO]\b").unwrap();
    let re_upper_o = RE_UPPER_O.get_or_init(|| Regex::new(r"([A-Z]{2,})o([A-Z]{2,})").unwrap());

    let mut s = input.trim().to_string();
    // Fix O/o inside numbers (`50O0` -> `5000`, `87.o0` -> `87.00`, `40o` -> `400`, `(40o)` -> `(400)`)
    for _ in 0..3 {
        let next = re_num_o.replace_all(&s, "${1}0${2}").into_owned();
        if next == s {
            break;
        }
        s = next;
    }
    s = re_num_trail.replace_all(&s, "${1}0").into_owned();
    static RE_PAREN_O: OnceLock<Regex> = OnceLock::new();
    let re_paren_o = RE_PAREN_O.get_or_init(|| Regex::new(r"(\d)[oO]([)\]\.,;:])").unwrap());
    s = re_paren_o.replace_all(&s, "${1}0${2}").into_owned();
    // Fix lowercase `o` surrounded by ALL-CAPS letters (`INVoICE` -> `INVOICE`)
    s = re_upper_o.replace_all(&s, "${1}O${2}").into_owned();
    s
}

/// Detect Mirage CTC garbled non-Latin / shirorekha lines (e.g. `IIL•NLLNNLVNL•R•I•I•`).
pub fn is_garbled_ctc_line(text: &str, confidence: f64) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    let bullets = t.chars().filter(|&c| matches!(c, '•' | '·' | '|')).count();
    if bullets >= 2 {
        return true;
    }
    let upper_i_l_n = t
        .chars()
        .filter(|&c| matches!(c, 'I' | 'L' | 'N' | 'V' | '•'))
        .count();
    if t.len() >= 8 && upper_i_l_n * 100 / t.chars().count().max(1) >= 65 {
        return true;
    }
    confidence < 0.36 && !t.chars().any(|c| c.is_ascii_digit())
}

/// Context-aware `₹` (Indian Rupee) recovery across OCR lines.
/// Repairs Mirage/Tesseract substitutions (`r340`, `T60`, `¥ 12,345.00`, and `$280` in Indian contexts) to `₹`.
pub fn repair_indian_currency_lines(lines: &mut Vec<OCRLine>) {
    static RE_RT_PRICE: OnceLock<Regex> = OnceLock::new();
    static RE_YEN_PRICE: OnceLock<Regex> = OnceLock::new();
    static RE_DOLLAR_PRICE: OnceLock<Regex> = OnceLock::new();
    static RE_EQUALS_PRICE: OnceLock<Regex> = OnceLock::new();

    let re_rt = RE_RT_PRICE.get_or_init(|| {
        Regex::new(r"\b[rT](\d{2,6}(?:,\d{2,3})*(?:\.\d{1,2})?)\b").unwrap()
    });
    let re_yen = RE_YEN_PRICE.get_or_init(|| {
        Regex::new(r"(?:¥|हैं)\s*(\d[\d,]*(?:\.\d{1,2})?)").unwrap()
    });
    let re_dollar = RE_DOLLAR_PRICE.get_or_init(|| {
        Regex::new(r"\$\s*(\d{2,6}(?:,\d{2,3})*(?:\.\d{1,2})?)\b").unwrap()
    });
    let re_equals = RE_EQUALS_PRICE.get_or_init(|| {
        Regex::new(r"(?:=|\bINR\b)\s*(\d[\d,]*(?:\.\d{1,2})?)").unwrap()
    });

    // Drop pure shirorekha bullet garbage lines
    lines.retain(|l| {
        let bullets = l.text.chars().filter(|&c| c == '•').count();
        !(bullets >= 3 && l.confidence < 0.45)
    });

    let full_lower = lines
        .iter()
        .map(|l| l.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");

    let has_rt_or_yen = lines
        .iter()
        .any(|l| re_rt.is_match(&l.text) || re_yen.is_match(&l.text) || l.text.contains('₹') || re_equals.is_match(&l.text));

    let is_indian_context = has_rt_or_yen
        || full_lower.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
        || [
            "gst", "cgst", "sgst", "igst", "inr", "rupee", "rs.", "pvt ltd", "india", "+91",
            "aadhaar", "pan", "upi", "naan", "paneer", "tandoori", "masala", "biryani", "roti",
            "dal", "thali", "kulcha", "paratha", "items selected", "tax invoice", "total amount",
        ]
        .iter()
        .any(|kw| full_lower.contains(kw));

    for ln in lines.iter_mut() {
        ln.text = normalize_ocr_line(&ln.text);
        // Avoid turning flight/gate/terminal codes like "T3" into "₹3" (re_rt requires >= 2 digits)
        if !ln.text.to_uppercase().contains("FLIGHT") && !ln.text.to_uppercase().contains("TERMINAL") {
            ln.text = re_rt.replace_all(&ln.text, "₹$1").into_owned();
        }
        ln.text = re_yen.replace_all(&ln.text, "₹$1").into_owned();
        if is_indian_context && !full_lower.contains("usd") {
            ln.text = re_dollar.replace_all(&ln.text, "₹$1").into_owned();
            ln.text = re_equals.replace_all(&ln.text, "₹$1").into_owned();
        }
    }
}

/// Grounded, deterministic structured extraction directly from verified OCR/native text.
/// Returns `null` honestly when a field is genuinely absent — never echoes field names as values.
pub fn extract_grounded_form(
    schema: &Value,
    text: &str,
    lines: &[OCRLine],
    classification: &PageClassification,
) -> Map<String, Value> {
    let entities = extract_intelligence(text, lines);
    let mut out = Map::new();
    let Some(fields) = schema.as_object() else {
        return out;
    };

    let text_lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    for (key, spec) in fields {
        let k = key.to_ascii_lowercase();
        let allowed_enums: Option<Vec<String>> = spec
            .get("enum")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());

        let found: Option<(Value, f64)> = if k.contains("document_type") || k == "type" || k == "card_type" {
            let dt = classification.document_type.as_str();
            if let Some(enums) = &allowed_enums {
                if let Some(m) = enums.iter().find(|e| e.eq_ignore_ascii_case(dt)) {
                    Some((json!(m), classification.confidence))
                } else if let Some(other) = enums.iter().find(|e| e.eq_ignore_ascii_case("other")) {
                    Some((json!(other), 0.65))
                } else {
                    None
                }
            } else {
                Some((json!(dt), classification.confidence))
            }
        } else if k.contains("total") || k.contains("amount") {
            extract_total_amount(&text_lines, &entities)
                .map(|v| (json!(v), 0.92))
        } else if k.contains("invoice_number") || k.contains("invoice_no") || k.contains("id_number") {
            extract_labeled_code(&text_lines, &["invoice no", "invoice number", "inv no", "id no", "id number", "receipt #", "receipt no", "ref"])
                .map(|v| (json!(v), 0.90))
        } else if k.contains("date") || k.contains("dob") {
            entities.dates.first().map(|d| (json!(d), 0.90))
        } else if k.contains("email") {
            entities.emails.first().map(|e| (json!(e), 0.95))
        } else if k.contains("phone") || k.contains("mobile") {
            entities.phones.first().map(|p| (json!(p), 0.90))
        } else if k.contains("payment") {
            let lower = text.to_lowercase();
            let pick = if lower.contains("upi") {
                Some("upi")
            } else if lower.contains("card") || lower.contains("visa") || lower.contains("mastercard") {
                Some("card")
            } else if lower.contains("cash") {
                Some("cash")
            } else {
                None
            };
            pick.and_then(|p| {
                if let Some(enums) = &allowed_enums {
                    enums.iter().find(|e| e.eq_ignore_ascii_case(p)).map(|m| (json!(m), 0.88))
                } else {
                    Some((json!(p), 0.88))
                }
            })
        } else if k.contains("vendor") || k.contains("merchant") || k.contains("company") {
            extract_vendor_header(&text_lines).map(|v| (json!(v), 0.78))
        } else {
            // Check exact key-value map match (e.g., "client: alpha corp")
            let k_spaced = k.replace('_', " ");
            entities
                .key_values
                .iter()
                .find(|(ek, _)| {
                    let ekl = ek.to_ascii_lowercase();
                    ekl == k || ekl == k_spaced || ekl.contains(&k_spaced)
                })
                .map(|(_, v)| (json!(v), 0.85))
        };

        if let Some((val, conf)) = found {
            out.insert(
                key.clone(),
                json!({
                    "value": val,
                    "confidence": (conf * 1000.0).round() / 1000.0,
                    "source": "ocr_grounded"
                }),
            );
        } else {
            out.insert(
                key.clone(),
                json!({
                    "value": Value::Null,
                    "confidence": 0.0,
                    "source": "none"
                }),
            );
        }
    }

    out
}

/// Sanitize raw native-model form predictions: reject low-confidence (< min_conf) or echoed field names.
pub fn sanitize_model_form_field(
    field_name: &str,
    all_keys: &HashSet<String>,
    raw: &Value,
    min_conf: f64,
) -> Value {
    let prob = raw
        .get("probability")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            let val_str = raw.get("value").and_then(|v| v.as_str())?;
            raw.get("probabilities")?.get(val_str)?.as_f64()
        })
        .unwrap_or(0.0);

    let val = raw.get("value").cloned().unwrap_or(Value::Null);
    let is_garbage = match &val {
        Value::Null => true,
        Value::String(s) => {
            let low = s.trim().to_ascii_lowercase();
            low.is_empty()
                || low == "-"
                || low == "none"
                || low == "null"
                || low == field_name.to_ascii_lowercase()
                || all_keys.contains(&low)
                || low.contains("_name")
                || low.contains("_amount")
                || low.contains("_number")
                || low.contains("_type")
        }
        _ => false,
    };

    if prob < min_conf || is_garbage {
        json!({
            "value": Value::Null,
            "confidence": (prob * 1000.0).round() / 1000.0,
            "source": "none"
        })
    } else {
        json!({
            "value": val,
            "confidence": (prob * 1000.0).round() / 1000.0,
            "source": "model_form"
        })
    }
}

fn extract_total_amount(lines: &[&str], entities: &ExtractedEntities) -> Option<String> {
    static RE_TOTAL_NUM: OnceLock<Regex> = OnceLock::new();
    let re_num = RE_TOTAL_NUM.get_or_init(|| {
        Regex::new(r"(?i)(?:₹|rs\.?|inr|\$|usd|€|eur|£|gbp)?\s*\d[\d,]*(?:\.\d{1,2})?(?:\s*(?:rupees?|inr|usd|eur|gbp|rs\.?))?").unwrap()
    });

    // Check reconstructed rows first (merges 2-column "GRAND TOTAL   ₹ 736.91")
    let recon_lines: Vec<&str> = entities
        .reconstructed_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let search_pool = if recon_lines.is_empty() { lines } else { &recon_lines };

    for (idx, line) in search_pool.iter().enumerate().rev() {
        let low = line.to_ascii_lowercase();
        if low.contains("grand total")
            || low.contains("total amount")
            || low.contains("amount due")
            || low.contains("balance due")
            || low.contains("net payable")
            || low.contains("total payable")
            || low.contains("कुल राशि")
            || low.starts_with("total")
        {
            if let Some(m) = re_amount().find(line) {
                return Some(m.as_str().trim().to_string());
            }
            if let Some(m) = re_num.find(line) {
                let s = m.as_str().trim();
                if s.chars().any(|c| c.is_ascii_digit()) {
                    return Some(s.to_string());
                }
            }
            // Check immediately following line if label and amount were split vertically
            if let Some(next_line) = search_pool.get(idx + 1) {
                if let Some(m) = re_amount().find(next_line).or_else(|| re_num.find(next_line)) {
                    let s = m.as_str().trim();
                    if s.chars().any(|c| c.is_ascii_digit()) {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    entities.amounts.last().cloned()
}

fn extract_labeled_code(lines: &[&str], prefixes: &[&str]) -> Option<String> {
    static RE_CODE: OnceLock<Regex> = OnceLock::new();
    let re_code = RE_CODE.get_or_init(|| Regex::new(r"[A-Z0-9][A-Z0-9\-/#]{2,}").unwrap());
    for line in lines {
        let low = line.to_ascii_lowercase();
        for pref in prefixes {
            if let Some(pos) = low.find(pref) {
                let rest = &line[pos + pref.len()..];
                let trimmed = rest.trim_start_matches([':', '#', ' ', '.', '-']);
                if let Some(m) = re_code.find(trimmed) {
                    let s = m.as_str();
                    if s.chars().any(|c| c.is_ascii_digit()) {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_vendor_header(lines: &[&str]) -> Option<String> {
    // 1. Explicit label on any line: "Vendor: ...", "Merchant: ...", "Seller: ..."
    for line in lines.iter().take(8) {
        let low = line.to_ascii_lowercase();
        for pref in ["vendor:", "merchant:", "seller:", "billed by:", "supplier:", "company:"] {
            if let Some(pos) = low.find(pref) {
                let val = line[pos + pref.len()..].trim();
                if val.len() >= 2 {
                    return Some(val.to_string());
                }
            }
        }
    }

    // 2. Top header lines (first 2 lines): require positive business-entity evidence and
    //    reject generic page/document headers like "Scanned page", "Smoke Test", "INVOICE", etc.
    const GENERIC_BANNED: &[&str] = &[
        "scanned", "page", "smoke", "test", "sample", "document", "invoice", "receipt", "total",
        "amount", "balance", "due", "date", "bill", "summary", "report", "table", "form",
        "letter", "copy", "draft", "original", "customer", "client", "item", "qty", "price",
        "tax ", "gstin", "quote", "estimate", "memo", "notice",
    ];
    const BUSINESS_MARKERS: &[&str] = &[
        "ltd", "pvt", "limited", "inc", "llp", "corp", "co.", "company", "enterprises",
        "solutions", "technologies", "tech", "systems", "services", "traders", "trading",
        "store", "mart", "shop", "cafe", "restaurant", "hotel", "kitchen", "bistro", "pharmacy",
        "clinic", "hospital", "labs", "motors", "logistics", "agency", "studio", "associates",
        "group", "industries", "electronics", "foods", "retail", "bazaar", "udyog", "लिमिटेड",
        "प्राइवेट", "स्टोर", "होटल",
    ];

    for candidate in lines.iter().take(2) {
        let trimmed = candidate.trim();
        let low = trimmed.to_lowercase();
        if GENERIC_BANNED.iter().any(|b| low.contains(b)) {
            continue;
        }
        let has_marker = BUSINESS_MARKERS.iter().any(|m| low.contains(m));
        if has_marker && trimmed.len() >= 3 && trimmed.len() <= 72 {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn re_pan_or_aadhaar() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(?:[A-Z]{5}\d{4}[A-Z]|\d{4}[\s\-]\d{4}[\s\-]\d{4})\b").unwrap()
    })
}

fn collect_unique(re: &Regex, text: &str, filter: impl Fn(&str) -> bool) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for m in re.find_iter(text) {
        let s = m.as_str().trim();
        if !filter(s) {
            continue;
        }
        let key = s.to_lowercase();
        if seen.insert(key) {
            out.push(s.to_string());
        }
    }
    out
}

/// Group visually aligned OCR lines into logical rows (merges 2-column splits like "SUBTOTAL | 624.50").
pub fn group_visual_rows(lines: &[OCRLine]) -> Vec<Vec<&OCRLine>> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut heights: Vec<i32> = lines.iter().map(|l| (l.bbox[3] - l.bbox[1]).max(1)).collect();
    heights.sort_unstable();
    let median_h = heights[heights.len() / 2];
    let tol = ((median_h as f64) * 0.55).max(6.0);

    let mut refs: Vec<&OCRLine> = lines.iter().collect();
    refs.sort_by(|a, b| {
        let ca = (a.bbox[1] + a.bbox[3]) as f64 / 2.0;
        let cb = (b.bbox[1] + b.bbox[3]) as f64 / 2.0;
        ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<Vec<&OCRLine>> = Vec::new();
    for ln in refs {
        let cy = (ln.bbox[1] + ln.bbox[3]) as f64 / 2.0;
        let same = rows
            .last()
            .map(|r| {
                let avg: f64 = r
                    .iter()
                    .map(|x| (x.bbox[1] + x.bbox[3]) as f64 / 2.0)
                    .sum::<f64>()
                    / (r.len() as f64);
                (cy - avg).abs() <= tol
            })
            .unwrap_or(false);
        if same {
            rows.last_mut().unwrap().push(ln);
        } else {
            rows.push(vec![ln]);
        }
    }

    for row in &mut rows {
        row.sort_by_key(|l| l.bbox[0]);
    }
    rows
}

/// Extract entities, key-value pairs, and reconstructed tabular rows from OCR output.
pub fn extract_intelligence(text: &str, lines: &[OCRLine]) -> ExtractedEntities {
    let rows = group_visual_rows(lines);
    let mut reconstructed_lines = Vec::with_capacity(rows.len());
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut key_values: BTreeMap<String, String> = BTreeMap::new();

    for row in &rows {
        let cols: Vec<String> = row.iter().map(|l| l.text.trim().to_string()).collect();
        if cols.len() >= 2 {
            reconstructed_lines.push(cols.join("   "));
            table_rows.push(cols.clone());
            if cols.len() == 2 {
                let k = cols[0].trim_end_matches(':').trim();
                let v = cols[1].trim();
                if !k.is_empty() && !v.is_empty() && k.len() <= 48 {
                    key_values.insert(k.to_string(), v.to_string());
                }
            }
        } else if let Some(single) = cols.first() {
            reconstructed_lines.push(single.clone());
            if let Some((k, v)) = single.split_once(':') {
                let k = k.trim();
                let v = v.trim();
                if !k.is_empty() && !v.is_empty() && k.len() <= 40 {
                    key_values.insert(k.to_string(), v.to_string());
                }
            }
        }
    }

    let reconstructed_text = if reconstructed_lines.is_empty() {
        text.to_string()
    } else {
        reconstructed_lines.join("\n")
    };

    let markdown_table = if table_rows.len() >= 2 {
        let max_cols = table_rows.iter().map(|r| r.len()).max().unwrap_or(2);
        let mut md = Vec::new();
        for (idx, row) in table_rows.iter().enumerate() {
            let mut padded = row.clone();
            padded.resize(max_cols, String::new());
            let escaped: Vec<String> = padded.iter().map(|c| c.replace('|', "\\|")).collect();
            md.push(format!("| {} |", escaped.join(" | ")));
            if idx == 0 {
                md.push(format!("| {} |", vec!["---"; max_cols].join(" | ")));
            }
        }
        md.join("\n")
    } else {
        String::new()
    };

    let scan_text = if reconstructed_text.is_empty() {
        text
    } else {
        &reconstructed_text
    };

    let emails = collect_unique(re_email(), scan_text, |_| true);
    let urls = collect_unique(re_url(), scan_text, |_| true);
    let dates = collect_unique(re_date(), scan_text, |_| true);
    let amounts = collect_unique(re_amount(), scan_text, |_| true);
    let ids = collect_unique(re_pan_or_aadhaar(), scan_text, |_| true);
    let phones = collect_unique(re_phone(), scan_text, |s| {
        let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
        // Exclude pure dates or 12-digit Aadhaar groups (already captured in ids)
        (10..=15).contains(&digits) && !re_date().is_match(s) && !re_pan_or_aadhaar().is_match(s)
    });

    ExtractedEntities {
        emails,
        phones,
        urls,
        dates,
        amounts,
        ids,
        key_values,
        table_rows,
        markdown_table,
        reconstructed_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_entities_and_merges_two_column_rows() {
        let lines = vec![
            OCRLine { text: "INVOICE NO: INV-2026-0842".into(), confidence: 0.9, bbox: [20, 20, 400, 50], bbox_norm: [0; 4] },
            OCRLine { text: "CONTACT: billing@acme.io | +91 98765-43210".into(), confidence: 0.9, bbox: [20, 60, 500, 90], bbox_norm: [0; 4] },
            OCRLine { text: "SUBTOTAL".into(), confidence: 0.9, bbox: [20, 110, 180, 140], bbox_norm: [0; 4] },
            OCRLine { text: "INR 624.50".into(), confidence: 0.9, bbox: [420, 112, 560, 142], bbox_norm: [0; 4] },
            OCRLine { text: "GRAND TOTAL".into(), confidence: 0.9, bbox: [20, 160, 200, 190], bbox_norm: [0; 4] },
            OCRLine { text: "INR 736.91".into(), confidence: 0.9, bbox: [420, 162, 560, 192], bbox_norm: [0; 4] },
        ];
        let ent = extract_intelligence("", &lines);
        assert_eq!(ent.emails, vec!["billing@acme.io"]);
        assert_eq!(ent.phones.len(), 1);
        assert_eq!(ent.amounts.len(), 2);
        assert_eq!(ent.key_values.get("SUBTOTAL").map(String::as_str), Some("INR 624.50"));
        assert_eq!(ent.key_values.get("INVOICE NO").map(String::as_str), Some("INV-2026-0842"));
        assert!(ent.markdown_table.contains("| SUBTOTAL | INR 624.50 |"));
    }
}
