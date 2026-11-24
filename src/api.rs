use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{delete, get, post};
use axum::Router;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};

use crate::automation::{FolderWatcher, WatcherConfig};
use crate::config::{AppConfig, load_config, project_root};
use crate::engine::{OCREngine, SmartOptions, decode_rgb_bytes, preset_forms};
use crate::index::SearchIndex;
use crate::pdf::is_pdf_bytes;
use crate::redact::RedactStyle;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub engine: OCREngine,
    pub watcher: FolderWatcher,
    pub index: Option<SearchIndex>,
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (code, Json(json!({ "detail": msg.into() })))
}

pub fn build_router(engine: OCREngine) -> Router {
    let index = SearchIndex::open_default().ok();
    build_router_with_index(engine, index)
}

pub fn build_router_with_index(engine: OCREngine, index: Option<SearchIndex>) -> Router {
    let config = load_config(None);
    let prefix = config.api_prefix.trim_end_matches('/').to_string();
    let watcher = FolderWatcher::with_index(engine.clone(), index.clone());
    let state = Arc::new(AppState {
        config,
        engine,
        watcher,
        index,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(index_page))
        .route(&format!("{prefix}/config"), get(get_config))
        .route(&format!("{prefix}/presets"), get(get_presets))
        .route(&format!("{prefix}/health"), get(health))
        .route(&format!("{prefix}/auto"), post(auto_endpoint))
        .route(&format!("{prefix}/ocr"), post(ocr_endpoint))
        .route(&format!("{prefix}/extract"), post(extract_endpoint))
        .route(&format!("{prefix}/describe"), post(describe_endpoint))
        .route(&format!("{prefix}/redact"), post(redact_endpoint))
        .route(&format!("{prefix}/batch"), post(batch_endpoint))
        .route(&format!("{prefix}/search"), get(search_endpoint))
        .route(&format!("{prefix}/search/{{id}}"), delete(delete_search_doc))
        .route(&format!("{prefix}/review"), get(review_queue_endpoint))
        .route(&format!("{prefix}/review/{{id}}"), delete(resolve_review_endpoint))
        .route(&format!("{prefix}/automation/status"), get(automation_status))
        .route(&format!("{prefix}/automation/start"), post(automation_start))
        .route(&format!("{prefix}/automation/stop"), post(automation_stop))
        .route(&format!("{prefix}/automation/scan-now"), post(automation_scan_now))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(cors)
        .with_state(state)
}

async fn index_page() -> impl IntoResponse {
    let path = project_root().join("web").join("index.html");
    match fs::read_to_string(&path) {
        Ok(html) => (StatusCode::OK, Html(html)).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Web UI not found").into_response(),
    }
}

async fn get_config(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(serde_json::to_value(&st.config).unwrap_or_else(|_| json!({})))
}

async fn get_presets() -> Json<Value> {
    Json(serde_json::to_value(preset_forms()).unwrap_or_else(|_| json!({})))
}

async fn health(State(st): State<Arc<AppState>>) -> Json<Value> {
    let running = st.watcher.status()["running"].as_bool().unwrap_or(false);
    let indexed_docs = st.index.as_ref().map(|i| i.count()).unwrap_or(0);
    Json(json!({
        "status": "ok",
        "service": st.config.name,
        "version": st.config.version,
        "model_loaded": st.engine.model_filename(),
        "indexed_docs": indexed_docs,
        "automation_running": running
    }))
}

struct ParsedUpload {
    file: Option<(String, Vec<u8>)>,
    files: Vec<(String, Vec<u8>)>,
    fields: std::collections::HashMap<String, String>,
}

async fn parse_multipart(mut mp: Multipart) -> Result<ParsedUpload, (StatusCode, Json<Value>)> {
    let mut parsed = ParsedUpload {
        file: None,
        files: Vec::new(),
        fields: std::collections::HashMap::new(),
    };
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "files" {
            let fname = field.file_name().unwrap_or("upload.bin").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| api_err(StatusCode::BAD_REQUEST, format!("failed reading file: {e}")))?
                .to_vec();
            if name == "file" {
                parsed.file = Some((fname, bytes));
            } else {
                parsed.files.push((fname, bytes));
            }
        } else {
            let val = field
                .text()
                .await
                .map_err(|e| api_err(StatusCode::BAD_REQUEST, format!("invalid form field: {e}")))?;
            parsed.fields.insert(name, val);
        }
    }
    Ok(parsed)
}

