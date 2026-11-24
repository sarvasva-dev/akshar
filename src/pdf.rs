use anyhow::{Context, Result, bail};
use image::RgbImage;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

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

/// Offline multi-page PDF rasterization using system `pdftoppm` (Poppler).
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
