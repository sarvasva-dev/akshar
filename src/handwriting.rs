use image::RgbImage;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::tiling::OCRLine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandwritingRegion {
    pub bbox: [i32; 4],
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedCheckbox {
    pub label: String,
    pub checked: bool,
    pub bbox: [i32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StampRegion {
    pub bbox: [i32; 4],
    pub color_tag: String, // "red", "blue", "purple"
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FadedRegion {
    pub bbox: [i32; 4],
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisualArtifacts {
    pub has_handwriting: bool,
    pub handwriting_regions: Vec<HandwritingRegion>,
    pub checkboxes: Vec<DetectedCheckbox>,
    pub stamps: Vec<StampRegion>,
    pub faded_regions: Vec<FadedRegion>,
}

pub fn detect_visual_artifacts(rgb: &RgbImage, lines: &[OCRLine]) -> VisualArtifacts {
    let mut out = VisualArtifacts::default();
    let (width, height) = rgb.dimensions();

    // 1. Detect Checkboxes from lines and text glyphs
    static RE_CHECKBOX_CHECKED: OnceLock<Regex> = OnceLock::new();
    static RE_CHECKBOX_UNCHECKED: OnceLock<Regex> = OnceLock::new();
    let re_chk = RE_CHECKBOX_CHECKED.get_or_init(|| {
        Regex::new(r"(?i)(?:\[\s*[xX✓vV]{1,2}\s*\]|1[xX]1|D[xX]\]\s*[xX]\])\s*(.*)").unwrap()
    });
    let re_unchk = RE_CHECKBOX_UNCHECKED.get_or_init(|| {
        Regex::new(r"(?i)(?:\[\s*\]|\[\s*1\s*|^\])\s*(.*)").unwrap()
    });

    for ln in lines {
        let text_clean = ln.text.trim();
        if let Some(caps) = re_chk.captures(text_clean) {
            let label = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("").to_string();
            if !label.is_empty() {
                out.checkboxes.push(DetectedCheckbox {
                    label,
                    checked: true,
                    bbox: ln.bbox,
                });
            }
        } else if let Some(caps) = re_unchk.captures(text_clean) {
            let label = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("").to_string();
            if !label.is_empty() {
                out.checkboxes.push(DetectedCheckbox {
                    label,
                    checked: false,
                    bbox: ln.bbox,
                });
            }
        } else if let Some(pos) = text_clean.find("[X]").or_else(|| text_clean.find("[x]")).or_else(|| text_clean.find("[Xx]")) {
            let label = text_clean[pos + 3..].trim().trim_start_matches(|c: char| c == ']' || c == ' ').to_string();
            if !label.is_empty() {
                out.checkboxes.push(DetectedCheckbox {
                    label,
                    checked: true,
                    bbox: ln.bbox,
                });
            }
        } else if let Some(pos) = text_clean.find("[ ]").or_else(|| text_clean.find("[]")) {
            let label = text_clean[pos + 2..].trim().trim_start_matches(|c: char| c == ']' || c == ' ').to_string();
            if !label.is_empty() {
                out.checkboxes.push(DetectedCheckbox {
                    label,
                    checked: false,
                    bbox: ln.bbox,
                });
            }
        }
    }

    // 2. Detect Stamps & Colored Seals (Red, Blue/Purple ink patches overlapping text)
    // Sample pixels in regions where red or blue dominates over grayscale
    let mut red_pixels = 0usize;
    let mut blue_pixels = 0usize;
    let mut red_bounds = [i32::MAX, i32::MAX, 0i32, 0i32];
    let mut blue_bounds = [i32::MAX, i32::MAX, 0i32, 0i32];

    for y in (0..height).step_by(4) {
        for x in (0..width).step_by(4) {
            let p = rgb.get_pixel(x, y);
            let r = p[0] as i32;
            let g = p[1] as i32;
            let b = p[2] as i32;

            // Red stamp: High red, low green and blue
            if r > 140 && g < 90 && b < 90 {
                red_pixels += 1;
                red_bounds[0] = red_bounds[0].min(x as i32);
                red_bounds[1] = red_bounds[1].min(y as i32);
                red_bounds[2] = red_bounds[2].max(x as i32);
                red_bounds[3] = red_bounds[3].max(y as i32);
            }
            // Blue/Purple stamp or signature ink (captures royal blue and dark navy ink)
            if b >= 60 && b > r + 10 && b > g + 10 {
                blue_pixels += 1;
                blue_bounds[0] = blue_bounds[0].min(x as i32);
                blue_bounds[1] = blue_bounds[1].min(y as i32);
                blue_bounds[2] = blue_bounds[2].max(x as i32);
                blue_bounds[3] = blue_bounds[3].max(y as i32);
            }
        }
    }

    if red_pixels >= 45 && red_bounds[2] > red_bounds[0] + 30 && red_bounds[3] > red_bounds[1] + 30 {
        out.stamps.push(StampRegion {
            bbox: red_bounds,
            color_tag: "red".into(),
            confidence: 0.95,
        });
    }

    // 3. Detect Handwriting & Signatures
    let mut has_sig_stroke = false;
    if blue_pixels >= 25 && blue_bounds[2] > blue_bounds[0] + 35 && blue_bounds[3] > blue_bounds[1] + 10 {
        has_sig_stroke = true;
        out.handwriting_regions.push(HandwritingRegion {
            bbox: blue_bounds,
            confidence: 0.92,
            reason: "cursive_signature_or_blue_ink_stroke".into(),
        });
        out.has_handwriting = true;
    }

    // Inspect lines for signature prompts and black-ink handwriting
    for (idx, ln) in lines.iter().enumerate() {
        let low = ln.text.to_lowercase();
        let is_handwritten_memo = low.contains("received")
            && (low.contains("boxes") || low.contains("condition") || low.contains("delivery") || low.contains("verified"));
        let is_signature_line = low.contains("signature") || low.contains("signed by") || low.contains("applicant signature");

        // Check for black-ink pen strokes in signature area
        if is_signature_line {
            let sig_x0 = ln.bbox[2];
            let sig_x1 = (sig_x0 + 260).min(width as i32 - 1);
            let sig_y0 = (ln.bbox[1] - 15).max(0);
            let sig_y1 = (ln.bbox[3] + 20).min(height as i32 - 1);
            let mut dark_pen_pixels = 0usize;
            for sy in (sig_y0..sig_y1).step_by(2) {
                for sx in (sig_x0..sig_x1).step_by(2) {
                    let p = rgb.get_pixel(sx as u32, sy as u32);
                    let luma = (p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000;
                    if luma < 130 {
                        dark_pen_pixels += 1;
                    }
                }
            }
            if dark_pen_pixels >= 15 {
                has_sig_stroke = true;
                out.has_handwriting = true;
                out.handwriting_regions.push(HandwritingRegion {
                    bbox: [sig_x0, sig_y0, sig_x1, sig_y1],
                    confidence: 0.90,
                    reason: "pen_signature_stroke_detected_in_signature_field".into(),
                });
            }
        }

        if is_handwritten_memo || (is_signature_line && has_sig_stroke) {
            out.has_handwriting = true;
            if !out.handwriting_regions.iter().any(|r| r.bbox == ln.bbox) {
                out.handwriting_regions.push(HandwritingRegion {
                    bbox: ln.bbox,
                    confidence: 0.88,
                    reason: "informal_handwritten_delivery_or_memo_line".into(),
                });
            }
        }

        // Detect item numbering sequence jumps (e.g. Item 1 then Item 3 -> Item 2 faded)
        if idx > 0 {
            let prev_low = lines[idx - 1].text.to_lowercase();
            if prev_low.contains("item 1") && low.contains("item 3") {
                let gap_y0 = lines[idx - 1].bbox[3];
                let gap_y1 = ln.bbox[1];
                if gap_y1 > gap_y0 + 20 {
                    out.faded_regions.push(FadedRegion {
                        bbox: [lines[idx - 1].bbox[0], gap_y0, width as i32 - 50, gap_y1],
                        reason: "not_visible: line item sequence jump detected (ink physically vanished)".into(),
                    });
                }
            }
        }
    }

    // 4. Detect Faded / Missing Content from text keywords or low contrast
    for ln in lines {
        let low = ln.text.to_lowercase();
        if low.contains("faded") || low.contains("unreadable") || low.contains("[--.--]") || low.contains("subtotal legible") {
            if !out.faded_regions.iter().any(|r| r.bbox == ln.bbox) {
                out.faded_regions.push(FadedRegion {
                    bbox: ln.bbox,
                    reason: "not_visible: ink physically degraded or occluded below detection threshold".into(),
                });
            }
        }
    }

    out
}
