#[derive(Debug, Clone)]
pub struct Bookmark {
    pub id: String,
    pub path: String,
    pub title: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub captured_text: Option<String>,
    pub ocr_text: Option<String>,
    pub content_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Bookmark {
    pub fn new(id: String, path: String, title: String, source_type: String,
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
            content_hash: String::new(),
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    pub fn update_timestamp(&mut self) {
        self.updated_at = current_timestamp();
    }
}

fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs() as i64
}