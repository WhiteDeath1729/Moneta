use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub path: String,
    pub title: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub captured_text: Option<String>,
    pub ocr_text: Option<String>,
    pub tags: Vec<String>,
    pub content_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl Bookmark {
    pub fn new(
        id: String,
        path: String,
        title: String,
        source_type: String,
    ) -> Self {
        let timestamp = current_timestamp();

        Self {
            id,
            path,
            title,
            source_type,
            source_url: None,
            captured_text: None,
            ocr_text: None,
            tags: Vec::new(),
            content_hash: String::new(),
            created_at: timestamp,
            updated_at: timestamp,
            summary: None,
        }
    }

    pub fn update_timestamp(&mut self) {
        self.updated_at = current_timestamp();
    }

    pub fn text_for_embedding(&self) -> String {
        if self.source_type.eq_ignore_ascii_case("image") {
            return self.ocr_text.clone().unwrap_or_default();
        }

        if self.source_type.eq_ignore_ascii_case("web") {
            return format!(
                "{}\n{}",
                self.title,
                self.captured_text.as_deref().unwrap_or("")
            );
        }

        format!(
            "{}\n{}\n{}",
            self.title,
            self.captured_text.as_deref().unwrap_or(""),
            self.ocr_text.as_deref().unwrap_or("")
        )
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
    fn test_bookmark_backward_compatibility() {
        let yaml = r#"
id: '1790672986583839600'
path: C:\WINDOWS\system32\ApplicationFrameHost.exe
title: ApplicationFrameHost
source_type: exe
source_url: null
captured_text: Josiah Claremont, widowed many years before...
ocr_text: null
tags: []
content_hash: ''
created_at: 1790672986
updated_at: 1790672986
"#;
        let bm: Bookmark = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(bm.id, "1790672986583839600");
        assert_eq!(bm.summary, None);
    }

    #[test]
    fn embedding_text_uses_ocr_for_images_and_page_text_for_web_bookmarks() {
        let mut image = Bookmark::new(
            "image-1".into(),
            "screenshot.png".into(),
            "Screenshot".into(),
            "image".into(),
        );
        image.captured_text = Some("must not be embedded".into());
        image.ocr_text = Some("recognized words".into());
        assert_eq!(image.text_for_embedding(), "recognized words");

        let mut web = Bookmark::new(
            "web-1".into(),
            "Bookmarks Bar".into(),
            "Example site".into(),
            "web".into(),
        );
        web.captured_text = Some("full webpage text".into());
        assert!(web.text_for_embedding().contains("full webpage text"));
    }
}