use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use image::{ImageFormat, Rgb, RgbImage};
use akshar::api::build_router_with_index;
use akshar::automation::{FolderWatcher, WatcherConfig};
use akshar::config::load_config;
use akshar::engine::{OCREngine, SmartOptions};
use akshar::index::SearchIndex;
use akshar::pdf::render_pdf_bytes;
use akshar::redact::RedactStyle;
use akshar::tiling::compute_tiles;
use serde_json::{Value, json};
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tower::ServiceExt;

/// Build a valid multi-page vector PDF using PDF built-in Helvetica-Bold (zero external deps).
fn make_vector_pdf(pages: &[(&[&str], u32, u32)], font_size: u32, line_step: u32) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    // Object 1: Catalog, Object 2: Pages, Object 3: Font
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    let mut kids = Vec::new();
    let base_obj = 4;
    for i in 0..pages.len() {
        kids.push(format!("{} 0 R", base_obj + i * 2));
    }
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".into());

    for (idx, (lines, w, h)) in pages.iter().enumerate() {
        let content_obj_id = base_obj + idx * 2 + 1;
        let mut stream = String::from("BT\n");
        stream.push_str(&format!("/F1 {} Tf\n", font_size));
        let mut y = h.saturating_sub(42);
        for line in *lines {
            let escaped = line.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
            stream.push_str(&format!("1 0 0 1 36 {} Tm ({}) Tj\n", y, escaped));
            y = y.saturating_sub(line_step);
        }
        stream.push_str("ET\n");

        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            w, h, content_obj_id
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            stream
        ));
    }

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = vec![0usize];
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, obj).as_bytes());
    }
    let xref_pos = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref_pos
        )
        .as_bytes(),
    );
    pdf
}

