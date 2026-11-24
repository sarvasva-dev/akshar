use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::project_root;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDoc {
    pub id: i64,
    pub filename: String,
    pub source_path: String,
    pub route: String,
    pub line_count: usize,
    pub created_at: String,
    pub snippet: String,
    pub text: String,
}

#[derive(Clone)]
pub struct SearchIndex {
    db_path: PathBuf,
    conn: Arc<Mutex<Connection>>,
}

impl SearchIndex {
    pub fn open_default() -> Result<Self> {
        let path = project_root().join("data").join("akshar.db");
        Self::open(&path)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS documents (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 filename TEXT NOT NULL,
                 source_path TEXT NOT NULL DEFAULT '',
                 route TEXT NOT NULL DEFAULT 'auto',
                 line_count INTEGER NOT NULL DEFAULT 0,
                 text TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
                 filename,
                 text,
                 content='documents',
                 content_rowid='id'
             );
             CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents BEGIN
                 INSERT INTO documents_fts(rowid, filename, text) VALUES (new.id, new.filename, new.text);
             END;
             CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents BEGIN
                 INSERT INTO documents_fts(documents_fts, rowid, filename, text) VALUES ('delete', old.id, old.filename, old.text);
             END;",
        )?;
        Ok(Self {
            db_path: path.to_path_buf(),
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.db_path
    }

    pub fn insert(
        &self,
        filename: &str,
        source_path: &str,
        route: &str,
        line_count: usize,
        text: &str,
    ) -> Result<i64> {
        if text.trim().is_empty() {
            return Ok(0);
        }
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO documents (filename, source_path, route, line_count, text, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![filename, source_path, route, line_count as i64, text, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<IndexedDoc>> {
        let q = query.trim();
        let conn = self.conn.lock().unwrap();
        let lim = limit.clamp(1, 100) as i64;

        if q.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT id, filename, source_path, route, line_count, created_at, substr(text, 1, 180), text
                 FROM documents ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![lim], |r| {
                    Ok(IndexedDoc {
                        id: r.get(0)?,
                        filename: r.get(1)?,
                        source_path: r.get(2)?,
                        route: r.get(3)?,
                        line_count: r.get::<_, i64>(4)? as usize,
                        created_at: r.get(5)?,
                        snippet: r.get(6)?,
                        text: r.get(7)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            return Ok(rows);
        }

        // Convert plain user words into prefix FTS5 terms (e.g. `inv 482` -> `"inv"* "482"*`)
        let fts_query = q
            .split_whitespace()
            .map(|tok| format!("\"{}\"*", tok.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");

        let mut stmt = conn.prepare(
            "SELECT d.id, d.filename, d.source_path, d.route, d.line_count, d.created_at,
                    snippet(documents_fts, 1, '[', ']', '...', 18), d.text
             FROM documents_fts
             JOIN documents d ON d.id = documents_fts.rowid
             WHERE documents_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let rows = stmt
            .query_map(params![fts_query, lim], |r| {
                Ok(IndexedDoc {
                    id: r.get(0)?,
                    filename: r.get(1)?,
                    source_path: r.get(2)?,
                    route: r.get(3)?,
                    line_count: r.get::<_, i64>(4)? as usize,
                    created_at: r.get(5)?,
                    snippet: r.get(6)?,
                    text: r.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fts5_indexes_and_searches_documents() {
        let td = TempDir::new().unwrap();
        let idx = SearchIndex::open(&td.path().join("test.db")).unwrap();
        idx.insert("invoice_april.png", "", "standard_ocr", 5, "ACME TECH INVOICE 48291 TOTAL 1250 USD").unwrap();
        idx.insert("notes.pdf", "", "pdf_auto", 12, "Quantum mechanics lecture notes").unwrap();

        let hits = idx.search("48291", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].filename, "invoice_april.png");

        let all = idx.search("", 10).unwrap();
        assert_eq!(all.len(), 2);
        assert!(idx.delete(hits[0].id).unwrap());
        assert_eq!(idx.count(), 1);
    }
}
