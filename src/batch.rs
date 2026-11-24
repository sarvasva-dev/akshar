use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::config::project_root;
use crate::engine::{OCREngine, SmartOptions};

pub struct BatchQueue {
    db_path: PathBuf,
}

impl BatchQueue {
    pub fn open(custom_path: Option<&Path>) -> Result<Self> {
        let p = custom_path
            .map(PathBuf::from)
            .unwrap_or_else(|| project_root().join("data").join("batch_queue.db"));
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&p)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS batch_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT UNIQUE NOT NULL,
                status TEXT NOT NULL, -- pending, processing, completed, failed
                elapsed_ms REAL,
                result_json TEXT,
                error TEXT,
                created_at TEXT NOT NULL,
                finished_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_batch_status ON batch_queue(status);",
        )?;
        Ok(Self { db_path: p })
    }

    fn conn(&self) -> Result<Connection> {
        Connection::open(&self.db_path).context("failed to open batch database")
    }

    pub fn enqueue_files(&self, files: &[PathBuf]) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        let mut count = 0usize;

        for f in files {
            let path_str = f.to_string_lossy();
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO batch_queue (file_path, status, created_at) VALUES (?1, 'pending', ?2)"
            )?;
            let changed = stmt.execute(params![path_str, now])?;
            if changed > 0 {
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn process_all(
        &self,
        engine: &OCREngine,
        opts: &SmartOptions,
        cancel_token: Arc<AtomicBool>,
        progress_cb: impl Fn(usize, usize, &str),
    ) -> Result<(usize, usize)> {
        let conn = self.conn()?;
        let total: usize = conn.query_row(
            "SELECT COUNT(*) FROM batch_queue",
            [],
            |r| r.get(0),
        )?;
        let mut processed: usize = conn.query_row(
            "SELECT COUNT(*) FROM batch_queue WHERE status = 'completed'",
            [],
            |r| r.get(0),
        )?;

        loop {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }

            let next_item: Option<(i64, String)> = {
                let conn = self.conn()?;
                conn.query_row(
                    "SELECT id, file_path FROM batch_queue WHERE status = 'pending' ORDER BY id ASC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ).ok()
            };

            let Some((id, path_str)) = next_item else {
                break;
            };

            let path = PathBuf::from(&path_str);
            progress_cb(processed, total, &path_str);

            {
                let conn = self.conn()?;
                conn.execute("UPDATE batch_queue SET status = 'processing' WHERE id = ?1", params![id])?;
            }

            let t0 = Instant::now();
            let res = engine.smart_process_file(&path, opts);
            let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let now = Utc::now().to_rfc3339();

            let conn = self.conn()?;
            match res {
                Ok(val) => {
                    let json_str = serde_json::to_string(&val).unwrap_or_default();
                    conn.execute(
                        "UPDATE batch_queue SET status = 'completed', elapsed_ms = ?1, result_json = ?2, finished_at = ?3 WHERE id = ?4",
                        params![elapsed_ms, json_str, now, id],
                    )?;
                    processed += 1;
                }
                Err(err) => {
                    let err_str = err.to_string();
                    conn.execute(
                        "UPDATE batch_queue SET status = 'failed', elapsed_ms = ?1, error = ?2, finished_at = ?3 WHERE id = ?4",
                        params![elapsed_ms, err_str, now, id],
                    )?;
                }
            }
        }

        let failed: usize = {
            let conn = self.conn()?;
            conn.query_row(
                "SELECT COUNT(*) FROM batch_queue WHERE status = 'failed'",
                [],
                |r| r.get(0),
            ).unwrap_or(0)
        };

        Ok((processed, failed))
    }

    pub fn stats(&self) -> Result<serde_json::Value> {
        let conn = self.conn()?;
        let total: usize = conn.query_row("SELECT COUNT(*) FROM batch_queue", [], |r| r.get(0))?;
        let completed: usize = conn.query_row("SELECT COUNT(*) FROM batch_queue WHERE status = 'completed'", [], |r| r.get(0))?;
        let failed: usize = conn.query_row("SELECT COUNT(*) FROM batch_queue WHERE status = 'failed'", [], |r| r.get(0))?;
        let pending: usize = conn.query_row("SELECT COUNT(*) FROM batch_queue WHERE status = 'pending'", [], |r| r.get(0))?;

        Ok(serde_json::json!({
            "total": total,
            "completed": completed,
            "failed": failed,
            "pending": pending,
            "is_finished": pending == 0
        }))
    }
}