fn png_from_lines(lines: &[&str], w: u32, h: u32, font_size: u32, step: u32) -> Vec<u8> {
    let pdf = make_vector_pdf(&[(lines, w, h)], font_size, step);
    let page = render_pdf_bytes(&pdf, 150, 1).expect("rasterize PDF").pop().unwrap();
    let mut buf = Cursor::new(Vec::new());
    page.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

fn no_text_scene_png() -> Vec<u8> {
    let mut img = RgbImage::from_pixel(480, 360, Rgb([135, 195, 240]));
    for y in 260..360 {
        for x in 0..480 {
            img.put_pixel(x, y, Rgb([70, 150, 80]));
        }
    }
    for y in 40..130 {
        for x in 330..420 {
            let dx = x as i32 - 375;
            let dy = y as i32 - 85;
            if dx * dx + dy * dy <= 42 * 42 {
                img.put_pixel(x, y, Rgb([250, 215, 65]));
            }
        }
    }
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

#[test]
fn test_config_and_tiles() {
    let cfg = load_config(None);
    assert_eq!(cfg.name, "Akshar");
    assert_eq!(cfg.api_prefix, "/v1");

    let tiles = compute_tiles(800, 2000, Some(500), 0.2);
    assert_eq!(tiles.first().unwrap().0, 0);
    assert_eq!(tiles.last().unwrap().1, 2000);
    assert!(tiles.len() >= 4);
    for pair in tiles.windows(2) {
        assert!(pair[1].0 < pair[0].1);
    }
}

#[test]
fn test_ocr_tiled_pdf_and_auto_fallback() {
    let engine = OCREngine::new(None, None, None).expect("load native engine");

    // 1. Standard OCR on vector-rendered invoice
    let inv_png = png_from_lines(
        &[
            "INVOICE NUMBER 48291",
            "DATE 2026 NOVEMBER 24",
            "TOTAL AMOUNT 1250 USD",
        ],
        420,
        240,
        18,
        36,
    );
    let r_inv = engine
        .smart_process_bytes(&inv_png, Some("inv.png"), &SmartOptions::default())
        .expect("smart ocr");
    assert_eq!(r_inv["meta"]["auto_route"], "standard_ocr");
    let txt = r_inv["text"].as_str().unwrap().to_uppercase();
    assert!(txt.contains("48291"), "got: {txt}");
    assert!(txt.contains("1250"), "got: {txt}");

    // 2. Vision fallback on no-text image
    let scene_png = no_text_scene_png();
    let r_scene = engine
        .smart_process_bytes(&scene_png, Some("scene.png"), &SmartOptions::default())
        .expect("vision fallback");
    assert_eq!(r_scene["meta"]["auto_route"], "scene_vision_fallback");
    assert!(r_scene["scene"].is_object());

    // 3. Multi-page PDF OCR
    let pdf = make_vector_pdf(
        &[
            (&["PAGE ONE CONTRACT SUMMARY", "CLIENT ALPHA CORP"], 420, 280),
            (&["PAGE TWO PAYMENT TERMS", "BALANCE DUE 5425 USD"], 420, 280),
        ],
        18,
        38,
    );
    let r_pdf = engine
        .smart_process_bytes(&pdf, Some("contract.pdf"), &SmartOptions::default())
        .expect("pdf ocr");
    assert_eq!(r_pdf["meta"]["is_pdf"], true);
    assert_eq!(r_pdf["meta"]["page_count"], 2);
    let pdf_txt = r_pdf["text"].as_str().unwrap().to_uppercase();
    assert!(pdf_txt.contains("ALPHA"), "got: {pdf_txt}");
    assert!(pdf_txt.contains("5425"), "got: {pdf_txt}");

    // 4. Tall 40-line document -> Full-page tiled OCR bypasses 32-line cap
    let owned_lines: Vec<String> = (1..=40)
        .map(|i| format!("ITEM {:02} CODE AX{:03} PRICE {}", i, i, 10 + i))
        .collect();
    let line_refs: Vec<&str> = owned_lines.iter().map(String::as_str).collect();
    let doc_png = png_from_lines(&line_refs, 420, 1040, 14, 24);
    let doc_rgb = akshar::engine::decode_rgb_bytes(&doc_png).unwrap();
    let r_tiled = engine
        .ocr_rgb(&doc_rgb, 0.25, 32, "en", &[], true, Some(520))
        .expect("tiled ocr");
    let count = r_tiled["meta"]["line_count"].as_u64().unwrap();
    assert!(count > 32, "expected >32 lines from tiled OCR, got {count}");

    // 5. Auto-PII redaction on invoice with email + custom term
    let pii_png = png_from_lines(
        &[
            "CUSTOMER REPORT",
            "EMAIL: alice@example.com",
            "SECRET PROJECT FALCON",
        ],
        460,
        240,
        18,
        40,
    );
    let (redacted_png, summary, _ms) = engine
        .redact_image_bytes(&pii_png, &["Falcon".into()], RedactStyle::SolidBlack, false, "en")
        .expect("redact pii");
    assert!(!redacted_png.is_empty());
    assert!(summary.redacted_count >= 1, "expected PII redactions, got {:?}", summary);
    assert!(
        summary.redacted_text.contains("[REDACTED:"),
        "expected redacted token, got: {}",
        summary.redacted_text
    );

    // 6. Benchmark regression test: p2 scanned invoice with --preset invoice
    //    Must return document_type="invoice", total_amount="12345 rupees", and honest null for missing fields (NEVER "total_name"!)
    let p2_png = png_from_lines(
        &["Total amount due: 12345 rupees."],
        460,
        240,
        18,
        40,
    );
    let mut form_opts = SmartOptions::default();
    form_opts.mode = "form".into();
    form_opts.form_preset = Some("invoice".into());
    let r_form = engine
        .smart_process_bytes(&p2_png, Some("p2.png"), &form_opts)
        .expect("grounded invoice preset");
    let form_fields = &r_form["fields"]["form"];
    assert_eq!(form_fields["document_type"]["value"], "invoice", "got: {form_fields}");
    let tot = form_fields["total_amount"]["value"].as_str().unwrap_or("");
    assert!(tot.contains("12345"), "expected 12345 in total_amount, got: {form_fields}");
    assert!(form_fields["invoice_number"]["value"].is_null(), "missing field must be null, got: {form_fields}");
    assert!(form_fields["date"]["value"].is_null(), "missing date must be null, got: {form_fields}");
    assert!(form_fields["vendor_name"]["value"].is_null(), "missing vendor must be null, got: {form_fields}");

    // 7. Native PDF fast path + 2D table recovery (digital PDF must take <500ms, never OCR'd!)
    let native_pdf = make_vector_pdf(
        &[
            (&["Smoke Test - Fast Native Path in Milliseconds"], 500, 300),
            (&["Item       Qty       Price", "Pen        10        50", "Book       4         200"], 500, 300),
        ],
        14,
        32,
    );
    let r_native = engine
        .smart_process_bytes(&native_pdf, Some("native.pdf"), &SmartOptions::default())
        .expect("native pdf fast path");
    assert_eq!(r_native["meta"]["auto_route"], "pdf_native_fast");
    assert_eq!(r_native["meta"]["native_pages"], 2);
    assert_eq!(r_native["meta"]["ocr_pages"], 0);
    assert!(r_native["meta"]["elapsed_ms"].as_f64().unwrap() < 1000.0);
    assert!(r_native["text"].as_str().unwrap().contains("Fast Native Path"));
    let tbls = r_native["tables"].as_array().unwrap();
    assert!(!tbls.is_empty(), "expected 2D table on page 2, got: {r_native}");
    assert_eq!(tbls[0]["headers"], json!(["Item", "Qty", "Price"]));
}

#[test]
fn test_folder_watcher_and_webhook() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recv_clone = received.clone();

    let hook_thread = thread::spawn(move || {
        listener.set_nonblocking(false).unwrap();
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        data.extend_from_slice(&buf[..n]);
                        let s = String::from_utf8_lossy(&data);
                        if let Some(pos) = s.find("\r\n\r\n") {
                            let header = &s[..pos];
                            let cl: usize = header
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                            if data.len() >= pos + 4 + cl {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            recv_clone
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&data).to_string());
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        }
    });

    let td = TempDir::new().unwrap();
    let inbox = td.path().join("inbox");
    let outbox = td.path().join("outbox");
    let processed = td.path().join("processed");
    let failed = td.path().join("failed");
    std::fs::create_dir_all(&inbox).unwrap();

    let sample_png = png_from_lines(&["ELECTRICITY BILL", "DUE 840 INR"], 420, 220, 18, 36);
    std::fs::write(inbox.join("bill.png"), &sample_png).unwrap();

    let engine = OCREngine::new(None, None, None).unwrap();
    let watcher = FolderWatcher::new(engine);
    let cfg = WatcherConfig {
        inbox_dir: inbox.to_string_lossy().into_owned(),
        outbox_dir: outbox.to_string_lossy().into_owned(),
        processed_dir: processed.to_string_lossy().into_owned(),
        failed_dir: failed.to_string_lossy().into_owned(),
        mode: "auto".into(),
        archive_source: true,
        webhook_url: Some(format!("http://127.0.0.1:{port}/hook")),
        poll_interval: 0.25,
        ..Default::default()
    };
    watcher.start(Some(cfg));

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if watcher.status()["stats"]["processed"].as_u64().unwrap_or(0) >= 1 {
            break;
        }
        thread::sleep(Duration::from_millis(150));
    }
    let st = watcher.stop();
    let _ = hook_thread.join();

    assert_eq!(st["running"], false);
    assert_eq!(st["stats"]["processed"], 1);
    assert!(outbox.join("bill.json").is_file());
    assert!(outbox.join("bill.txt").is_file());
    assert!(outbox.join("bill.md").is_file());
    assert!(outbox.join("summary.csv").is_file());
    assert!(processed.join("bill.png").is_file());
    assert!(!received.lock().unwrap().is_empty());
    assert!(received.lock().unwrap()[0].contains("ocr.completed"));
}

