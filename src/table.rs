use serde::{Deserialize, Serialize};

use crate::tiling::OCRLine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordBox {
    pub text: String,
    pub bbox: [i32; 4], // [x0, y0, x1, y1]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCell {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub bbox: [i32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub cells: Vec<TableCell>,
    pub markdown: String,
}

#[derive(Debug, Clone)]
struct Segment {
    text: String,
    bbox: [i32; 4],
}

fn group_segments_into_rows(mut segs: Vec<Segment>) -> Vec<Vec<Segment>> {
    if segs.is_empty() {
        return Vec::new();
    }
    let mut heights: Vec<i32> = segs.iter().map(|s| (s.bbox[3] - s.bbox[1]).max(1)).collect();
    heights.sort_unstable();
    let median_h = heights[heights.len() / 2];
    let tol = ((median_h as f64) * 0.55).max(5.0);

    segs.sort_by(|a, b| {
        let ca = (a.bbox[1] + a.bbox[3]) as f64 / 2.0;
        let cb = (b.bbox[1] + b.bbox[3]) as f64 / 2.0;
        ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<Vec<Segment>> = Vec::new();
    for s in segs {
        let cy = (s.bbox[1] + s.bbox[3]) as f64 / 2.0;
        let same = rows
            .last()
            .map(|r| {
                let avg = r
                    .iter()
                    .map(|x| (x.bbox[1] + x.bbox[3]) as f64 / 2.0)
                    .sum::<f64>()
                    / (r.len() as f64);
                (cy - avg).abs() <= tol
            })
            .unwrap_or(false);
        if same {
            rows.last_mut().unwrap().push(s);
        } else {
            rows.push(vec![s]);
        }
    }
    for row in &mut rows {
        row.sort_by_key(|s| s.bbox[0]);
    }
    rows
}

/// Merge adjacent words within the same row into column segments whenever the gap is small,
/// splitting into distinct table cells whenever the gap exceeds a column gutter threshold.
fn merge_words_to_cells(row: &[WordBox]) -> Vec<Segment> {
    if row.is_empty() {
        return Vec::new();
    }
    let mut char_widths = Vec::new();
    for w in row {
        let len = w.text.chars().count().max(1) as f64;
        let bw = (w.bbox[2] - w.bbox[0]).max(1) as f64;
        char_widths.push(bw / len);
    }
    char_widths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med_cw = char_widths[char_widths.len() / 2];
    let gutter = (med_cw * 2.2).max(14.0);

    let mut cells: Vec<Segment> = Vec::new();
    for w in row {
        if let Some(prev) = cells.last_mut() {
            let gap = (w.bbox[0] - prev.bbox[2]) as f64;
            if gap < gutter {
                prev.text.push(' ');
                prev.text.push_str(&w.text);
                prev.bbox = [
                    prev.bbox[0].min(w.bbox[0]),
                    prev.bbox[1].min(w.bbox[1]),
                    prev.bbox[2].max(w.bbox[2]),
                    prev.bbox[3].max(w.bbox[3]),
                ];
                continue;
            }
        }
        cells.push(Segment {
            text: w.text.clone(),
            bbox: w.bbox,
        });
    }
    cells
}

/// Recover 2D geometric tables from word-level boxes (native PDF fast path).
pub fn extract_tables_from_words(words: &[WordBox]) -> Vec<ExtractedTable> {
    if words.len() < 4 {
        return Vec::new();
    }
    let segs: Vec<Segment> = words
        .iter()
        .map(|w| Segment {
            text: w.text.clone(),
            bbox: w.bbox,
        })
        .collect();
    let word_rows = group_segments_into_rows(segs);
    let cell_rows: Vec<Vec<Segment>> = word_rows
        .iter()
        .map(|r| {
            let wbs: Vec<WordBox> = r
                .iter()
                .map(|s| WordBox {
                    text: s.text.clone(),
                    bbox: s.bbox,
                })
                .collect();
            merge_words_to_cells(&wbs)
        })
        .collect();
    build_tables_from_cell_rows(&cell_rows)
}

/// Recover 2D geometric tables from line-level OCR boxes (scanned images).
pub fn extract_tables_from_lines(lines: &[OCRLine]) -> Vec<ExtractedTable> {
    if lines.len() < 4 {
        return Vec::new();
    }
    let segs: Vec<Segment> = lines
        .iter()
        .map(|l| Segment {
            text: l.text.trim().to_string(),
            bbox: l.bbox,
        })
        .collect();
    let cell_rows = group_segments_into_rows(segs);
    build_tables_from_cell_rows(&cell_rows)
}

fn build_tables_from_cell_rows(cell_rows: &[Vec<Segment>]) -> Vec<ExtractedTable> {
    // Partition into contiguous blocks of rows with >= 2 cells
    let mut blocks: Vec<&[Vec<Segment>]> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, row) in cell_rows.iter().enumerate() {
        if row.len() >= 2 {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            if i - s >= 2 {
                blocks.push(&cell_rows[s..i]);
            }
        }
    }
    if let Some(s) = start {
        if cell_rows.len() - s >= 2 {
            blocks.push(&cell_rows[s..]);
        }
    }

    let mut tables = Vec::new();
    for block in blocks {
        // Cluster column x-intervals across all cells in this block
        let mut intervals: Vec<(i32, i32)> = block
            .iter()
            .flat_map(|r| r.iter().map(|c| (c.bbox[0], c.bbox[2])))
            .collect();
        intervals.sort_by_key(|&(x0, _)| x0);

        let mut col_bounds: Vec<(i32, i32)> = Vec::new();
        for (x0, x1) in intervals {
            if let Some(last) = col_bounds.last_mut() {
                if x0 <= last.1 + 6 {
                    last.1 = last.1.max(x1);
                    continue;
                }
            }
            col_bounds.push((x0, x1));
        }

        let num_cols = col_bounds.len();
        if num_cols < 2 {
            continue;
        }

        let mut grid: Vec<Vec<String>> = Vec::with_capacity(block.len());
        let mut cells: Vec<TableCell> = Vec::new();

        for (r_idx, row) in block.iter().enumerate() {
            let mut row_vals = vec![String::new(); num_cols];
            for seg in row {
                let cx = (seg.bbox[0] + seg.bbox[2]) / 2;
                let col_idx = col_bounds
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, (c0, c1))| {
                        let (c0, c1) = (*c0, *c1);
                        if cx >= c0 && cx <= c1 {
                            0
                        } else {
                            (cx - c0).abs().min((cx - c1).abs())
                        }
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0);

                if !row_vals[col_idx].is_empty() {
                    row_vals[col_idx].push(' ');
                }
                row_vals[col_idx].push_str(&seg.text);
                cells.push(TableCell {
                    row: r_idx,
                    col: col_idx,
                    text: seg.text.clone(),
                    bbox: seg.bbox,
                });
            }
            grid.push(row_vals);
        }

        let headers = grid[0].clone();
        let data_rows = grid[1..].to_vec();
        let mut md = Vec::new();
        let esc = |v: &[String]| {
            format!(
                "| {} |",
                v.iter()
                    .map(|s| s.replace('|', "\\|"))
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        };
        md.push(esc(&headers));
        md.push(format!("| {} |", vec!["---"; num_cols].join(" | ")));
        for r in &data_rows {
            md.push(esc(r));
        }

        tables.push(ExtractedTable {
            headers,
            rows: data_rows,
            cells,
            markdown: md.join("\n"),
        });
    }
    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_3x3_table_from_word_boxes() {
        let words = vec![
            WordBox { text: "Item".into(), bbox: [40, 100, 80, 118] },
            WordBox { text: "Qty".into(), bbox: [200, 100, 235, 118] },
            WordBox { text: "Price".into(), bbox: [320, 100, 365, 118] },
            WordBox { text: "Pen".into(), bbox: [40, 130, 72, 148] },
            WordBox { text: "10".into(), bbox: [200, 130, 222, 148] },
            WordBox { text: "50".into(), bbox: [320, 130, 342, 148] },
            WordBox { text: "Book".into(), bbox: [40, 160, 84, 178] },
            WordBox { text: "4".into(), bbox: [200, 160, 212, 178] },
            WordBox { text: "200".into(), bbox: [320, 160, 354, 178] },
        ];
        let tables = extract_tables_from_words(&words);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers, vec!["Item", "Qty", "Price"]);
        assert_eq!(tables[0].rows[0], vec!["Pen", "10", "50"]);
        assert_eq!(tables[0].rows[1], vec!["Book", "4", "200"]);
    }
}