fn parse_bool(fields: &std::collections::HashMap<String, String>, key: &str) -> bool {
    fields
        .get(key)
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(false)
}

fn parse_keywords(fields: &std::collections::HashMap<String, String>) -> Vec<String> {
    fields
        .get("keywords")
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_schema(raw: Option<&String>) -> Result<Option<Value>, (StatusCode, Json<Value>)> {
    let Some(s) = raw.map(|x| x.trim()).filter(|x| !x.is_empty()) else {
        return Ok(None);
    };
    let v: Value = serde_json::from_str(s)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, format!("Invalid form_schema JSON: {e}")))?;
    if !v.is_object() || v.as_object().map(|m| m.is_empty()).unwrap_or(true) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "form_schema must be a non-empty JSON object",
        ));
    }
    Ok(Some(v))
}

fn index_result(index: &Option<SearchIndex>, filename: &str, res: &Value) {
    let Some(idx) = index else { return };
    let text = res["text"].as_str().unwrap_or("");
    let route = res["meta"]["auto_route"].as_str().unwrap_or("auto");
    let lines = res["meta"]["line_count"].as_u64().unwrap_or(0) as usize;
    if !text.trim().is_empty() {
        let _ = idx.insert(filename, "", route, lines, text);
    } else if let Some(objs) = res["objects"].as_array().filter(|a| !a.is_empty()) {
        let names = objs
            .iter()
            .filter_map(|o| o["name"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if !names.is_empty() {
            let _ = idx.insert(filename, "", route, 0, &format!("Objects: {names}"));
        }
    }

    if res["needs_review"].as_bool().unwrap_or(false) {
        let reasons: Vec<String> = res["review_reasons"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let _ = idx.add_review_item(filename, &reasons, res);
    }
}

async fn auto_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    let (fname, bytes) = up
        .file
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "Empty upload"))?;

    let opts = SmartOptions {
        mode: up.fields.get("mode").cloned().unwrap_or_else(|| "auto".into()),
        text_conf: up.fields.get("text_conf").and_then(|v| v.parse().ok()).unwrap_or(0.3),
        conf: up.fields.get("conf").and_then(|v| v.parse().ok()).unwrap_or(0.3),
        language: up.fields.get("language").cloned().unwrap_or_else(|| "en".into()),
        keywords: parse_keywords(&up.fields),
        form: parse_schema(up.fields.get("form_schema"))?,
        form_preset: up
            .fields
            .get("form_preset")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        enhance: parse_bool(&up.fields, "enhance"),
        dpi: 180,
        max_pages: 25,
    };

    let engine = st.engine.clone();
    let idx = st.index.clone();
    let res = tokio::task::spawn_blocking(move || {
        let out = engine.smart_process_bytes(&bytes, Some(&fname), &opts)?;
        index_result(&idx, &fname, &out);
        Ok::<Value, anyhow::Error>(out)
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("worker error: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(res))
}

async fn ocr_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    let (fname, bytes) = up
        .file
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "Empty upload"))?;

    let text_conf: f64 = up.fields.get("text_conf").and_then(|v| v.parse().ok()).unwrap_or(0.3);
    let max_lines: usize = up.fields.get("max_lines").and_then(|v| v.parse().ok()).unwrap_or(32);
    let language = up.fields.get("language").cloned().unwrap_or_else(|| "en".into());
    let tiled = parse_bool(&up.fields, "tiled");
    let enhance = parse_bool(&up.fields, "enhance");
    let keywords = parse_keywords(&up.fields);

    let engine = st.engine.clone();
    let idx = st.index.clone();
    let res = tokio::task::spawn_blocking(move || {
        let out = if is_pdf_bytes(&bytes, Some(&fname)) {
            let opts = SmartOptions {
                mode: if tiled { "tiled".into() } else { "ocr".into() },
                text_conf,
                conf: text_conf,
                language,
                keywords,
                enhance,
                ..Default::default()
            };
            engine.smart_process_bytes(&bytes, Some(&fname), &opts)?
        } else {
            let rgb = decode_rgb_bytes(&bytes)?;
            let img = if enhance {
                crate::preprocess::enhance_for_ocr(&rgb)
            } else {
                rgb
            };
            engine.ocr_rgb(&img, text_conf, max_lines, &language, &keywords, tiled, None)?
        };
        index_result(&idx, &fname, &out);
        Ok::<Value, anyhow::Error>(out)
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("worker error: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(res))
}

async fn extract_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    let (fname, bytes) = up
        .file
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "Empty upload"))?;
    let schema = parse_schema(up.fields.get("form_schema"))?
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "form_schema is required"))?;
    let language = up.fields.get("language").cloned().unwrap_or_else(|| "en".into());
    let keywords = parse_keywords(&up.fields);

    let engine = st.engine.clone();
    let res = tokio::task::spawn_blocking(move || {
        if is_pdf_bytes(&bytes, Some(&fname)) {
            let opts = SmartOptions {
                mode: "form".into(),
                language,
                keywords,
                form: Some(schema),
                ..Default::default()
            };
            return engine.smart_process_bytes(&bytes, Some(&fname), &opts);
        }
        let rgb = decode_rgb_bytes(&bytes)?;
        engine.extract_rgb(&rgb, &schema, &language, &keywords)
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("worker error: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(res))
}

async fn describe_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    let (fname, bytes) = up
        .file
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "Empty upload"))?;
    let conf: f64 = up.fields.get("conf").and_then(|v| v.parse().ok()).unwrap_or(0.3);
    let text_conf: f64 = up.fields.get("text_conf").and_then(|v| v.parse().ok()).unwrap_or(0.3);
    let language = up.fields.get("language").cloned().unwrap_or_else(|| "en".into());
    let keywords = parse_keywords(&up.fields);

    let engine = st.engine.clone();
    let idx = st.index.clone();
    let res = tokio::task::spawn_blocking(move || {
        let rgb = decode_rgb_bytes(&bytes)?;
        let out = engine.describe_rgb(&rgb, conf, text_conf, &language, &keywords)?;
        index_result(&idx, &fname, &out);
        Ok::<Value, anyhow::Error>(out)
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("worker error: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(res))
}

async fn redact_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    let (_, bytes) = up
        .file
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "Empty upload"))?;

    let custom_terms: Vec<String> = up
        .fields
        .get("custom_terms")
        .map(|s| {
            s.split([',', '\n'])
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let style = match up.fields.get("style").map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("pixelate") => RedactStyle::Pixelate,
        _ => RedactStyle::SolidBlack,
    };
    let enhance = parse_bool(&up.fields, "enhance");
    let language = up.fields.get("language").cloned().unwrap_or_else(|| "en".into());

    let engine = st.engine.clone();
    let out = tokio::task::spawn_blocking(move || {
        let (png_bytes, summary, elapsed_ms) =
            engine.redact_image_bytes(&bytes, &custom_terms, style, enhance, &language)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        Ok::<Value, anyhow::Error>(json!({
            "redacted_count": summary.redacted_count,
            "matches": summary.matches,
            "redacted_text": summary.redacted_text,
            "redacted_image_base64": b64,
            "elapsed_ms": elapsed_ms
        }))
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("worker error: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(out))
}

async fn batch_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    if up.files.is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "No files uploaded"));
    }
    let opts = SmartOptions {
        mode: up.fields.get("mode").cloned().unwrap_or_else(|| "auto".into()),
        text_conf: up.fields.get("text_conf").and_then(|v| v.parse().ok()).unwrap_or(0.3),
        conf: up.fields.get("text_conf").and_then(|v| v.parse().ok()).unwrap_or(0.3),
        language: up.fields.get("language").cloned().unwrap_or_else(|| "en".into()),
        keywords: Vec::new(),
        form: parse_schema(up.fields.get("form_schema"))?,
        form_preset: up
            .fields
            .get("form_preset")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        enhance: parse_bool(&up.fields, "enhance"),
        dpi: 180,
        max_pages: 25,
    };

    let files = up.files;
    let engine = st.engine.clone();
    let idx = st.index.clone();
    let out = tokio::task::spawn_blocking(move || {
        let t0 = Instant::now();
        let total = files.len();
        let mut items = Vec::with_capacity(total);
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut total_lines = 0usize;

        for (name, bytes) in files {
            if bytes.is_empty() {
                failed += 1;
                items.push(json!({ "filename": name, "status": "failed", "error": "Empty file" }));
                continue;
            }
            match engine.smart_process_bytes(&bytes, Some(&name), &opts) {
                Ok(res) => {
                    index_result(&idx, &name, &res);
                    succeeded += 1;
                    total_lines += res["meta"]["line_count"].as_u64().unwrap_or(0) as usize;
                    items.push(json!({ "filename": name, "status": "completed", "result": res }));
                }
                Err(err) => {
                    failed += 1;
                    items.push(json!({ "filename": name, "status": "failed", "error": err.to_string() }));
                }
            }
        }
        let elapsed_ms = (t0.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0;
        json!({
            "items": items,
            "meta": {
                "total": total,
                "succeeded": succeeded,
                "failed": failed,
                "total_lines": total_lines,
                "elapsed_ms": elapsed_ms
            }
        })
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("worker error: {e}")))?;

    Ok(Json(out))
}