#[tokio::test]
async fn test_axum_http_endpoints() {
    let engine = OCREngine::new(None, None, None).unwrap();
    let idx_dir = TempDir::new().unwrap();
    let idx = SearchIndex::open(&idx_dir.path().join("test_idx.db")).unwrap();
    idx.insert("sample_bill.png", "", "standard_ocr", 3, "ACME INVOICE 9981 TOTAL 500 USD").unwrap();
    let app = build_router_with_index(engine, Some(idx));

    // 1. Light UI
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Akshar"));
    assert!(html.contains("Extract text from any image or PDF"));

    // 2. Health, Config, Presets, Search, Automation status
    for (uri, key) in [
        ("/v1/health", "status"),
        ("/v1/config", "name"),
        ("/v1/presets", "invoice"),
        ("/v1/search?q=9981", "results"),
        ("/v1/automation/status", "running"),
    ] {
        let r = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "failed {uri}");
        let v: Value = serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert!(v.get(key).is_some(), "missing {key} in {uri}: {v}");
        if uri.starts_with("/v1/search") {
            assert_eq!(v["count"], 1);
        }
    }

    // 3. Automation start -> stop JSON endpoint
    let td = TempDir::new().unwrap();
    let payload = json!({
        "inbox_dir": td.path().join("in").to_string_lossy(),
        "outbox_dir": td.path().join("out").to_string_lossy(),
        "processed_dir": td.path().join("proc").to_string_lossy(),
        "failed_dir": td.path().join("fail").to_string_lossy(),
        "mode": "auto"
    });
    let r_start = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/automation/start")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r_start.status(), StatusCode::OK);

    let r_stop = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/automation/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r_stop.status(), StatusCode::OK);
}
