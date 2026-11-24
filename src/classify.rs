use serde::{Deserialize, Serialize};

use crate::table::ExtractedTable;
use crate::tiling::OCRLine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageClassification {
    pub document_type: String,
    pub confidence: f64,
    pub signals: Vec<String>,
}

/// Deterministic, explainable document & page-type classifier (<0.2 ms).
pub fn classify_page(
    text: &str,
    lines: &[OCRLine],
    tables: &[ExtractedTable],
    has_objects: bool,
) -> PageClassification {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return PageClassification {
            document_type: if has_objects { "photo".into() } else { "other".into() },
            confidence: if has_objects { 0.90 } else { 0.50 },
            signals: if has_objects {
                vec!["no_text".into(), "visual_objects_present".into()]
            } else {
                vec!["no_text".into()]
            },
        };
    }

    let lower = trimmed.to_lowercase();
    let mut scores: Vec<(&str, f64, Vec<String>)> = vec![
        ("invoice", 0.0, vec![]),
        ("receipt", 0.0, vec![]),
        ("id_card", 0.0, vec![]),
        ("letter", 0.0, vec![]),
        ("form", 0.0, vec![]),
        ("table", 0.0, vec![]),
    ];

    let mut add = |kind: &str, pts: f64, sig: &str| {
        if let Some(entry) = scores.iter_mut().find(|(k, _, _)| *k == kind) {
            entry.1 += pts;
            entry.2.push(sig.to_string());
        }
    };

    // Invoice cues
    for (kw, w) in [
        ("invoice", 0.55),
        ("tax invoice", 0.35),
        ("total amount due", 0.55),
        ("amount due", 0.45),
        ("balance due", 0.45),
        ("bill to", 0.35),
        ("invoice no", 0.40),
        ("invoice number", 0.40),
        ("due date", 0.30),
        ("gst", 0.25),
        ("subtotal", 0.25),
        ("grand total", 0.25),
        ("payment terms", 0.30),
        ("rupees", 0.20),
    ] {
        if lower.contains(kw) {
            add("invoice", w, &format!("kw:{kw}"));
        }
    }

    // Receipt cues
    for (kw, w) in [
        ("receipt", 0.55),
        ("paid by card", 0.45),
        ("cash tendered", 0.45),
        ("change due", 0.40),
        ("thank you for shopping", 0.45),
        ("thank you for visiting", 0.40),
        ("pos", 0.20),
        ("terminal", 0.15),
    ] {
        if lower.contains(kw) {
            add("receipt", w, &format!("kw:{kw}"));
        }
    }

    // ID card cues
    for (kw, w) in [
        ("passport", 0.60),
        ("driver license", 0.60),
        ("driving licence", 0.60),
        ("date of birth", 0.40),
        ("government of", 0.45),
        ("unique identification", 0.55),
        ("aadhaar", 0.60),
        ("income tax department", 0.55),
        ("permanent account number", 0.60),
        ("nationality", 0.35),
    ] {
        if lower.contains(kw) {
            add("id_card", w, &format!("kw:{kw}"));
        }
    }

    // Letter cues
    for (kw, w) in [
        ("dear ", 0.45),
        ("sincerely", 0.45),
        ("yours faithfully", 0.50),
        ("regards,", 0.35),
        ("subject:", 0.30),
    ] {
        if lower.contains(kw) {
            add("letter", w, &format!("kw:{kw}"));
        }
    }

    // Form cues
    let colon_lines = lines.iter().filter(|l| l.text.contains(':')).count();
    if colon_lines >= 4 && lines.len() >= 5 {
        add("form", 0.40, "many_key_value_fields");
    }
    for (kw, w) in [
        ("application form", 0.55),
        ("signature", 0.25),
        ("applicant", 0.35),
        ("please fill", 0.40),
    ] {
        if lower.contains(kw) {
            add("form", w, &format!("kw:{kw}"));
        }
    }

    // Table cues
    if !tables.is_empty() {
        let max_rows = tables.iter().map(|t| t.rows.len() + 1).max().unwrap_or(0);
        let max_cols = tables.iter().map(|t| t.headers.len()).max().unwrap_or(0);
        if max_rows >= 3 && max_cols >= 3 {
            add("table", 0.65, &format!("grid:{max_rows}x{max_cols}"));
        } else if max_rows >= 2 && max_cols >= 2 {
            add("table", 0.35, &format!("grid:{max_rows}x{max_cols}"));
        }
    }

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (best_kind, best_score, best_signals) = &scores[0];

    if *best_score >= 0.40 {
        let conf = (0.50 + best_score * 0.45).clamp(0.55, 0.99);
        return PageClassification {
            document_type: (*best_kind).to_string(),
            confidence: (conf * 1000.0).round() / 1000.0,
            signals: best_signals.clone(),
        };
    }

    PageClassification {
        document_type: if lines.len() >= 3 {
            "document".into()
        } else {
            "other".into()
        },
        confidence: 0.60,
        signals: vec![format!("lines:{}", lines.len())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_scanned_invoice_correctly() {
        let res = classify_page("Total amount due: 12345 rupees.", &[], &[], false);
        assert_eq!(res.document_type, "invoice");
        assert!(res.confidence >= 0.70);
    }

    #[test]
    fn classifies_table_page_correctly() {
        let tbl = ExtractedTable {
            headers: vec!["Item".into(), "Qty".into(), "Price".into()],
            rows: vec![
                vec!["Pen".into(), "10".into(), "50".into()],
                vec!["Book".into(), "4".into(), "200".into()],
            ],
            cells: vec![],
            markdown: String::new(),
        };
        let res = classify_page("Item Qty Price\nPen 10 50\nBook 4 200", &[], &[tbl], false);
        assert_eq!(res.document_type, "table");
    }
}
