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
}