#[derive(Deserialize)]
struct SearchParams {
    #[serde(default, alias = "query")]
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    25
}

async fn search_endpoint(
    State(st): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> ApiResult {
    let Some(idx) = &st.index else {
        return Ok(Json(json!({ "count": 0, "total_indexed": 0, "results": [] })));
    };
    let results = idx
        .search(&params.q, params.limit)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, format!("search error: {e}")))?;
    Ok(Json(json!({
        "count": results.len(),
        "total_indexed": idx.count(),
        "results": results
    })))
}

async fn delete_search_doc(
    State(st): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult {
    let Some(idx) = &st.index else {
        return Err(api_err(StatusCode::SERVICE_UNAVAILABLE, "search index disabled"));
    };
    let deleted = idx
        .delete(id)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "deleted": deleted })))
}

#[derive(Deserialize)]
struct ReviewParams {
    #[serde(default = "default_review_limit")]
    limit: usize,
}
fn default_review_limit() -> usize {
    50
}

async fn review_queue_endpoint(
    State(st): State<Arc<AppState>>,
    Query(params): Query<ReviewParams>,
) -> ApiResult {
    let Some(idx) = &st.index else {
        return Ok(Json(json!({ "count": 0, "items": [] })));
    };
    let items = idx
        .list_review_items(params.limit)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("review error: {e}")))?;
    Ok(Json(json!({
        "count": items.len(),
        "items": items
    })))
}

async fn resolve_review_endpoint(
    State(st): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult {
    let Some(idx) = &st.index else {
        return Err(api_err(StatusCode::SERVICE_UNAVAILABLE, "search index disabled"));
    };
    let resolved = idx
        .resolve_review_item(id)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "resolved": resolved })))
}

async fn automation_status(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(st.watcher.status())
}

async fn automation_start(
    State(st): State<Arc<AppState>>,
    Json(cfg): Json<WatcherConfig>,
) -> Json<Value> {
    Json(st.watcher.start(Some(cfg)))
}

async fn automation_stop(State(st): State<Arc<AppState>>) -> Json<Value> {
    let w = st.watcher.clone();
    let res = tokio::task::spawn_blocking(move || w.stop())
        .await
        .unwrap_or_else(|_| json!({ "running": false }));
    Json(res)
}

async fn automation_scan_now(State(st): State<Arc<AppState>>) -> Json<Value> {
    let w = st.watcher.clone();
    let res = tokio::task::spawn_blocking(move || {
        let jobs = w.scan_once(None);
        json!({
            "scanned_jobs": jobs,
            "status": w.status()
        })
    })
    .await
    .unwrap_or_else(|_| json!({ "scanned_jobs": [] }));
    Json(res)
}
