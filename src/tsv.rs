use anyhow::{Context, Result};
use image::{ImageFormat, RgbImage, imageops};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

use crate::config::project_root;
use crate::extract::{normalize_ocr_line, repair_indian_currency_lines};
use crate::table::WordBox;
use crate::tiling::OCRLine;

#[derive(Debug, Clone)]
struct TsvWord {
    block: i32,
    par: i32,
    line: i32,
    #[allow(dead_code)]
    word_idx: i32,
    bbox: [i32; 4], // [x0, y0, x1, y1]
    conf: f64,      // 0..100
    text: String,
}

pub fn tessdata_dir() -> Option<PathBuf> {
    let dir = project_root().join("assets").join("tessdata");
    if dir.join("eng.traineddata").is_file() {
        Some(dir)
    } else {
        None
    }
}

fn run_tesseract_tsv_file(img_path: &Path, tess_dir: &Path, lang: &str, psm: Option<u8>) -> Result<Vec<TsvWord>> {
    let mut cmd = Command::new("tesseract");
    cmd.arg(img_path)
        .arg("stdout")
        .arg("--tessdata-dir")
        .arg(tess_dir)
        .arg("-l")
        .arg(lang);
    if let Some(p) = psm {
        cmd.arg("--psm").arg(p.to_string());
    }
    cmd.arg("-c").arg("tessedit_create_tsv=1");

    let output = cmd.output().context("failed to execute tesseract")?;
    if !output.status.success() {
        anyhow::bail!("tesseract exited with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut words = Vec::new();
    for line in stdout.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 12 {
            continue;
        }
        let level: i32 = cols[0].parse().unwrap_or(0);
        if level != 5 {
            continue;
        }
        let txt = cols[11].trim();
        if txt.is_empty() {
            continue;
        }
        let conf: f64 = cols[10].parse().unwrap_or(-1.0);
        if conf < 0.0 {
            continue;
        }
        let left: i32 = cols[6].parse().unwrap_or(0);
        let top: i32 = cols[7].parse().unwrap_or(0);
        let w: i32 = cols[8].parse().unwrap_or(0);
        let h: i32 = cols[9].parse().unwrap_or(0);
        if w <= 0 || h <= 0 {
            continue;
        }
        words.push(TsvWord {
            block: cols[2].parse().unwrap_or(0),
            par: cols[3].parse().unwrap_or(0),
            line: cols[4].parse().unwrap_or(0),
            word_idx: cols[5].parse().unwrap_or(0),
            bbox: [left, top, left + w, top + h],
            conf,
            text: txt.to_string(),
        });
    }
    Ok(words)
}

fn word_overlap_ratio(a: [i32; 4], b: [i32; 4]) -> f64 {
    let iw = (a[2].min(b[2]) - a[0].max(b[0])).max(0) as f64;
    let ih = (a[3].min(b[3]) - a[1].max(b[1])).max(0) as f64;
    let inter = iw * ih;
    if inter == 0.0 {
        return 0.0;
    }
    let area_a = ((a[2] - a[0]).max(1) * (a[3] - a[1]).max(1)) as f64;
    let area_b = ((b[2] - b[0]).max(1) * (b[3] - b[1]).max(1)) as f64;
    inter / area_a.min(area_b)
}

fn contains_devanagari(s: &str) -> bool {
    s.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
}

fn looks_like_numeric_amount(s: &str) -> bool {
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    digits >= 2 && s.chars().all(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-' | '/'))
}

fn is_rupee_misread_token(s: &str) -> bool {
    matches!(
        s.trim(),
        "=" | "==" | "¥" | "%" | "हैं" | "है" | "₹" | "र" | "₹." | "Rs" | "Rs." | "INR"
    )
}

/// Fast (~115–220 ms), 100% deterministic document OCR with word-level bounding boxes
/// and automatic English + Hindi (`hin+eng`) confidence fusion.
/// Returns `None` if `tesseract` / `tessdata` is unavailable or the image is a natural scene photo.
pub fn run_fast_document_ocr(
    rgb: &RgbImage,
    lang: &str,
) -> Result<Option<(Vec<OCRLine>, Vec<WordBox>)>> {
    let Some(tess_dir) = tessdata_dir() else {
        return Ok(None);
    };
    let (width, height) = rgb.dimensions();
    if width < 32 || height < 32 {
        return Ok(None);
    }

    let tmp = TempDir::new()?;
    let img_path = tmp.path().join("page.png");
    rgb.save_with_format(&img_path, ImageFormat::Png)?;

    // Pass 1: fast English LSTM pass (~110 ms)
    let mut words = run_tesseract_tsv_file(&img_path, &tess_dir, "eng", None)?;
    if words.is_empty() {
        return Ok(None);
    }

    // Check if any multi-letter word has low confidence (< 74%), indicating Devanagari/Hindi script
    let has_low_conf_words = words.iter().any(|w| {
        let alpha = w.text.chars().filter(|c| c.is_alphabetic()).count();
        alpha >= 2 && w.conf < 74.0
    });
    let want_hindi = lang.eq_ignore_ascii_case("hi")
        || lang.eq_ignore_ascii_case("hin")
        || (has_low_conf_words && tess_dir.join("hin.traineddata").is_file());

    if want_hindi && tess_dir.join("hin.traineddata").is_file() {
        if let Ok(hin_words) = run_tesseract_tsv_file(&img_path, &tess_dir, "hin+eng", None) {
            // Word-by-word confidence fusion:
            // Keep English digits/codes where `eng` has high confidence (>= 80%),
            // and substitute `hin+eng` Devanagari words where `hin+eng` has higher confidence.
            for w in &mut words {
                if let Some(hw) = hin_words
                    .iter()
                    .find(|h| word_overlap_ratio(w.bbox, h.bbox) >= 0.45)
                {
                    let eng_is_strong_num = w.conf >= 82.0
                        && w.text.chars().any(|c| c.is_ascii_digit())
                        && !contains_devanagari(&hw.text);
                    if !eng_is_strong_num
                        && (hw.conf > w.conf + 5.0
                            || (contains_devanagari(&hw.text) && hw.conf >= 65.0 && w.conf < 78.0))
                    {
                        w.text = hw.text.clone();
                        w.conf = hw.conf;
                    }
                }
            }
        }
    }

    // Filter out low-confidence noise speckles
    words.retain(|w| w.conf >= 32.0 && !w.text.trim().is_empty());
    if words.is_empty() {
        return Ok(None);
    }

    let mean_conf: f64 = words.iter().map(|w| w.conf).sum::<f64>() / (words.len() as f64);
    // Natural scene photos or dark complex app screenshots where Tesseract struggles fall back to Mirage
    if mean_conf < 58.0 {
        return Ok(None);
    }

    // Repair standalone `₹` tokens immediately preceding a numeric amount on the same line
    for i in 0..words.len().saturating_sub(1) {
        let same_line = words[i].block == words[i + 1].block
            && words[i].par == words[i + 1].par
            && words[i].line == words[i + 1].line;
        if same_line
            && is_rupee_misread_token(&words[i].text)
            && looks_like_numeric_amount(&words[i + 1].text)
        {
            words[i].text = "₹".into();
            words[i].conf = words[i].conf.max(90.0);
        }
    }

    // Group words into lines by (block, par, line)
    let mut line_groups: Vec<Vec<&TsvWord>> = Vec::new();
    for w in &words {
        let same = line_groups
            .last()
            .and_then(|g| g.first())
            .map(|first| first.block == w.block && first.par == w.par && first.line == w.line)
            .unwrap_or(false);
        if same {
            line_groups.last_mut().unwrap().push(w);
        } else {
            line_groups.push(vec![w]);
        }
    }

    let mut ocr_lines = Vec::with_capacity(line_groups.len());
    let mut word_boxes = Vec::with_capacity(words.len());

    for w in &words {
        word_boxes.push(WordBox {
            text: normalize_ocr_line(&w.text),
            bbox: w.bbox,
        });
    }

    for grp in line_groups {
        let x0 = grp.iter().map(|w| w.bbox[0]).min().unwrap_or(0);
        let y0 = grp.iter().map(|w| w.bbox[1]).min().unwrap_or(0);
        let x1 = grp.iter().map(|w| w.bbox[2]).max().unwrap_or(0);
        let y1 = grp.iter().map(|w| w.bbox[3]).max().unwrap_or(0);
        let avg_c = grp.iter().map(|w| w.conf).sum::<f64>() / (grp.len() as f64) / 100.0;
        let joined = grp
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let cleaned = normalize_ocr_line(&joined);
        if cleaned.is_empty() {
            continue;
        }
        ocr_lines.push(OCRLine {
            text: cleaned,
            confidence: (avg_c.clamp(0.0, 1.0) * 10000.0).round() / 10000.0,
            bbox: [x0, y0, x1, y1],
            bbox_norm: [
                ((x0 as f64) / (width as f64) * 1000.0).round() as i32,
                ((y0 as f64) / (height as f64) * 1000.0).round() as i32,
                ((x1 as f64) / (width as f64) * 1000.0).round() as i32,
                ((y1 as f64) / (height as f64) * 1000.0).round() as i32,
            ],
        });
    }

    repair_indian_currency_lines(&mut ocr_lines);
    Ok(Some((ocr_lines, word_boxes)))
}

/// Crop a single bounding box from `rgb` and decode it with `hin+eng` (--psm 7) when Mirage
/// produces a garbled or low-confidence line on a mixed Hindi/English photo.
pub fn recover_garbled_box_crop(rgb: &RgbImage, bbox: [i32; 4]) -> Option<String> {
    let tess_dir = tessdata_dir()?;
    if !tess_dir.join("hin.traineddata").is_file() {
        return None;
    }
    let (w, h) = rgb.dimensions();
    let pad = 4i32;
    let x0 = (bbox[0] - pad).clamp(0, w as i32) as u32;
    let y0 = (bbox[1] - pad).clamp(0, h as i32) as u32;
    let x1 = (bbox[2] + pad).clamp(0, w as i32) as u32;
    let y1 = (bbox[3] + pad).clamp(0, h as i32) as u32;
    if x1 <= x0 + 8 || y1 <= y0 + 8 {
        return None;
    }
    let crop = imageops::crop_imm(rgb, x0, y0, x1 - x0, y1 - y0).to_image();
    let tmp = TempDir::new().ok()?;
    let p = tmp.path().join("crop.png");
    crop.save_with_format(&p, ImageFormat::Png).ok()?;
    let words = run_tesseract_tsv_file(&p, &tess_dir, "hin+eng", Some(7)).ok()?;
    if words.is_empty() {
        return None;
    }
    let avg_conf = words.iter().map(|w| w.conf).sum::<f64>() / (words.len() as f64);
    if avg_conf < 62.0 {
        return None;
    }
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| normalize_ocr_line(trimmed))
}
