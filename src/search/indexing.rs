use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::ai::embeddings::{deserialize_vector, serialize_vector};
use crate::ai::OfflineAIService;
use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::MetadataService;
use crate::vault::storage::VaultStorage;

pub struct SqliteIndex {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteIndex {
    /// Opens or creates the SQLite index at the specified file path.
    pub fn open<P: AsRef<Path>>(path: P) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let index = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        index.init_schema()?;
        Ok(index)
    }

    /// Opens an in-memory SQLite database, useful for fast and isolated unit tests.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let index = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        index.init_schema()?;
        Ok(index)
    }

    /// Initializes tables matching SADD Section 6.4 Data Dictionary.
    pub fn init_schema(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS bookmarks (
                id TEXT PRIMARY KEY,
                path TEXT,
                title TEXT NOT NULL,
                source_url TEXT,
                source_type TEXT NOT NULL,
                captured_text TEXT,
                ocr_text TEXT,
                content_hash TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id TEXT NOT NULL,
                target_id TEXT,
                FOREIGN KEY (source_id) REFERENCES bookmarks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                source TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS bookmark_tags (
                bookmark_id TEXT NOT NULL,
                tag_id INTEGER NOT NULL,
                confidence REAL,
                source TEXT NOT NULL,
                PRIMARY KEY (bookmark_id, tag_id),
                FOREIGN KEY (bookmark_id) REFERENCES bookmarks(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS contexts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bookmark_id TEXT NOT NULL,
                application TEXT,
                captured_at INTEGER NOT NULL,
                FOREIGN KEY (bookmark_id) REFERENCES bookmarks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS embeddings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bookmark_id TEXT NOT NULL,
                model TEXT NOT NULL,
                vector_ref TEXT NOT NULL,
                FOREIGN KEY (bookmark_id) REFERENCES bookmarks(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_bookmarks_title ON bookmarks(title);
            CREATE INDEX IF NOT EXISTS idx_bookmarks_source_type ON bookmarks(source_type);
            CREATE INDEX IF NOT EXISTS idx_tags_name ON tags(name);
            CREATE INDEX IF NOT EXISTS idx_links_target ON links(target_id);
            "#,
        )?;

        // Try to initialize SQLite FTS5 table; fall back gracefully if FTS5 extension is not compiled
        let _ = conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS bookmarks_fts USING fts5(
                id UNINDEXED,
                title,
                captured_text,
                ocr_text,
                tags
            );
            "#,
        );

        Ok(())
    }

    /// Indexes a bookmark and updates all relational metadata, tags, wiki links, and embeddings.
    pub fn index_bookmark(
        &self,
        bookmark: &Bookmark,
        ai: Option<&OfflineAIService>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();

        // 1. Insert or replace into bookmarks
        conn.execute(
            r#"
            INSERT OR REPLACE INTO bookmarks
            (id, path, title, source_url, source_type, captured_text, ocr_text, content_hash, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                bookmark.id,
                bookmark.path,
                bookmark.title,
                bookmark.source_url,
                bookmark.source_type,
                bookmark.captured_text,
                bookmark.ocr_text,
                bookmark.content_hash,
                bookmark.created_at,
                bookmark.updated_at,
            ],
        )?;

        // 2. Clear old tags & links for this bookmark
        conn.execute(
            "DELETE FROM bookmark_tags WHERE bookmark_id = ?1",
            params![bookmark.id],
        )?;
        conn.execute(
            "DELETE FROM links WHERE source_id = ?1",
            params![bookmark.id],
        )?;

        // 3. Insert tags
        for tag in &bookmark.tags {
            let normalized = MetadataService::normalize_tag(tag);
            if normalized.is_empty() {
                continue;
            }

            conn.execute(
                "INSERT OR IGNORE INTO tags (name, source) VALUES (?1, 'USER')",
                params![normalized],
            )?;

            let tag_id: i64 = conn.query_row(
                "SELECT id FROM tags WHERE name = ?1",
                params![normalized],
                |row| row.get(0),
            )?;

            conn.execute(
                r#"
                INSERT OR REPLACE INTO bookmark_tags (bookmark_id, tag_id, confidence, source)
                VALUES (?1, ?2, 1.0, 'USER')
                "#,
                params![bookmark.id, tag_id],
            )?;
        }

        // 4. Extract and insert wiki links [[...]]
        let full_content = format!(
            "{} {}",
            bookmark.captured_text.as_deref().unwrap_or(""),
            bookmark.ocr_text.as_deref().unwrap_or("")
        );
        let wiki_links = MetadataService::extract_wiki_links(&full_content);
        for target in wiki_links {
            conn.execute(
                "INSERT INTO links (source_id, target_id) VALUES (?1, ?2)",
                params![bookmark.id, target],
            )?;
        }

        // 5. Generate and store embeddings if AI service provided
        if let Some(ai_service) = ai {
            let text_to_embed = format!(
                "{} {} {}",
                bookmark.title,
                bookmark.captured_text.as_deref().unwrap_or(""),
                bookmark.ocr_text.as_deref().unwrap_or("")
            );
            let embedding = ai_service.generate_embedding(&text_to_embed);
            let serialized = serialize_vector(&embedding);

            conn.execute(
                "DELETE FROM embeddings WHERE bookmark_id = ?1",
                params![bookmark.id],
            )?;

            conn.execute(
                r#"
                INSERT INTO embeddings (bookmark_id, model, vector_ref)
                VALUES (?1, ?2, ?3)
                "#,
                params![bookmark.id, ai_service.embedding_model_name(), serialized],
            )?;
        }

        // 6. Update FTS index if table exists
        let tag_str = bookmark.tags.join(" ");
        let _ = conn.execute(
            "DELETE FROM bookmarks_fts WHERE id = ?1",
            params![bookmark.id],
        );
        let _ = conn.execute(
            r#"
            INSERT INTO bookmarks_fts (id, title, captured_text, ocr_text, tags)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                bookmark.id,
                bookmark.title,
                bookmark.captured_text.as_deref().unwrap_or(""),
                bookmark.ocr_text.as_deref().unwrap_or(""),
                tag_str
            ],
        );

        Ok(())
    }

    /// Deletes a bookmark and its associated entries from the derived index.
    pub fn delete_bookmark(&self, id: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
        conn.execute("DELETE FROM bookmark_tags WHERE bookmark_id = ?1", params![id])?;
        conn.execute("DELETE FROM links WHERE source_id = ?1", params![id])?;
        conn.execute("DELETE FROM embeddings WHERE bookmark_id = ?1", params![id])?;
        conn.execute("DELETE FROM contexts WHERE bookmark_id = ?1", params![id])?;
        let _ = conn.execute("DELETE FROM bookmarks_fts WHERE id = ?1", params![id]);
        Ok(())
    }

    /// Rebuilds the complete SQLite index by scanning all Markdown files in the vault.
    pub fn rebuild_from_vault(
        &self,
        storage: &VaultStorage,
        ai: Option<&OfflineAIService>,
    ) -> rusqlite::Result<usize> {
        let bookmarks = storage.list_all().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?;

        // Re-initialize tables cleanly
        self.init_schema()?;

        let mut count = 0;
        for bm in &bookmarks {
            self.index_bookmark(bm, ai)?;
            count += 1;
        }

        Ok(count)
    }

    /// Executes full-text and keyword search across title, captured_text, ocr_text, and tags.
    pub fn query_keyword(&self, query: &str) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let query_clean = query.trim();

        // 1. Try FTS search if available
        let fts_pattern = query_clean
            .split_whitespace()
            .map(|w| format!("\"{w}\"*"))
            .collect::<Vec<String>>()
            .join(" ");

        if let Ok(mut stmt) = conn.prepare("SELECT id FROM bookmarks_fts WHERE bookmarks_fts MATCH ?1")
            && let Ok(rows) = stmt.query_map(params![fts_pattern], |r| r.get::<_, String>(0)) {
                let ids: Vec<String> = rows.filter_map(Result::ok).collect();
                if !ids.is_empty() {
                    return Ok(ids);
                }
            }

        // 2. Standard LIKE search fallback
        let like_pattern = format!("%{query_clean}%");
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT b.id
            FROM bookmarks b
            LEFT JOIN bookmark_tags bt ON b.id = bt.bookmark_id
            LEFT JOIN tags t ON bt.tag_id = t.id
            WHERE b.title LIKE ?1
               OR b.captured_text LIKE ?1
               OR b.ocr_text LIKE ?1
               OR b.source_url LIKE ?1
               OR t.name LIKE ?1
            ORDER BY b.updated_at DESC
            "#,
        )?;

        let rows = stmt.query_map(params![like_pattern], |row| row.get(0))?;
        let mut ids = Vec::new();
        for r in rows {
            ids.push(r?);
        }
        Ok(ids)
    }

    /// Retrieves all embeddings stored in the database.
    pub fn get_all_embeddings(&self) -> rusqlite::Result<Vec<(String, Vec<f32>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT bookmark_id, vector_ref FROM embeddings")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let v_str: String = row.get(1)?;
            let vec: Vec<f32> = deserialize_vector(&v_str).unwrap_or_default();
            Ok((id, vec))
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Resolves backlinks to a given target bookmark id or title.
    pub fn get_backlinks(&self, target_id_or_title: &str) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT source_id FROM links WHERE target_id = ?1 OR target_id = ?2",
        )?;
        let rows = stmt.query_map(params![target_id_or_title, target_id_or_title], |r| r.get(0))?;
        let mut backlinks = Vec::new();
        for r in rows {
            backlinks.push(r?);
        }
        Ok(backlinks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_indexing_and_fts() {
        let index = SqliteIndex::open_in_memory().unwrap();
        let ai = OfflineAIService::new();

        let mut bm = Bookmark::new(
            "bm-001".into(),
            "vault/bm-001.md".into(),
            "Rust Concurrency Patterns".into(),
            "web".into(),
        );
        bm.captured_text = Some("Fearless concurrency with threads and message passing in Rust.".into());
        bm.tags = vec!["rust".into(), "concurrency".into()];

        index.index_bookmark(&bm, Some(&ai)).unwrap();

        let results = index.query_keyword("concurrency").unwrap();
        assert_eq!(results, vec!["bm-001"]);

        let results_by_tag = index.query_keyword("rust").unwrap();
        assert_eq!(results_by_tag, vec!["bm-001"]);

        let embeddings = index.get_all_embeddings().unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].0, "bm-001");
        assert_eq!(embeddings[0].1.len(), 128);
    }

    #[test]
    fn test_index_rebuild_from_vault() {
        let temp_dir = std::env::temp_dir().join("moneta_rebuild_test");
        let storage = VaultStorage::new(&temp_dir).unwrap();

        let bm1 = Bookmark::new("rb-1".into(), "".into(), "First Document".into(), "file".into());
        let bm2 = Bookmark::new("rb-2".into(), "".into(), "Second Document".into(), "file".into());
        storage.save(&bm1).unwrap();
        storage.save(&bm2).unwrap();

        let index = SqliteIndex::open_in_memory().unwrap();
        let count = index.rebuild_from_vault(&storage, None).unwrap();
        assert_eq!(count, 2);

        let results = index.query_keyword("First").unwrap();
        assert_eq!(results, vec!["rb-1"]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_backlinks_in_index() {
        let index = SqliteIndex::open_in_memory().unwrap();

        let mut bm = Bookmark::new(
            "bm-source".into(),
            "".into(),
            "Source Note".into(),
            "text".into(),
        );
        bm.captured_text = Some("Connecting to [[Target Note]] for research.".into());

        index.index_bookmark(&bm, None).unwrap();

        let backlinks = index.get_backlinks("Target Note").unwrap();
        assert_eq!(backlinks, vec!["bm-source"]);
    }
}
