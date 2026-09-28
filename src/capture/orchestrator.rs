use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::capture::file::FileCaptureService;
use crate::capture::ocr::OcrEngine;
use crate::vault::bookmark::Bookmark;
use crate::vault::context::ContextService;
use crate::vault::metadata::MetadataService;
use crate::vault::storage::VaultStorage;

pub struct CaptureService {
    storage: VaultStorage,
}

impl CaptureService {
    pub fn new<P: AsRef<Path>>(vault_dir: P) -> io::Result<Self> {
        let storage = VaultStorage::new(vault_dir)?;
        Ok(Self { storage })
    }

    pub fn storage(&self) -> &VaultStorage {
        &self.storage
    }

    /// Captures a URL bookmark, optionally fetching the HTML page title.
    pub fn capture_url(
        &self,
        url: &str,
        custom_title: Option<&str>,
        folder_path: Option<&str>,
    ) -> io::Result<Bookmark> {
        if url::Url::parse(url).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid URL: {url}"),
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
                self.capture_image(path_ref, true)?
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
        // Compute content hash
        let hash_input = format!(
            "{}:{}:{}:{}",
            bookmark.title,
            bookmark.source_url.as_deref().unwrap_or(""),
            bookmark.captured_text.as_deref().unwrap_or(""),
            bookmark.ocr_text.as_deref().unwrap_or("")
        );
        bookmark.content_hash = MetadataService::compute_content_hash(&hash_input);

        // Normalize existing tags
        bookmark.tags = bookmark
            .tags
            .iter()
            .map(|t| MetadataService::normalize_tag(t))
            .filter(|t| !t.is_empty())
            .collect();

        // Enrich with context
        let _ctx = ContextService::capture_context(&bookmark.id);

        // Save to authoritative vault
        self.storage.save(&bookmark)?;

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
                "https://www.rust-lang.org/",
                Some("Rust Official"),
                Some("Programming"),
            )
            .unwrap();

        assert_eq!(bm.title, "Rust Official");
        assert_eq!(bm.source_type, "web");
        assert!(!bm.content_hash.is_empty());
        assert!(service.storage().load(&bm.id).is_ok());

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
