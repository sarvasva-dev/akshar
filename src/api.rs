use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde_json::{Value, json};
use std::fs;
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};

use crate::automation::{FolderWatcher, WatcherConfig};
use crate::config::{AppConfig, load_config, project_root};
use crate::engine::{OCREngine, SmartOptions, decode_rgb_bytes, preset_forms};
use crate::pdf::is_pdf_bytes;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub engine: OCREngine,
    pub watcher: FolderWatcher,
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (code, Json(json!({ "detail": msg.into() })))
}

pub fn build_router(engine: OCREngine) -> Router {
    let config = load_config(None);
    let prefix = config.api_prefix.trim_end_matches('/').to_string();
    let watcher = FolderWatcher::new(engine.clone());
    let state = Arc::new(AppState {
        config,
        engine,
        watcher,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(index))
        .route(&format!("{prefix}/config"), get(get_config))
        .route(&format!("{prefix}/presets"), get(get_presets))
        .route(&format!("{prefix}/health"), get(health))
        .route(&format!("{prefix}/auto"), post(auto_endpoint))
        .route(&format!("{prefix}/ocr"), post(ocr_endpoint))
        .route(&format!("{prefix}/extract"), post(extract_endpoint))
        .route(&format!("{prefix}/describe"), post(describe_endpoint))
        .route(&format!("{prefix}/batch"), post(batch_endpoint))
        .route(&format!("{prefix}/automation/status"), get(automation_status))
        .route(&format!("{prefix}/automation/start"), post(automation_start))
        .route(&format!("{prefix}/automation/stop"), post(automation_stop))
        .route(&format!("{prefix}/automation/scan-now"), post(automation_scan_now))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(cors)
        .with_state(state)
}

async fn index() -> impl IntoResponse {
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
    Json(json!({
        "status": "ok",
        "service": st.config.name,
        "version": st.config.version,
        "model_loaded": st.engine.model_filename(),
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

async fn auto_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    let (fname, bytes) = up
        .file
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "Empty upload"))?;

    let opts = SmartOptions {
        mode: up.fields.get("mode").cloned().unwrap_or_else(|| "auto".into()),
        text_conf: up
            .fields
            .get("text_conf")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.3),
        conf: up
            .fields
            .get("conf")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.3),
        language: up.fields.get("language").cloned().unwrap_or_else(|| "en".into()),
        keywords: parse_keywords(&up.fields),
        form: parse_schema(up.fields.get("form_schema"))?,
        form_preset: up
            .fields
            .get("form_preset")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        dpi: 180,
        max_pages: 25,
    };

    let engine = st.engine.clone();
    let res = tokio::task::spawn_blocking(move || engine.smart_process_bytes(&bytes, Some(&fname), &opts))
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

    let text_conf: f64 = up
        .fields
        .get("text_conf")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.3);
    let max_lines: usize = up
        .fields
        .get("max_lines")
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);
    let language = up.fields.get("language").cloned().unwrap_or_else(|| "en".into());
    let tiled = up
        .fields
        .get("tiled")
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(false);
    let keywords = parse_keywords(&up.fields);

    let engine = st.engine.clone();
    let res = tokio::task::spawn_blocking(move || {
        if is_pdf_bytes(&bytes, Some(&fname)) {
            let opts = SmartOptions {
                mode: if tiled { "tiled".into() } else { "ocr".into() },
                text_conf,
                conf: text_conf,
                language,
                keywords,
                ..Default::default()
            };
            return engine.smart_process_bytes(&bytes, Some(&fname), &opts);
        }
        let rgb = decode_rgb_bytes(&bytes)?;
        engine.ocr_rgb(&rgb, text_conf, max_lines, &language, &keywords, tiled, None)
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
    let (_, bytes) = up
        .file
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "Empty upload"))?;
    let conf: f64 = up.fields.get("conf").and_then(|v| v.parse().ok()).unwrap_or(0.3);
    let text_conf: f64 = up
        .fields
        .get("text_conf")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.3);
    let language = up.fields.get("language").cloned().unwrap_or_else(|| "en".into());
    let keywords = parse_keywords(&up.fields);

    let engine = st.engine.clone();
    let res = tokio::task::spawn_blocking(move || {
        let rgb = decode_rgb_bytes(&bytes)?;
        engine.describe_rgb(&rgb, conf, text_conf, &language, &keywords)
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("worker error: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(res))
}

async fn batch_endpoint(State(st): State<Arc<AppState>>, mp: Multipart) -> ApiResult {
    let up = parse_multipart(mp).await?;
    if up.files.is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "No files uploaded"));
    }
    let opts = SmartOptions {
        mode: up.fields.get("mode").cloned().unwrap_or_else(|| "auto".into()),
        text_conf: up
            .fields
            .get("text_conf")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.3),
        conf: up
            .fields
            .get("text_conf")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.3),
        language: up.fields.get("language").cloned().unwrap_or_else(|| "en".into()),
        keywords: Vec::new(),
        form: parse_schema(up.fields.get("form_schema"))?,
        form_preset: up
            .fields
            .get("form_preset")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        dpi: 180,
        max_pages: 25,
    };

    let files = up.files;
    let engine = st.engine.clone();
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
