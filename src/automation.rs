use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, UNIX_EPOCH};

use crate::config::project_root;
use crate::engine::{OCREngine, SmartOptions};

pub const SUPPORTED_EXTENSIONS: [&str; 8] = [
    "png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "pdf",
];

fn utc_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    #[serde(default = "default_inbox")]
    pub inbox_dir: String,
    #[serde(default = "default_outbox")]
    pub outbox_dir: String,
    #[serde(default = "default_processed")]
    pub processed_dir: String,
    #[serde(default = "default_failed")]
    pub failed_dir: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_conf")]
    pub text_conf: f64,
    #[serde(default = "default_formats")]
    pub formats: Vec<String>,
    #[serde(default = "default_true")]
    pub archive_source: bool,
    #[serde(default)]
    pub form_preset: Option<String>,
    #[serde(default)]
    pub form_schema: Option<Value>,
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default = "default_poll")]
    pub poll_interval: f64,
}

fn default_inbox() -> String { project_root().join("automation/inbox").to_string_lossy().into_owned() }
fn default_outbox() -> String { project_root().join("automation/outbox").to_string_lossy().into_owned() }
fn default_processed() -> String { project_root().join("automation/processed").to_string_lossy().into_owned() }
fn default_failed() -> String { project_root().join("automation/failed").to_string_lossy().into_owned() }
fn default_mode() -> String { "auto".into() }
fn default_lang() -> String { "en".into() }
fn default_conf() -> f64 { 0.3 }
fn default_formats() -> Vec<String> { vec!["json".into(), "txt".into(), "md".into()] }
fn default_true() -> bool { true }
fn default_poll() -> f64 { 1.0 }

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            inbox_dir: default_inbox(),
            outbox_dir: default_outbox(),
            processed_dir: default_processed(),
            failed_dir: default_failed(),
            mode: default_mode(),
            language: default_lang(),
            text_conf: default_conf(),
            formats: default_formats(),
            archive_source: true,
            form_preset: None,
            form_schema: None,
            webhook_url: None,
            poll_interval: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub timestamp: String,
    pub filename: String,
    pub status: String,
    pub route: String,
    pub page_count: usize,
    pub line_count: usize,
    pub object_count: usize,
    pub elapsed_ms: f64,
    pub outputs: Vec<String>,
    pub preview: String,
    pub webhook: Option<String>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_to: Option<String>,
}

struct WatcherState {
    config: WatcherConfig,
    running: bool,
    started_at: Option<String>,
    last_scan_at: Option<String>,
    processed: usize,
    failed: usize,
    seen: HashMap<String, (u64, u128)>,
    jobs: Vec<JobRecord>,
}

#[derive(Clone)]
pub struct FolderWatcher {
    engine: OCREngine,
    state: Arc<Mutex<WatcherState>>,
    stop_flag: Arc<AtomicBool>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl FolderWatcher {
    pub fn new(engine: OCREngine) -> Self {
        Self {
            engine,
            state: Arc::new(Mutex::new(WatcherState {
                config: WatcherConfig::default(),
                running: false,
                started_at: None,
                last_scan_at: None,
                processed: 0,
                failed: 0,
                seen: HashMap::new(),
                jobs: Vec::new(),
            })),
            stop_flag: Arc::new(AtomicBool::new(false)),
            handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&self, config: Option<WatcherConfig>) -> Value {
        let cfg = {
            let mut st = self.state.lock().unwrap();
            if let Some(c) = config {
                st.config = c;
            }
            ensure_dirs(&st.config);
            if st.running {
                return build_status(&st);
            }
            st.running = true;
            st.started_at = Some(utc_now());
            st.config.clone()
        };

        self.stop_flag.store(false, Ordering::SeqCst);
        let me = self.clone();
        let poll = cfg.poll_interval.max(0.25);
        let jh = thread::spawn(move || {
            while !me.stop_flag.load(Ordering::SeqCst) {
                let _ = me.scan_once(None);
                let step_ms = 50u64;
                let total_ms = (poll * 1000.0) as u64;
                let mut waited = 0u64;
                while waited < total_ms && !me.stop_flag.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(step_ms.min(total_ms - waited)));
                    waited += step_ms;
                }
            }
        });
        *self.handle.lock().unwrap() = Some(jh);
        self.status()
    }

    pub fn stop(&self) -> Value {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(jh) = self.handle.lock().unwrap().take() {
            let _ = jh.join();
        }
        let mut st = self.state.lock().unwrap();
        st.running = false;
        build_status(&st)
    }

    pub fn status(&self) -> Value {
        let st = self.state.lock().unwrap();
        build_status(&st)
    }

