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

    /// Extracts readable text from a DOCX file.
    pub fn extract_docx_text<P: AsRef<Path>>(path: P) -> io::Result<String> {
        use std::io::Read;
        let file = fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file).map_err(io::Error::other)?;
        let mut doc_xml = archive.by_name("word/document.xml").map_err(io::Error::other)?;
        let mut content = String::new();
        doc_xml.read_to_string(&mut content)?;

        let mut extracted = String::new();
        let mut cursor = 0;
        while let Some(start_tag) = content[cursor..].find("<w:t") {
            let abs_start = cursor + start_tag;
            if let Some(close_bracket) = content[abs_start..].find('>') {
                let text_start = abs_start + close_bracket + 1;
                if let Some(end_tag) = content[text_start..].find("</w:t>") {
                    extracted.push_str(&content[text_start..text_start + end_tag]);
                    extracted.push(' ');
                    cursor = text_start + end_tag + 6;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(extracted)
    }

    /// Extracts readable text from a PDF file.
    pub fn extract_pdf_text<P: AsRef<Path>>(path: P) -> io::Result<String> {
        let bytes = fs::read(path)?;
        let mut text = String::new();
        let mut in_text_obj = false;
        let mut i = 0;

        while i < bytes.len() {
            if !in_text_obj {
                if i + 2 <= bytes.len() && &bytes[i..i + 2] == b"BT" {
                    in_text_obj = true;
                    i += 2;
                    continue;
                }
            } else {
                if i + 2 <= bytes.len() && &bytes[i..i + 2] == b"ET" {
                    in_text_obj = false;
                    i += 2;
                    text.push('\n');
                    continue;
                }
                if bytes[i] == b'(' {
                    i += 1;
                    let start = i;
                    while i < bytes.len() && bytes[i] != b')' {
                        if bytes[i] == b'\\' && i + 1 < bytes.len() {
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    if let Ok(segment) = std::str::from_utf8(&bytes[start..i]) {
                        text.push_str(segment);
                        text.push(' ');
                    }
                }
            }
            i += 1;
        }

        if text.trim().is_empty() {
            let mut current_run = String::new();
            for &b in &bytes {
                if b.is_ascii_graphic() || b == b' ' {
                    current_run.push(b as char);
                } else {
                    if current_run.len() >= 6 && !current_run.starts_with('/') {
                        text.push_str(&current_run);
                        text.push(' ');
                    }
                    current_run.clear();
                }
            }
        }

        Ok(text)
    }

    /// Extracts the complete available textual content of a source file based on its extension.
    pub fn extract_full_source_text<P: AsRef<Path>>(path: P) -> Option<String> {
        let path = path.as_ref();
        if !path.exists() || !path.is_file() {
            return None;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "txt" | "md" | "markdown" | "rs" | "py" | "c" | "cpp" | "h" | "js" | "ts" | "json"
            | "toml" | "yaml" | "yml" | "html" | "css" | "csv" | "log" => {
                fs::read_to_string(path).ok()
            }
            "epub" => {
                if let Ok(chapters) = epub::extract_chapters(path) {
                    let mut full = String::new();
                    for ch in chapters {
                        full.push_str(&format!("\n# {}\n", ch.name));
                        let plain: String = ch
                            .html
                            .chars()
                            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || c.is_ascii_punctuation())
                            .collect();
                        full.push_str(&plain);
                        full.push('\n');
                    }
                    Some(full)
                } else {
                    None
                }
            }
            "docx" => Self::extract_docx_text(path).ok(),
            "pdf" => Self::extract_pdf_text(path).ok(),
            _ => None,
        }
    }

    /// Locates the surrounding context window around the selected text inside the complete document.
    pub fn find_surrounding_context(full_doc: &str, selected_text: &str, radius_chars: usize) -> Option<String> {
        if selected_text.trim().is_empty() {
            return None;
        }

        let needle = selected_text.trim();
        let idx = full_doc.find(needle).or_else(|| {
            let first_30: String = needle.chars().take(30).collect();
            full_doc.find(&first_30)
        })?;

        let start = idx.saturating_sub(radius_chars);
        let end = (idx + needle.len() + radius_chars).min(full_doc.len());

        Some(full_doc[start..end].to_string())
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
