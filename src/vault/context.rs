#[derive(Debug, Clone)]
pub struct Context {
    pub id: i64,
    pub bookmark_id: String,
    pub application: Option<String>,
    pub captured_at: i64,
}

impl Context {
    pub fn new(
        id: i64,
        bookmark_id: String,
        application: Option<String>,
        captured_at: i64,
    ) -> Self {
        Self {
            id,
            bookmark_id,
            application,
            captured_at,
        }
    }
}