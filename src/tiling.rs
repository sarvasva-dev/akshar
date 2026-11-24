use anyhow::Result;
use image::{imageops, RgbImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OCRLine {
    pub text: String,
    pub confidence: f64,
    pub bbox: [i32; 4],      // [x0, y0, x1, y1] in pixels
    pub bbox_norm: [i32; 4], // [x0, y0, x1, y1] in 0..1000
}

pub fn compute_tiles(
    width: u32,
    height: u32,
    tile_height: Option<u32>,
    overlap_ratio: f64,
) -> Vec<(u32, u32)> {
    let th = tile_height.unwrap_or_else(|| {
        let target = ((width as f64) * 0.72) as u32;
        target.min(height).max(320)
    });
    if height <= th {
        return vec![(0, height)];
    }

    let overlap = (((th as f64) * overlap_ratio) as u32).max(32);
    let step = th.saturating_sub(overlap).max(64);
    let mut tiles = Vec::new();
    let mut y = 0u32;
    while y < height {
        let y1 = (y + th).min(height);
        let y0 = y1.saturating_sub(th);
        tiles.push((y0, y1));
        if y1 >= height {
            break;
        }
        y += step;
    }
    tiles
}

fn box_area(b: [i32; 4]) -> f64 {
    ((b[2] - b[0]).max(1) as f64) * ((b[3] - b[1]).max(1) as f64)
}

fn intersection_area(a: [i32; 4], b: [i32; 4]) -> f64 {
    let iw = (a[2].min(b[2]) - a[0].max(b[0])).max(0) as f64;
    let ih = (a[3].min(b[3]) - a[1].max(b[1])).max(0) as f64;
    iw * ih
}

fn box_iou(a: [i32; 4], b: [i32; 4]) -> f64 {
    let inter = intersection_area(a, b);
    if inter == 0.0 {
        return 0.0;
    }
    inter / (box_area(a) + box_area(b) - inter)
}

fn containment_ratio(a: [i32; 4], b: [i32; 4]) -> f64 {
    let inter = intersection_area(a, b);
    if inter == 0.0 {
        return 0.0;
    }
    inter / box_area(a).min(box_area(b))
}

fn vertical_overlap_ratio(a: [i32; 4], b: [i32; 4]) -> f64 {
    let iy = (a[3].min(b[3]) - a[1].max(b[1])).max(0) as f64;
    let min_h = (a[3] - a[1]).min(b[3] - b[1]).max(1) as f64;
    iy / min_h
}

fn horizontal_overlap_ratio(a: [i32; 4], b: [i32; 4]) -> f64 {
    let ix = (a[2].min(b[2]) - a[0].max(b[0])).max(0) as f64;
    let min_w = (a[2] - a[0]).min(b[2] - b[0]).max(1) as f64;
    ix / min_w
}

/// Normalized Levenshtein similarity in [0.0, 1.0].
pub fn text_similarity(a: &str, b: &str) -> f64 {
    let sa: Vec<char> = a.trim().to_lowercase().chars().collect();
    let sb: Vec<char> = b.trim().to_lowercase().chars().collect();
    let max_len = sa.len().max(sb.len());
    if max_len == 0 {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=sb.len()).collect();
    let mut curr = vec![0usize; sb.len() + 1];
    for (i, ca) in sa.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in sb.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    1.0 - (prev[sb.len()] as f64) / (max_len as f64)
}

fn is_duplicate(a: &OCRLine, b: &OCRLine) -> bool {
    if box_iou(a.bbox, b.bbox) >= 0.45 {
        return true;
    }
    if containment_ratio(a.bbox, b.bbox) >= 0.65 {
        return true;
    }
    if vertical_overlap_ratio(a.bbox, b.bbox) >= 0.55
        && horizontal_overlap_ratio(a.bbox, b.bbox) >= 0.45
        && text_similarity(&a.text, &b.text) >= 0.65
    {
        return true;
    }
    false
}

fn prefer_candidate(candidate: &OCRLine, existing: &OCRLine) -> bool {
    let area_c = box_area(candidate.bbox);
    let area_e = box_area(existing.bbox);
    if area_c > area_e * 1.4 && candidate.text.len() > existing.text.len() {
        return true;
    }
    if area_e > area_c * 1.4 && existing.text.len() > candidate.text.len() {
        return false;
    }
    if candidate.confidence > existing.confidence + 0.03 {
        return true;
    }
    if (candidate.confidence - existing.confidence).abs() <= 0.03
        && candidate.text.len() > existing.text.len()
    {
        return true;
    }
    false
}

pub fn sort_reading_order(mut lines: Vec<OCRLine>) -> Vec<OCRLine> {
    if lines.len() <= 2 {
        lines.sort_by_key(|l| (l.bbox[1], l.bbox[0]));
        return lines;
    }

    // 1. Check for multi-column layout with a vertical gutter
    let min_x = lines.iter().map(|l| l.bbox[0]).min().unwrap_or(0);
    let max_x = lines.iter().map(|l| l.bbox[2]).max().unwrap_or(1);
    let page_w = (max_x - min_x).max(1);

    if page_w > 300 && lines.len() >= 4 {
        // Test gutter candidate around the middle (42% .. 58% of page width)
        let gutter_left = min_x + (page_w * 42 / 100);
        let gutter_right = min_x + (page_w * 58 / 100);

        let mut left_col = Vec::new();
        let mut right_col = Vec::new();
        let mut spanning_lines = Vec::new();

        for ln in &lines {
            let cx = (ln.bbox[0] + ln.bbox[2]) / 2;
            let spans = ln.bbox[0] < gutter_left && ln.bbox[2] > gutter_right;

            if spans {
                spanning_lines.push(ln.clone());
            } else if cx < gutter_left {
                left_col.push(ln.clone());
            } else if cx > gutter_right {
                right_col.push(ln.clone());
            } else {
                spanning_lines.push(ln.clone());
            }
        }

        // If both left and right columns have at least 2 lines, and spanning lines are near the top/bottom
        if left_col.len() >= 2 && right_col.len() >= 2 && spanning_lines.len() <= lines.len() / 2 {
            let col_top = left_col.iter().chain(right_col.iter()).map(|l| l.bbox[1]).min().unwrap_or(0);
            let col_bottom = left_col.iter().chain(right_col.iter()).map(|l| l.bbox[3]).max().unwrap_or(i32::MAX);

            let mut top_headers = Vec::new();
            let mut bottom_footers = Vec::new();

            for sp in spanning_lines {
                if sp.bbox[3] <= col_top + 15 {
                    top_headers.push(sp);
                } else if sp.bbox[1] >= col_bottom - 15 {
                    bottom_footers.push(sp);
                } else {
                    // Placed into closer column
                    let cx = (sp.bbox[0] + sp.bbox[2]) / 2;
                    if cx < min_x + page_w / 2 {
                        left_col.push(sp);
                    } else {
                        right_col.push(sp);
                    }
                }
            }

            let mut result = Vec::with_capacity(lines.len());
            result.extend(sort_single_column_rows(top_headers));
            result.extend(sort_single_column_rows(left_col));
            result.extend(sort_single_column_rows(right_col));
            result.extend(sort_single_column_rows(bottom_footers));
            return result;
        }
    }

    sort_single_column_rows(lines)
}

fn sort_single_column_rows(mut lines: Vec<OCRLine>) -> Vec<OCRLine> {
    if lines.is_empty() {
        return lines;
    }
    let mut heights: Vec<i32> = lines.iter().map(|l| (l.bbox[3] - l.bbox[1]).max(1)).collect();
    heights.sort_unstable();
    let median_h = heights[heights.len() / 2];
    let row_tol = ((median_h as f64) * 0.55).max(6.0);

    lines.sort_by(|a, b| {
        let ca = (a.bbox[1] + a.bbox[3]) as f64 / 2.0;
        let cb = (b.bbox[1] + b.bbox[3]) as f64 / 2.0;
        ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<Vec<OCRLine>> = Vec::new();
    for ln in lines {
        let cy = (ln.bbox[1] + ln.bbox[3]) as f64 / 2.0;
        let same_row = rows.last().map(|r| {
            let avg_y: f64 = r.iter().map(|x| (x.bbox[1] + x.bbox[3]) as f64 / 2.0).sum::<f64>()
                / (r.len() as f64);
            (cy - avg_y).abs() <= row_tol
        }).unwrap_or(false);
        if same_row {
            rows.last_mut().unwrap().push(ln);
        } else {
            rows.push(vec![ln]);
        }
    }

    let mut ordered = Vec::new();
    for mut row in rows {
        row.sort_by_key(|l| l.bbox[0]);
        ordered.extend(row);
    }
    ordered
}

pub fn deduplicate_lines(mut lines: Vec<OCRLine>) -> Vec<OCRLine> {
    if lines.is_empty() {
        return lines;
    }
    lines.sort_by(|a, b| {
        box_area(b.bbox)
            .partial_cmp(&box_area(a.bbox))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut cleaned: Vec<OCRLine> = Vec::new();
    for cand in lines {
        if cleaned.iter().any(|kept| is_duplicate(&cand, kept)) {
            continue;
        }
        cleaned.push(cand);
    }
    sort_reading_order(cleaned)
}

pub fn run_tiled_ocr<F>(
    rgb: &RgbImage,
    mut run_single: F,
    tile_height: Option<u32>,
    overlap_ratio: f64,
) -> Result<(Vec<OCRLine>, usize)>
where
    F: FnMut(&RgbImage) -> Result<Vec<OCRLine>>,
{
    let (width, height) = rgb.dimensions();
    let tiles = compute_tiles(width, height, tile_height, overlap_ratio);
    let mut merged: Vec<OCRLine> = Vec::new();

    for &(y0, y1) in &tiles {
        let strip = imageops::crop_imm(rgb, 0, y0, width, y1 - y0).to_image();
        let strip_lines = run_single(&strip)?;
        for ln in strip_lines {
            let px = [
                ln.bbox[0],
                ln.bbox[1] + y0 as i32,
                ln.bbox[2],
                ln.bbox[3] + y0 as i32,
            ];
            let norm = [
                ((px[0] as f64) / (width as f64) * 1000.0).round() as i32,
                ((px[1] as f64) / (height as f64) * 1000.0).round() as i32,
                ((px[2] as f64) / (width as f64) * 1000.0).round() as i32,
                ((px[3] as f64) / (height as f64) * 1000.0).round() as i32,
            ];
            let candidate = OCRLine {
                text: ln.text,
                confidence: ln.confidence,
                bbox: px,
                bbox_norm: norm,
            };

            let mut matched = false;
            for existing in &mut merged {
                if is_duplicate(&candidate, existing) {
                    matched = true;
                    if prefer_candidate(&candidate, existing) {
                        *existing = candidate.clone();
                    }
                    break;
                }
            }
            if !matched {
                merged.push(candidate);
            }
        }
    }

    Ok((deduplicate_lines(merged), tiles.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_cover_tall_image_with_overlap() {
        let tiles = compute_tiles(800, 2000, Some(500), 0.2);
        assert_eq!(tiles.first().unwrap().0, 0);
        assert_eq!(tiles.last().unwrap().1, 2000);
        assert!(tiles.len() >= 4);
        for w in tiles.windows(2) {
            assert!(w[1].0 < w[0].1);
        }
    }

    #[test]
    fn deduplicate_removes_contained_subbox() {
        let full = OCRLine {
            text: "LINE 34 ORDER REF ORD-1034 AMOUNT 84.00".into(),
            confidence: 0.72,
            bbox: [32, 1610, 654, 1643],
            bbox_norm: [40, 800, 818, 822],
        };
        let frag = OCRLine {
            text: "84.00".into(),
            confidence: 0.35,
            bbox: [576, 1613, 656, 1642],
            bbox_norm: [720, 802, 820, 821],
        };
        let out = deduplicate_lines(vec![frag, full]);
        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("LINE 34"));
    }

    #[test]
    fn reading_order_sorts_rows_then_columns() {
        let b = OCRLine { text: "B".into(), confidence: 0.9, bbox: [300, 50, 400, 80], bbox_norm: [0; 4] };
        let a = OCRLine { text: "A".into(), confidence: 0.9, bbox: [20, 52, 120, 82], bbox_norm: [0; 4] };
        let c = OCRLine { text: "C".into(), confidence: 0.9, bbox: [20, 140, 120, 170], bbox_norm: [0; 4] };
        let sorted = sort_reading_order(vec![c, b, a]);
        let texts: Vec<&str> = sorted.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["A", "B", "C"]);
    }
}
