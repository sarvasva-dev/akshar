pub mod api;
pub mod automation;
pub mod config;
pub mod engine;
pub mod pdf;
pub mod tiling;

pub use automation::{FolderWatcher, WatcherConfig};
pub use config::{AppConfig, load_config};
pub use engine::{OCREngine, SmartOptions, preset_forms};
