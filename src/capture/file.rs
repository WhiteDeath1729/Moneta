use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::capture::epub;
use crate::vault::bookmark::Bookmark;

pub struct FileCaptureService;

impl FileCaptureService {
    /// Captures a local text or markdown file as a Moneta bookmark.
    pub fn capture_text_file<P: AsRef<Path>>(path: P) -> io::Result<Bookmark> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("File not found: {}", path.display()),
            ));
        }

        let content = fs::read_to_string(path)?;
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled Document")
            .to_string();

        let timestamp = current_timestamp();
        let id = format!("file-{timestamp:x}");

        let mut bookmark = Bookmark::new(
            id,
            path.to_string_lossy().to_string(),
            title,
            "file".into(),
        );

        bookmark.source_url = Some(path.to_string_lossy().to_string());
        bookmark.captured_text = Some(content);
        bookmark.created_at = timestamp;
        bookmark.updated_at = timestamp;

        Ok(bookmark)
    }

    /// Captures a local image file as a Moneta bookmark.
    pub fn capture_image_file<P: AsRef<Path>>(
        path: P,
        ocr_text: Option<String>,
    ) -> io::Result<Bookmark> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Image file not found: {}", path.display()),
            ));
        }

        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Image Bookmark")
            .to_string();

        let timestamp = current_timestamp();
        let id = format!("image-{timestamp:x}");

        let mut bookmark = Bookmark::new(
            id,
            path.to_string_lossy().to_string(),
            title,
            "image".into(),
        );

        bookmark.source_url = Some(path.to_string_lossy().to_string());
        bookmark.ocr_text = ocr_text;
        bookmark.created_at = timestamp;
        bookmark.updated_at = timestamp;

        Ok(bookmark)
    }

    /// Captures an EPUB ebook and compiles chapter summaries into a bookmark record.
    pub fn capture_epub_file<P: AsRef<Path>>(path: P) -> io::Result<Bookmark> {
        let path = path.as_ref();
        let chapters = epub::extract_chapters(path)?;

        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("EPUB Book")
            .to_string();

        let mut combined_text = String::new();
        for chapter in &chapters {
            combined_text.push_str(&format!("\n--- Chapter: {} ---\n", chapter.name));
            // Basic tag stripping
            let text_only: String = chapter
                .html
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace() || c.is_ascii_punctuation())
                .take(1000)
                .collect();
            combined_text.push_str(&text_only);
        }

        let timestamp = current_timestamp();
        let id = format!("epub-{timestamp:x}");

        let mut bookmark = Bookmark::new(
            id,
            path.to_string_lossy().to_string(),
            title,
            "document".into(),
        );

        bookmark.source_url = Some(path.to_string_lossy().to_string());
        bookmark.captured_text = Some(combined_text);
        bookmark.created_at = timestamp;
        bookmark.updated_at = timestamp;

        Ok(bookmark)
    }

    /// Captures the current text from system clipboard as a bookmark.
    pub fn capture_clipboard() -> io::Result<Bookmark> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| io::Error::other(e.to_string()))?;

        let text = clipboard
            .get_text()
            .map_err(|e| io::Error::other(e.to_string()))?;

        if text.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Clipboard is empty or contains no readable text",
            ));
        }

        let title = text
            .lines()
            .next()
            .unwrap_or("Clipboard Capture")
            .chars()
            .take(40)
            .collect::<String>();

        let timestamp = current_timestamp();
        let id = format!("clip-{timestamp:x}");

        let mut bookmark = Bookmark::new(
            id,
            "Clipboard".into(),
            if title.trim().is_empty() { "Clipboard Capture".into() } else { title },
            "clipboard".into(),
        );

        bookmark.captured_text = Some(text);
        bookmark.created_at = timestamp;
        bookmark.updated_at = timestamp;

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
    fn test_capture_text_file() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("moneta_test_sample.txt");
        fs::write(&test_file, "This is a test document for Moneta file capture.").unwrap();

        let bookmark = FileCaptureService::capture_text_file(&test_file).unwrap();
        assert_eq!(bookmark.source_type, "file");
        assert_eq!(bookmark.title, "moneta_test_sample");
        assert!(bookmark.captured_text.unwrap().contains("Moneta file capture"));

        let _ = fs::remove_file(test_file);
    }

    #[test]
    fn test_capture_epub_file() {
        let bookmark = FileCaptureService::capture_epub_file("test.epub").unwrap();
        assert_eq!(bookmark.source_type, "document");
        assert!(bookmark.captured_text.is_some());
    }

    #[test]
    fn test_capture_image_file() {
        let bookmark = FileCaptureService::capture_image_file(
            "test.jpg",
            Some("Extracted OCR text".into()),
        )
        .unwrap();
        assert_eq!(bookmark.source_type, "image");
        assert_eq!(bookmark.ocr_text.as_deref(), Some("Extracted OCR text"));
    }
}
