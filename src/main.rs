use anyhow::{Context, Result};
use clap::Parser;
use serde_json::{Value, json};
use std::path::PathBuf;

use akshar::api::build_router;
use akshar::automation::{FolderWatcher, WatcherConfig};
use akshar::config::load_config;
use akshar::engine::{OCREngine, SmartOptions};

#[derive(Parser, Debug)]
#[command(name = "akshar", version, about = "Local OCR, PDF text extraction, and folder watcher")]
struct Cli {
    /// Image, PDF file, or directory for batch processing
    target: Option<PathBuf>,

    /// Processing mode: auto, ocr, tiled, vision, form
    #[arg(long, default_value = "auto")]
    mode: String,

    /// Shortcut for --mode tiled
    #[arg(long)]
    tiled: bool,

    /// Detection confidence threshold (0..1)
    #[arg(long, default_value_t = 0.3)]
    conf: f64,

    /// Language hint (en, de, fr, es, it, nl, pl)
    #[arg(long, default_value = "en")]
    lang: String,

    /// Built-in form extraction preset (invoice, receipt, id_card, business_card)
    #[arg(long)]
    preset: Option<String>,

    /// Custom JSON schema string for structured form extraction
    #[arg(long)]
    form: Option<String>,

    /// Output structured JSON
    #[arg(long = "json")]
    as_json: bool,

    /// Output directory for batch or watch mode
    #[arg(long)]
    out: Option<PathBuf>,

    /// Comma-separated output formats for batch/watch (json,txt,md)
    #[arg(long, default_value = "json,txt,md")]
    formats: String,

    /// Watch an inbox directory continuously for images & PDFs
    #[arg(long)]
    watch: Option<PathBuf>,

    /// Keep processed files in inbox instead of moving to archive
    #[arg(long)]
    no_archive: bool,

    /// Optional webhook URL to POST JSON results on completion
    #[arg(long)]
    webhook: Option<String>,

    /// Start the HTTP API and web interface
    #[arg(long)]
    serve: bool,

    /// Server bind host (with --serve)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Server bind port (with --serve)
    #[arg(long, default_value_t = 8000)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let app_cfg = load_config(None);

    if cli.serve {
        let engine = OCREngine::new(None, None, None)?;
        let app = build_router(engine);
        let addr = format!("{}:{}", cli.host, cli.port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("failed to bind {addr}"))?;
        println!("{} listening on http://{}", app_cfg.name, addr);
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;
        return Ok(());
    }

    let mode = if cli.tiled {
        "tiled".to_string()
    } else if cli.form.is_some() || cli.preset.is_some() {
        "form".to_string()
    } else {
        cli.mode.clone()
    };

    let form_schema: Option<Value> = match &cli.form {
        Some(raw) => Some(serde_json::from_str(raw).context("invalid --form JSON")?),
        None => None,
    };

    let formats: Vec<String> = cli
        .formats
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    if let Some(inbox_path) = cli.watch {
        let inbox = std::fs::canonicalize(&inbox_path).unwrap_or(inbox_path);
        let parent = inbox.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let outbox = cli.out.unwrap_or_else(|| parent.join("outbox"));
        let engine = OCREngine::new(None, None, None)?;
        let watcher = FolderWatcher::new(engine);
        let cfg = WatcherConfig {
            inbox_dir: inbox.to_string_lossy().into_owned(),
            outbox_dir: outbox.to_string_lossy().into_owned(),
            processed_dir: parent.join("processed").to_string_lossy().into_owned(),
            failed_dir: parent.join("failed").to_string_lossy().into_owned(),
            mode: mode.clone(),
            language: cli.lang,
            text_conf: cli.conf,
            formats,
            archive_source: !cli.no_archive,
            form_preset: cli.preset,
            form_schema,
            webhook_url: cli.webhook,
            poll_interval: 1.0,
        };
        watcher.start(Some(cfg));
        println!(
            "{} watching {} -> {} (mode={}). Press Ctrl+C to stop.",
            app_cfg.name,
            inbox.display(),
            outbox.display(),
            mode
        );
        shutdown_signal().await;
        watcher.stop();
        println!("\nStopped.");
        return Ok(());
    }

    let Some(target) = cli.target else {
        use clap::CommandFactory;
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    let engine = OCREngine::new(None, None, None)?;

    if target.is_dir() {
        let outbox = cli.out.unwrap_or_else(|| target.join("ocr_output"));
        let watcher = FolderWatcher::new(engine);
        let cfg = WatcherConfig {
            inbox_dir: target.to_string_lossy().into_owned(),
            outbox_dir: outbox.to_string_lossy().into_owned(),
            mode,
            language: cli.lang,
            text_conf: cli.conf,
            formats,
            archive_source: false,
            form_preset: cli.preset,
            form_schema,
            webhook_url: cli.webhook,
            ..Default::default()
        };
        let jobs = watcher.scan_once(Some(&cfg));
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "processed": jobs.len(),
                "outbox": outbox.to_string_lossy(),
                "jobs": jobs
            }))?
        );
        return Ok(());
    }

    let opts = SmartOptions {
        mode: mode.clone(),
        text_conf: cli.conf,
        conf: cli.conf,
        language: cli.lang,
        keywords: Vec::new(),
        form: form_schema,
        form_preset: cli.preset,
        dpi: 180,
        max_pages: 25,
    };
    let result = engine.smart_process_file(&target, &opts)?;
    if cli.as_json || mode == "form" || mode == "vision" {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("{}", result["text"].as_str().unwrap_or(""));
    }
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