    pub fn scan_once(&self, override_cfg: Option<&WatcherConfig>) -> Vec<JobRecord> {
        let cfg = override_cfg
            .cloned()
            .unwrap_or_else(|| self.state.lock().unwrap().config.clone());
        ensure_dirs(&cfg);

        let inbox = Path::new(&cfg.inbox_dir);
        let mut candidates: Vec<PathBuf> = fs::read_dir(inbox)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && !p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with('.'))
                        .unwrap_or(true)
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                        .unwrap_or(false)
            })
            .collect();
        candidates.sort();

        let mut jobs = Vec::new();
        for path in candidates {
            let meta = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let sig = (meta.len(), mtime);
            if !cfg.archive_source {
                let already = self
                    .state
                    .lock()
                    .unwrap()
                    .seen
                    .get(&path.to_string_lossy().to_string())
                    == Some(&sig);
                if already {
                    continue;
                }
            }
            if !is_file_stable(&path) {
                continue;
            }
            self.state
                .lock()
                .unwrap()
                .seen
                .insert(path.to_string_lossy().to_string(), sig);
            jobs.push(self.process_file(&path, &cfg));
        }

        self.state.lock().unwrap().last_scan_at = Some(utc_now());
        jobs
    }

    pub fn process_file(&self, path: &Path, cfg: &WatcherConfig) -> JobRecord {
        ensure_dirs(cfg);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let stem = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("output")
            .to_string();
        let outbox = Path::new(&cfg.outbox_dir);
        let ts = utc_now();

        let smart_opts = SmartOptions {
            mode: cfg.mode.clone(),
            text_conf: cfg.text_conf,
            conf: cfg.text_conf,
            language: cfg.language.clone(),
            keywords: Vec::new(),
            form: cfg.form_schema.clone(),
            form_preset: cfg.form_preset.clone(),
            dpi: 180,
            max_pages: 25,
        };

        match self.engine.smart_process_file(path, &smart_opts) {
            Ok(result) => {
                let mut outputs = Vec::new();
                let fmts: Vec<String> = cfg.formats.iter().map(|f| f.to_ascii_lowercase()).collect();

                if fmts.iter().any(|f| f == "json") {
                    let dest = unique_dest(outbox, &format!("{stem}.json"));
                    if let Ok(pretty) = serde_json::to_string_pretty(&result) {
                        if fs::write(&dest, pretty).is_ok() {
                            outputs.push(dest.to_string_lossy().into_owned());
                        }
                    }
                }
                if fmts.iter().any(|f| f == "txt") {
                    let dest = unique_dest(outbox, &format!("{stem}.txt"));
                    let body = result
                        .get("text")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            serde_json::to_string_pretty(
                                result
                                    .get("fields")
                                    .or_else(|| result.get("scene"))
                                    .unwrap_or(&result),
                            )
                            .unwrap_or_default()
                        });
                    if fs::write(&dest, body).is_ok() {
                        outputs.push(dest.to_string_lossy().into_owned());
                    }
                }
                if fmts.iter().any(|f| f == "md") {
                    let dest = unique_dest(outbox, &format!("{stem}.md"));
                    if fs::write(&dest, format_markdown(&filename, &result)).is_ok() {
                        outputs.push(dest.to_string_lossy().into_owned());
                    }
                }

                let meta = &result["meta"];
                let text_str = result["text"].as_str().unwrap_or("");
                let mut job = JobRecord {
                    timestamp: ts,
                    filename: filename.clone(),
                    status: "completed".into(),
                    route: meta["auto_route"]
                        .as_str()
                        .unwrap_or(&cfg.mode)
                        .to_string(),
                    page_count: meta["page_count"].as_u64().unwrap_or(1) as usize,
                    line_count: meta["line_count"].as_u64().unwrap_or(0) as usize,
                    object_count: meta["object_count"].as_u64().unwrap_or(0) as usize,
                    elapsed_ms: meta["elapsed_ms"].as_f64().unwrap_or(0.0),
                    outputs,
                    preview: text_str.chars().take(160).collect(),
                    webhook: None,
                    error: None,
                    archived_to: None,
                };

                append_csv_summary(outbox, &job, text_str);

                if let Some(url) = cfg.webhook_url.as_deref().filter(|u| !u.trim().is_empty()) {
                    let payload = json!({
                        "event": "ocr.completed",
                        "file": filename,
                        "job": job,
                        "result": result
                    });
                    job.webhook = Some(dispatch_webhook(url, &payload));
                }

                if cfg.archive_source && path.exists() {
                    let dest = unique_dest(Path::new(&cfg.processed_dir), &filename);
                    if move_file(path, &dest).is_ok() {
                        job.archived_to = Some(dest.to_string_lossy().into_owned());
                    }
                }

                let mut st = self.state.lock().unwrap();
                st.processed += 1;
                st.jobs.insert(0, job.clone());
                st.jobs.truncate(100);
                job
            }
            Err(err) => {
                let mut job = JobRecord {
                    timestamp: ts,
                    filename: filename.clone(),
                    status: "failed".into(),
                    route: cfg.mode.clone(),
                    page_count: 0,
                    line_count: 0,
                    object_count: 0,
                    elapsed_ms: 0.0,
                    outputs: Vec::new(),
                    preview: String::new(),
                    webhook: None,
                    error: Some(err.to_string()),
                    archived_to: None,
                };
                if cfg.archive_source && path.exists() {
                    let dest = unique_dest(Path::new(&cfg.failed_dir), &filename);
                    if move_file(path, &dest).is_ok() {
                        job.archived_to = Some(dest.to_string_lossy().into_owned());
                    }
                }
                let mut st = self.state.lock().unwrap();
                st.failed += 1;
                st.jobs.insert(0, job.clone());
                st.jobs.truncate(100);
                job
            }
        }
    }
}

