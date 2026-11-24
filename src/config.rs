use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_subtitle")]
    pub subtitle: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_prefix")]
    pub api_prefix: String,
}

fn default_name() -> String {
    "Akshar".into()
}
fn default_subtitle() -> String {
    "Extract text from images and PDFs".into()
}
fn default_version() -> String {
    "1.0.0".into()
}
fn default_accent() -> String {
    "#2563eb".into()
}
fn default_prefix() -> String {
    "/v1".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: default_name(),
            subtitle: default_subtitle(),
            version: default_version(),
            accent: default_accent(),
            api_prefix: default_prefix(),
        }
    }
}

pub fn project_root() -> PathBuf {
    if let Ok(cwd) = env::current_dir()
        && (cwd.join("assets").is_dir() || cwd.join("config.json").is_file())
    {
        return cwd;
    }
    if let Ok(exe) = env::current_exe() {
        for anc in exe.ancestors().skip(1) {
            if anc.join("assets").is_dir() || anc.join("config.json").is_file() {
                return anc.to_path_buf();
            }
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn load_config(override_path: Option<&Path>) -> AppConfig {
    let path = override_path
        .map(PathBuf::from)
        .or_else(|| env::var("AKSHAR_CONFIG").ok().map(PathBuf::from))
        .unwrap_or_else(|| project_root().join("config.json"));

    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
        .unwrap_or_default()
}

pub fn default_library_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libcactus.dylib"
    } else if cfg!(target_os = "windows") {
        "libcactus.dll"
    } else {
        "libcactus.so"
    }
}

pub fn resolve_engine_path(override_path: Option<&Path>) -> Result<PathBuf> {
    let path = override_path
        .map(PathBuf::from)
        .or_else(|| env::var("AKSHAR_LIB_PATH").ok().map(PathBuf::from))
        .unwrap_or_else(|| {
            project_root()
                .join("assets")
                .join("engine")
                .join(default_library_name())
        });
    if !path.is_file() {
        bail!(
            "Inference library not found at {}. Run `sh scripts/setup.sh` first.",
            path.display()
        );
    }
    Ok(path)
}

pub fn resolve_model_path(override_path: Option<&Path>) -> Result<PathBuf> {
    let path = override_path
        .map(PathBuf::from)
        .or_else(|| env::var("AKSHAR_MODEL_PATH").ok().map(PathBuf::from))
        .unwrap_or_else(|| {
            project_root()
                .join("assets")
                .join("models")
                .join("ocr_v1.cact")
        });
    if !path.is_file() {
        bail!(
            "Model weights not found at {}. Run `sh scripts/setup.sh` first.",
            path.display()
        );
    }
    Ok(path)
}
