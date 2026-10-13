use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ai::{LocalAIEngine, OfflineAIService};
use crate::capture::file::FileCaptureService;
use crate::capture::ocr::OcrEngine;
use crate::search::indexing::SqliteIndex;
use crate::vault::bookmark::Bookmark;
use crate::vault::context::ContextService;
use crate::vault::metadata::MetadataService;
use crate::vault::storage::VaultStorage;

pub struct CaptureService {
    storage: VaultStorage,
    ai: OfflineAIService,
}

impl CaptureService {
    pub fn new<P: AsRef<Path>>(vault_dir: P) -> io::Result<Self> {
        Self::new_with_ai(vault_dir, Arc::new(LocalAIEngine::default()))
    }

    pub fn new_with_ai<P: AsRef<Path>>(
        vault_dir: P,
        engine: Arc<LocalAIEngine>,
    ) -> io::Result<Self> {
        let storage = VaultStorage::new(vault_dir)?;
        let ai = OfflineAIService::with_engine(engine);
        Ok(Self { storage, ai })
    }

    pub fn storage(&self) -> &VaultStorage {
        &self.storage
    }

    /// Captures the webpage text before enriching a URL bookmark.
    pub fn capture_url(
        &self,
        url: &str,
        custom_title: Option<&str>,
        folder_path: Option<&str>,
    ) -> io::Result<Bookmark> {
        if !url::Url::parse(url)
            .map(|url| matches!(url.scheme(), "http" | "https"))
            .unwrap_or(false)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid HTTP or HTTPS URL: {url}"),
            ));
        }

        let title = match custom_title {
            Some(t) if !t.trim().is_empty() => t.to_string(),
            _ => url.to_string(),
        };

        let timestamp = current_timestamp();
        let mut hash: u64 = 5381;
        for byte in url.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        let id = format!("web-{hash:x}");

        let mut bookmark = Bookmark::new(
            id,
            folder_path.unwrap_or("Web Bookmarks").to_string(),
            title,
            "web".into(),
        );

        bookmark.source_url = Some(url.to_string());
        match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .and_then(|client| client.get(url).send())
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::text)
        {
            Ok(html) => {
                let page_text = crate::capture::web::extract_webpage_text(&html);
                if !page_text.is_empty() {
                    bookmark.captured_text = Some(page_text);
                }
            }
            Err(error) => {
                eprintln!("Could not fetch webpage text for {url}: {error}");
            }
        }
        bookmark.created_at = timestamp;
        bookmark.updated_at = timestamp;

        self.save_and_enrich(bookmark)
    }

    /// Captures selected text or note.
    pub fn capture_text(
        &self,
        title: &str,
        text: &str,
        source_url: Option<&str>,
    ) -> io::Result<Bookmark> {
        if title.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Title cannot be empty",
            ));
        }

        let timestamp = current_timestamp();
        let id = format!("text-{timestamp:x}");

        let mut bookmark = Bookmark::new(
            id,
            "Notes".into(),
            title.to_string(),
            "text".into(),
        );

        bookmark.captured_text = Some(text.to_string());
        bookmark.source_url = source_url.map(|s| s.to_string());
        bookmark.created_at = timestamp;
        bookmark.updated_at = timestamp;

        self.save_and_enrich(bookmark)
    }

    /// Captures a local file (text, markdown, code, or epub document).
    pub fn capture_file<P: AsRef<Path>>(&self, path: P) -> io::Result<Bookmark> {
        let path_ref = path.as_ref();
        let extension = path_ref
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let bookmark = match extension.as_str() {
            "epub" => FileCaptureService::capture_epub_file(path_ref)?,
            "png" | "jpg" | "jpeg" | "bmp" | "webp" => {
                return self.capture_image(path_ref, true);
            }
            _ => FileCaptureService::capture_text_file(path_ref)?,
        };

        self.save_and_enrich(bookmark)
    }

    /// Captures an image file with optional local OCR extraction.
    pub fn capture_image<P: AsRef<Path>>(
        &self,
        path: P,
        run_ocr_flag: bool,
    ) -> io::Result<Bookmark> {
        let path_ref = path.as_ref();
        let mut ocr_text = None;

        if run_ocr_flag
            && let Ok(mut engine) = OcrEngine::new() {
                match engine.extract_text(path_ref) {
                    Ok(text) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            ocr_text = Some(trimmed.to_string());
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: OCR extraction failed: {e}");
                    }
                }
            }

        let bookmark = FileCaptureService::capture_image_file(path_ref, ocr_text)?;
        self.save_and_enrich(bookmark)
    }

    /// Captures content currently in the system clipboard.
    pub fn capture_clipboard(&self) -> io::Result<Bookmark> {
        let bookmark = FileCaptureService::capture_clipboard()?;
        self.save_and_enrich(bookmark)
    }

    /// Enriches a bookmark with context, computes content hash, and persists to vault.
    pub fn save_and_enrich(&self, mut bookmark: Bookmark) -> io::Result<Bookmark> {
        let tags = self.ai.suggest_tags(&bookmark);
        bookmark.tags = tags
            .into_iter()
            .map(|tag| MetadataService::normalize_tag(&tag.name))
            .filter(|tag| !tag.is_empty())
            .collect();
        bookmark.tags.sort();
        bookmark.tags.dedup();

        // Compute content hash
        let hash_input = format!(
            "{}:{}:{}:{}",
            bookmark.title,
            bookmark.source_url.as_deref().unwrap_or(""),
            bookmark.captured_text.as_deref().unwrap_or(""),
            bookmark.ocr_text.as_deref().unwrap_or("")
        );
        bookmark.content_hash = MetadataService::compute_content_hash(&hash_input);

        // Enrich with context
        let _ctx = ContextService::capture_context(&bookmark.id);

        let embedding = self.ai.generate_embedding(&bookmark.text_for_embedding());
        if embedding.is_empty() {
            return Err(io::Error::other(format!(
                "Could not generate an embedding for bookmark '{}'",
                bookmark.title
            )));
        }

        // Save the bookmark and both derived search representations.
        self.storage.save(&bookmark)?;
        self.storage.save_embedding(&bookmark.id, &embedding)?;

        let index_directory = if self
            .storage
            .root()
            .file_name()
            .is_some_and(|name| name == "bookmarks")
        {
            self.storage
                .root()
                .parent()
                .unwrap_or(self.storage.root())
        } else {
            self.storage.root()
        };
        let index_path = index_directory.join("moneta_index.db");
        let index = SqliteIndex::open(index_path).map_err(io::Error::other)?;
        index
            .index_bookmark_with_embedding(&bookmark, &self.ai, &embedding)
            .map_err(io::Error::other)?;

        Ok(bookmark)
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_capture_url() {
        let temp_dir = std::env::temp_dir().join("moneta_orch_test_url");
        let service = CaptureService::new(&temp_dir).unwrap();

        let bm = service
            .capture_url(
                "http://127.0.0.1:1/",
                Some("Rust Official"),
                Some("Programming"),
            )
            .unwrap();

        assert_eq!(bm.title, "Rust Official");
        assert_eq!(bm.source_type, "web");
        assert!(!bm.content_hash.is_empty());
        assert!(service.storage().load(&bm.id).is_ok());
        assert!(!bm.tags.is_empty());
        assert!(!service.storage().load_embedding(&bm.id).unwrap().is_empty());

        let index = SqliteIndex::open(temp_dir.join("moneta_index.db")).unwrap();
        let embeddings = index.get_all_embeddings().unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].0, bm.id);
        assert!(!embeddings[0].1.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_image_enrichment_persists_embedding_from_ocr_text_only() {
        let temp_dir = std::env::temp_dir().join("moneta_image_enrichment_test");
        let engine = Arc::new(LocalAIEngine::default());
        let service = CaptureService::new_with_ai(&temp_dir, engine.clone()).unwrap();
        let mut bookmark = Bookmark::new(
            "image-ocr-test".into(),
            "screenshot.png".into(),
            "Screenshot".into(),
            "image".into(),
        );
        bookmark.captured_text = Some("This text must not affect the vector.".into());
        bookmark.ocr_text = Some("Recognized OCR content about astronomy.".into());

        let saved = service.save_and_enrich(bookmark).unwrap();
        let expected = engine.generate_embedding("Recognized OCR content about astronomy.");
        let binary_embedding = service.storage().load_embedding(&saved.id).unwrap();
        let index = SqliteIndex::open(temp_dir.join("moneta_index.db")).unwrap();
        let indexed_embedding = index.get_all_embeddings().unwrap();

        assert_eq!(binary_embedding, expected);
        assert_eq!(indexed_embedding[0].1, expected);
        assert!(!saved.tags.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_orchestrator_capture_text() {
        let temp_dir = std::env::temp_dir().join("moneta_orch_test_text");
        let service = CaptureService::new(&temp_dir).unwrap();

        let bm = service
            .capture_text(
                "Rust Concurrency",
                "Rust provides fearless concurrency with Send and Sync traits.",
                Some("https://doc.rust-lang.org/book/"),
            )
            .unwrap();

        assert_eq!(bm.title, "Rust Concurrency");
        assert_eq!(bm.source_type, "text");
        assert!(bm.captured_text.unwrap().contains("fearless concurrency"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_orchestrator_capture_image_with_ocr() {
        let temp_dir = std::env::temp_dir().join("moneta_orch_test_img");
        let service = CaptureService::new(&temp_dir).unwrap();

        let bm = service.capture_image("test.jpg", true).unwrap();
        assert_eq!(bm.source_type, "image");
        assert!(bm.ocr_text.is_some());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