fn build_status(st: &WatcherState) -> Value {
    json!({
        "running": st.running,
        "config": st.config,
        "stats": {
            "processed": st.processed,
            "failed": st.failed,
            "started_at": st.started_at,
            "last_scan_at": st.last_scan_at
        },
        "jobs": st.jobs.iter().take(50).collect::<Vec<_>>()
    })
}

fn ensure_dirs(cfg: &WatcherConfig) {
    let _ = fs::create_dir_all(&cfg.inbox_dir);
    let _ = fs::create_dir_all(&cfg.outbox_dir);
    if cfg.archive_source {
        let _ = fs::create_dir_all(&cfg.processed_dir);
        let _ = fs::create_dir_all(&cfg.failed_dir);
    }
}

fn is_file_stable(path: &Path) -> bool {
    let s1 = match fs::metadata(path) {
        Ok(m) if m.len() > 0 => m.len(),
        _ => return false,
    };
    thread::sleep(Duration::from_millis(80));
    fs::metadata(path).map(|m| m.len() == s1).unwrap_or(false)
}

fn move_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    if fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    fs::copy(src, dest)?;
    fs::remove_file(src)
}

fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let dest = dir.join(name);
    if !dest.exists() {
        return dest;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    let stamp = Utc::now().format("%Y%m%d_%H%M%S");
    let mut cand = dir.join(format!("{stem}_{stamp}{ext}"));
    let mut n = 1usize;
    while cand.exists() {
        cand = dir.join(format!("{stem}_{stamp}_{n}{ext}"));
        n += 1;
    }
    cand
}

fn format_markdown(filename: &str, result: &Value) -> String {
    let meta = &result["meta"];
    let mut out = vec![
        format!("# {filename}"),
        String::new(),
        format!("- **Route**: `{}`", meta["auto_route"].as_str().unwrap_or("ocr")),
        format!("- **Elapsed**: `{} ms`", meta["elapsed_ms"].as_f64().unwrap_or(0.0)),
        format!("- **Lines**: `{}`", meta["line_count"].as_u64().unwrap_or(0)),
    ];
    if let Some(p) = meta["page_count"].as_u64() {
        out.push(format!("- **Pages**: `{p}`"));
    }
    if let Some(o) = meta["object_count"].as_u64() {
        out.push(format!("- **Objects**: `{o}`"));
    }
    out.push(String::new());
    if let Some(fields) = result.get("fields") {
        out.push("## Extracted Fields".into());
        out.push(String::new());
        out.push("```json".into());
        out.push(serde_json::to_string_pretty(fields).unwrap_or_default());
        out.push("```".into());
        out.push(String::new());
    }
    if let Some(scene) = result.get("scene") {
        out.push("## Scene".into());
        out.push(String::new());
        out.push("```json".into());
        out.push(serde_json::to_string_pretty(scene).unwrap_or_default());
        out.push("```".into());
        out.push(String::new());
    }
    if let Some(text) = result.get("text").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        out.push("## Extracted Text".into());
        out.push(String::new());
        out.push(text.to_string());
        out.push(String::new());
    }
    out.join("\n")
}

fn csv_escape(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn append_csv_summary(outbox: &Path, job: &JobRecord, text_preview: &str) {
    let csv_path = outbox.join("summary.csv");
    let need_header = !csv_path.exists();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&csv_path) {
        if need_header {
            let _ = writeln!(
                f,
                "timestamp,filename,status,route,pages,lines,objects,elapsed_ms,outputs,text_preview"
            );
        }
        let preview: String = text_preview.replace('\n', " ").chars().take(240).collect();
        let _ = writeln!(
            f,
            "{},{},{},{},{},{},{},{},{},{}",
            csv_escape(&job.timestamp),
            csv_escape(&job.filename),
            csv_escape(&job.status),
            csv_escape(&job.route),
            job.page_count,
            job.line_count,
            job.object_count,
            job.elapsed_ms,
            csv_escape(&job.outputs.join("; ")),
            csv_escape(&preview),
        );
    }
}

fn dispatch_webhook(url: &str, payload: &Value) -> String {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    match agent
        .post(url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "Akshar/1.0")
        .send_string(&payload.to_string())
    {
        Ok(resp) => format!("HTTP {}", resp.status()),
        Err(err) => format!("error: {err}"),
    }
}
