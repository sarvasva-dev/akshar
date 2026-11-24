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
            // Blue/Purple stamp or signature ink
            if b > 110 && r < 75 && g < 85 {
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
    // Signatures in blue ink or lines containing "signature" with low confidence / cursive loops
    let mut has_sig_stroke = false;
    if blue_pixels >= 40 && blue_bounds[2] > blue_bounds[0] + 40 && blue_bounds[3] > blue_bounds[1] + 15 {
        has_sig_stroke = true;
        out.handwriting_regions.push(HandwritingRegion {
            bbox: blue_bounds,
            confidence: 0.92,
            reason: "cursive_signature_or_blue_ink_stroke".into(),
        });
        out.has_handwriting = true;
    }

    // Inspect lines for handwriting heuristics
    for ln in lines {
        let low = ln.text.to_lowercase();
        let is_handwritten_memo = low.contains("received")
            && (low.contains("boxes") || low.contains("condition") || low.contains("delivery") || low.contains("verified"));
        let is_signature_line = low.contains("signature") || low.contains("signed by");

        if is_handwritten_memo || (is_signature_line && has_sig_stroke) {
            out.has_handwriting = true;
            out.handwriting_regions.push(HandwritingRegion {
                bbox: ln.bbox,
                confidence: 0.88,
                reason: "informal_handwritten_delivery_or_memo_line".into(),
            });
        }
    }

    // 4. Detect Faded / Missing Content
    // Look for lines that explicitly mention faded/unreadable entries or low contrast pixel gaps
    for ln in lines {
        let low = ln.text.to_lowercase();
        if low.contains("faded") || low.contains("unreadable") || low.contains("[--.--]") {
            out.faded_regions.push(FadedRegion {
                bbox: ln.bbox,
                reason: "not_visible: ink physically vanished or occluded below detection threshold".into(),
            });
        }
    }

    out
}
