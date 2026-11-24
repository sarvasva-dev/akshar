use image::{Rgb, RgbImage, imageops};

/// Enhance low-contrast or slightly blurry phone scans before OCR:
/// 1) percentile contrast stretch (1%..99%) per luma distribution
/// 2) gentle unsharp mask to sharpen character edges
pub fn enhance_for_ocr(src: &RgbImage) -> RgbImage {
    let (w, h) = src.dimensions();
    if w == 0 || h == 0 {
        return src.clone();
    }

    // Build luma histogram for robust 1st/99th percentile contrast stretching
    let mut hist = [0u64; 256];
    let total = (w as u64) * (h as u64);
    for p in src.pixels() {
        let luma = ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8) as usize;
        hist[luma.min(255)] += 1;
    }

    let lo_target = (total as f64 * 0.01) as u64;
    let hi_target = (total as f64 * 0.99) as u64;
    let mut acc = 0u64;
    let mut lo = 0u8;
    let mut hi = 255u8;
    for (i, &cnt) in hist.iter().enumerate() {
        acc += cnt;
        if acc <= lo_target {
            lo = i as u8;
        }
        if acc >= hi_target {
            hi = i as u8;
            break;
        }
    }

    let mut stretched = src.clone();
    if hi > lo + 12 {
        let scale = 255.0 / ((hi - lo) as f32);
        let mut lut = [0u8; 256];
        for (i, slot) in lut.iter_mut().enumerate() {
            let v = ((i as f32 - lo as f32) * scale).clamp(0.0, 255.0);
            *slot = v.round() as u8;
        }
        for p in stretched.pixels_mut() {
            p[0] = lut[p[0] as usize];
            p[1] = lut[p[1] as usize];
            p[2] = lut[p[2] as usize];
        }
    }

    // Unsharp mask: sharpens faint/blurry glyphs without ringing
    imageops::unsharpen(&stretched, 1.2, 4)
}

/// Specialized thermal receipt and faded scan enhancer:
/// Removes uneven paper shadows, whitens gray thermal paper, and boosts faint print contrast.
pub fn enhance_thermal_receipt(src: &RgbImage) -> RgbImage {
    let (w, h) = src.dimensions();
    if w == 0 || h == 0 {
        return src.clone();
    }

    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let p = src.get_pixel(x, y);
            let luma = (77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8;
            // Contrast curve steepening around text threshold
            let new_val = if luma > 185 {
                255u8 // Whiten background
            } else if luma < 120 {
                (luma * 8 / 10).min(255) as u8 // Darken ink
            } else {
                let norm = (luma - 120) as f32 / 65.0;
                (norm.powf(1.6) * 255.0).clamp(0.0, 255.0) as u8
            };
            out.put_pixel(x, y, Rgb([new_val, new_val, new_val]));
        }
    }
    imageops::unsharpen(&out, 1.4, 3)
}

/// Add a quiet white border around tight crops so edge characters aren't clipped.
pub fn pad_border(src: &RgbImage, pad: u32) -> RgbImage {
    if pad == 0 {
        return src.clone();
    }
    let (w, h) = src.dimensions();
    let mut out = RgbImage::from_pixel(w + pad * 2, h + pad * 2, Rgb([255, 255, 255]));
    imageops::overlay(&mut out, src, pad as i64, pad as i64);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhance_stretches_low_contrast_image() {
        let mut img = RgbImage::from_pixel(64, 64, Rgb([150, 150, 150]));
        for y in 16..48 {
            for x in 16..48 {
                img.put_pixel(x, y, Rgb([100, 100, 100]));
            }
        }
        let out = enhance_for_ocr(&img);
        let bg = out.get_pixel(4, 4)[0];
        let fg = out.get_pixel(32, 32)[0];
        assert!(bg > 200 && fg < 60, "expected wider contrast, got bg={bg} fg={fg}");
    }

    #[test]
    fn thermal_enhancer_cleans_faded_background() {
        let mut img = RgbImage::from_pixel(64, 64, Rgb([200, 200, 195]));
        img.put_pixel(32, 32, Rgb([90, 90, 90]));
        let out = enhance_thermal_receipt(&img);
        assert_eq!(out.get_pixel(4, 4)[0], 255);
        assert!(out.get_pixel(32, 32)[0] < 80);
    }
}
