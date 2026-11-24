use anyhow::{Context, Result, bail};
use image::RgbImage;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use tempfile::TempDir;

use crate::table::WordBox;
use crate::tiling::OCRLine;

#[derive(Debug, Clone)]
pub struct NativePdfPage {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub lines: Vec<OCRLine>,
    pub words: Vec<WordBox>,
}

impl NativePdfPage {
    pub fn has_usable_text(&self) -> bool {
        let alnum = self.text.chars().filter(|c| c.is_alphanumeric()).count();
        alnum >= 12
    }
}

pub fn is_pdf_bytes(data: &[u8], filename: Option<&str>) -> bool {
    if data.len() >= 5 && &data[..5] == b"%PDF-" {
        return true;
    }
    filename
        .map(|n| n.to_ascii_lowercase().ends_with(".pdf"))
        .unwrap_or(false)
}

pub fn is_pdf_path(path: &Path) -> bool {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
    {
        return true;
    }
    if let Ok(mut f) = fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 5];
        if f.read_exact(&mut buf).is_ok() {
            return &buf == b"%PDF-";
        }
    }
    false
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

fn re_page_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?s)<page\s+width="([0-9.]+)"\s+height="([0-9.]+)">(.*?)</page>"#).unwrap()
    })
}

fn re_line_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?s)<line\s+xMin="([0-9.\-]+)"\s+yMin="([0-9.\-]+)"\s+xMax="([0-9.\-]+)"\s+yMax="([0-9.\-]+)">(.*?)</line>"#).unwrap()
    })
}

fn re_word_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<word\s+xMin="([0-9.\-]+)"\s+yMin="([0-9.\-]+)"\s+xMax="([0-9.\-]+)"\s+yMax="([0-9.\-]+)">([^<]*)</word>"#).unwrap()
    })
}

/// Fast native PDF text + exact word/line bounding box extraction via `pdftotext -bbox-layout`.
/// Runs in milliseconds and yields exact, deterministic Unicode text for digital PDF pages.
pub fn extract_native_pdf_pages(path: &Path, max_pages: usize) -> Result<Vec<NativePdfPage>> {
    let last_page = max_pages.max(1).to_string();
    let output = Command::new("pdftotext")
        .arg("-bbox-layout")
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg(&last_page)
        .arg(path)
        .arg("-")
        .output()
        .context("failed to invoke `pdftotext` (install poppler-utils for PDF support)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pdftotext failed: {}", stderr.trim());
    }

    let xml = String::from_utf8_lossy(&output.stdout);
    let mut pages = Vec::new();

    for (idx, pcap) in re_page_tag().captures_iter(&xml).enumerate() {
        let pw: f64 = pcap[1].parse::<f64>().unwrap_or(612.0).max(1.0);
        let ph: f64 = pcap[2].parse::<f64>().unwrap_or(792.0).max(1.0);
        let body = &pcap[3];

        let mut lines = Vec::new();
        let mut words = Vec::new();
        let mut text_lines = Vec::new();

        for lcap in re_line_tag().captures_iter(body) {
            let lx0: f64 = lcap[1].parse().unwrap_or(0.0);
            let ly0: f64 = lcap[2].parse().unwrap_or(0.0);
            let lx1: f64 = lcap[3].parse().unwrap_or(0.0);
            let ly1: f64 = lcap[4].parse().unwrap_or(0.0);
            let line_body = &lcap[5];

            let mut line_words = Vec::new();
            for wcap in re_word_tag().captures_iter(line_body) {
                let wx0: f64 = wcap[1].parse().unwrap_or(0.0);
                let wy0: f64 = wcap[2].parse().unwrap_or(0.0);
                let wx1: f64 = wcap[3].parse().unwrap_or(0.0);
                let wy1: f64 = wcap[4].parse().unwrap_or(0.0);
                let wtxt = unescape_xml(&wcap[5]).trim().to_string();
                if wtxt.is_empty() {
                    continue;
                }
                words.push(WordBox {
                    text: wtxt.clone(),
                    bbox: [
                        wx0.round() as i32,
                        wy0.round() as i32,
                        wx1.round() as i32,
                        wy1.round() as i32,
                    ],
                });
                line_words.push(wtxt);
            }

            let joined = line_words.join(" ");
            if joined.is_empty() {
                continue;
            }
            text_lines.push(joined.clone());
            lines.push(OCRLine {
                text: joined,
                confidence: 1.0,
                bbox: [
                    lx0.round() as i32,
                    ly0.round() as i32,
                    lx1.round() as i32,
                    ly1.round() as i32,
                ],
                bbox_norm: [
                    ((lx0 / pw) * 1000.0).round() as i32,
                    ((ly0 / ph) * 1000.0).round() as i32,
                    ((lx1 / pw) * 1000.0).round() as i32,
                    ((ly1 / ph) * 1000.0).round() as i32,
                ],
            });
        }

        pages.push(NativePdfPage {
            page: idx + 1,
            width: pw.round() as u32,
            height: ph.round() as u32,
            text: text_lines.join("\n"),
            lines,
            words,
        });
    }

    Ok(pages)
}

/// Rasterize only a single specific 1-based page of a PDF (used when a page has no native text layer).
pub fn render_single_pdf_page(path: &Path, page_num: usize, dpi: u32) -> Result<RgbImage> {
    let tmp = TempDir::new().context("failed to create temp dir for PDF page")?;
    let prefix = tmp.path().join("page");
    let dpi_clamped = dpi.max(72);
    let pstr = page_num.max(1).to_string();

    let output = Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg(dpi_clamped.to_string())
        .arg("-f")
        .arg(&pstr)
        .arg("-l")
        .arg(&pstr)
        .arg(path)
        .arg(&prefix)
        .output()
        .context("failed to invoke `pdftoppm`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pdftoppm failed on page {page_num}: {}", stderr.trim());
    }

    let png = fs::read_dir(tmp.path())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("png"))
        .ok_or_else(|| anyhow::anyhow!("pdftoppm produced no PNG for page {page_num}"))?;

    Ok(image::open(&png)?.to_rgb8())
}

/// Rasterize all pages (up to max_pages) from PDF bytes.
pub fn render_pdf_bytes(data: &[u8], dpi: u32, max_pages: usize) -> Result<Vec<RgbImage>> {
    let tmp = TempDir::new().context("failed to create temp dir for PDF rasterization")?;
    let input_pdf = tmp.path().join("input.pdf");
    fs::write(&input_pdf, data).context("failed to write temp PDF")?;
    render_pdf_file(&input_pdf, dpi, max_pages)
}

pub fn render_pdf_file(path: &Path, dpi: u32, max_pages: usize) -> Result<Vec<RgbImage>> {
    let tmp = TempDir::new().context("failed to create temp dir for PDF pages")?;
    let prefix = tmp.path().join("page");
    let dpi_clamped = dpi.max(72);
    let last_page = max_pages.max(1).to_string();

    let output = Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg(dpi_clamped.to_string())
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg(&last_page)
        .arg(path)
        .arg(&prefix)
        .output()
        .context("failed to invoke `pdftoppm` (install poppler-utils for PDF OCR)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pdftoppm failed: {}", stderr.trim());
    }

    let mut page_files: Vec<_> = fs::read_dir(tmp.path())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("png"))
        .collect();
    page_files.sort();

    let mut pages = Vec::with_capacity(page_files.len());
    for page_path in page_files.into_iter().take(max_pages.max(1)) {
        let img = image::open(&page_path)
            .with_context(|| format!("failed to decode rasterized PDF page {}", page_path.display()))?
            .to_rgb8();
        pages.push(img);
    }
    Ok(pages)
}
