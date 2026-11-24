use image::{Rgb, RgbImage};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::tiling::OCRLine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionMatch {
    pub kind: String,
    pub bbox: [i32; 4],
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionSummary {
    pub redacted_count: usize,
    pub matches: Vec<RedactionMatch>,
    pub redacted_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactStyle {
    SolidBlack,
    Pixelate,
}

struct Rule {
    kind: &'static str,
    regex: Regex,
}

fn default_pii_rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            Rule {
                kind: "email",
                regex: Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap(),
            },
            Rule {
                kind: "aadhaar_id",
                regex: Regex::new(r"\b\d{4}[\s\-]\d{4}[\s\-]\d{4}\b").unwrap(),
            },
            Rule {
                kind: "pan_id",
                regex: Regex::new(r"\b[A-Z]{5}\d{4}[A-Z]\b").unwrap(),
            },
            Rule {
                kind: "card_number",
                regex: Regex::new(r"\b(?:\d{4}[\s\-]?){3}\d{4}\b").unwrap(),
            },
            Rule {
                kind: "phone",
                regex: Regex::new(r"(?:\+?\d{1,3}[\s\-]?)?(?:\(?\d{3,5}\)?[\s\-]?\d{3,5}(?:[\s\-]?\d{3,5})?|\b\d{10,12}\b)").unwrap(),
            },
        ]
    })
}

fn match_pii_kind(text: &str, custom_terms: &[String]) -> Option<String> {
    let lower = text.to_lowercase();
    for term in custom_terms {
        let t = term.trim().to_lowercase();
        if !t.is_empty() && lower.contains(&t) {
            return Some("custom".into());
        }
    }
    for rule in default_pii_rules() {
        if let Some(m) = rule.regex.find(text) {
            if rule.kind == "phone" {
                let digits = m.as_str().chars().filter(|c| c.is_ascii_digit()).count();
                if !(10..=15).contains(&digits) {
                    continue;
                }
            }
            return Some(rule.kind.to_string());
        }
    }
    None
}

pub fn redact_text(text: &str, custom_terms: &[String]) -> String {
    let mut out = text.to_string();
    for rule in default_pii_rules() {
        out = rule
            .regex
            .replace_all(&out, |caps: &regex::Captures| {
                let matched = caps.get(0).map(|m| m.as_str()).unwrap_or("");
                if rule.kind == "phone" {
                    let digits = matched.chars().filter(|c| c.is_ascii_digit()).count();
                    if !(10..=15).contains(&digits) {
                        return matched.to_string();
                    }
                }
                format!("[REDACTED:{}]", rule.kind.to_uppercase())
            })
            .into_owned();
    }
    for term in custom_terms {
        let t = term.trim();
        if t.is_empty() {
            continue;
        }
        if let Ok(re) = Regex::new(&format!("(?i){}", regex::escape(t))) {
            out = re.replace_all(&out, "[REDACTED:CUSTOM]").into_owned();
        }
    }
    out
}

/// Paint solid black or pixelated redaction boxes over detected PII lines on the image.
pub fn redact_image(
    rgb: &RgbImage,
    lines: &[OCRLine],
    custom_terms: &[String],
    style: RedactStyle,
) -> (RgbImage, RedactionSummary) {
    let mut out = rgb.clone();
    let (w, h) = out.dimensions();
    let mut matches = Vec::new();
    let mut text_lines = Vec::with_capacity(lines.len());

    for ln in lines {
        text_lines.push(redact_text(&ln.text, custom_terms));
        if let Some(kind) = match_pii_kind(&ln.text, custom_terms) {
            let pad = 4i32;
            let x0 = (ln.bbox[0] - pad).clamp(0, w as i32) as u32;
            let y0 = (ln.bbox[1] - pad).clamp(0, h as i32) as u32;
            let x1 = (ln.bbox[2] + pad).clamp(0, w as i32) as u32;
            let y1 = (ln.bbox[3] + pad).clamp(0, h as i32) as u32;

            match style {
                RedactStyle::SolidBlack => {
                    for y in y0..y1 {
                        for x in x0..x1 {
                            out.put_pixel(x, y, Rgb([14, 14, 18]));
                        }
                    }
                }
                RedactStyle::Pixelate => {
                    let block = ((y1.saturating_sub(y0)) / 3).clamp(6, 18);
                    let mut by = y0;
                    while by < y1 {
                        let by_end = (by + block).min(y1);
                        let mut bx = x0;
                        while bx < x1 {
                            let bx_end = (bx + block).min(x1);
                            let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
                            for y in by..by_end {
                                for x in bx..bx_end {
                                    let p = out.get_pixel(x, y);
                                    r += p[0] as u64;
                                    g += p[1] as u64;
                                    b += p[2] as u64;
                                    n += 1;
                                }
                            }
                            if n > 0 {
                                let avg = Rgb([(r / n) as u8, (g / n) as u8, (b / n) as u8]);
                                for y in by..by_end {
                                    for x in bx..bx_end {
                                        out.put_pixel(x, y, avg);
                                    }
                                }
                            }
                            bx += block;
                        }
                        by += block;
                    }
                }
            }

            matches.push(RedactionMatch {
                kind,
                bbox: [x0 as i32, y0 as i32, x1 as i32, y1 as i32],
                confidence: ln.confidence,
            });
        }
    }

    let summary = RedactionSummary {
        redacted_count: matches.len(),
        matches,
        redacted_text: text_lines.join("\n"),
    };
    (out, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email_phone_and_custom_terms() {
        let img = RgbImage::from_pixel(300, 120, Rgb([255, 255, 255]));
        let lines = vec![
            OCRLine { text: "Email: alice@example.com".into(), confidence: 0.9, bbox: [10, 10, 260, 40], bbox_norm: [0; 4] },
            OCRLine { text: "Safe Header".into(), confidence: 0.9, bbox: [10, 50, 160, 75], bbox_norm: [0; 4] },
            OCRLine { text: "Project Falcon Secret".into(), confidence: 0.85, bbox: [10, 80, 220, 105], bbox_norm: [0; 4] },
        ];
        let (redacted, summary) = redact_image(&img, &lines, &["Falcon".into()], RedactStyle::SolidBlack);
        assert_eq!(summary.redacted_count, 2);
        assert!(summary.redacted_text.contains("[REDACTED:EMAIL]"));
        assert!(summary.redacted_text.contains("[REDACTED:CUSTOM]"));
        assert_eq!(*redacted.get_pixel(50, 25), Rgb([14, 14, 18]));
        assert_eq!(*redacted.get_pixel(50, 62), Rgb([255, 255, 255]));
    }
}
