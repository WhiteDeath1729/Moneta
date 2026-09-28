use axum::{
    body::Bytes,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::TagInfo;
use crate::vault::storage::VaultStorage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionBookmark {
    pub title: String,
    pub url: String,
    pub selected_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromeBookmark {
    pub created_at_in_chrome: i64,
    pub title: String,
    pub url: String,
    pub folder_path: String,
}

impl ChromeBookmark {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.title.trim().is_empty() {
            return Err("Bookmark title cannot be empty");
        }
        if self.url.trim().is_empty() {
            return Err("Bookmark URL cannot be empty");
        }
        if url::Url::parse(&self.url).is_err() {
            return Err("Invalid bookmark URL");
        }
        if self.folder_path.trim().is_empty() {
            return Err("Folder path cannot be empty");
        }
        if self.created_at_in_chrome <= 0 {
            return Err("Creation timestamp must be positive");
        }
        Ok(())
    }
}

async fn health_check() -> &'static str {
    "OK"
}

// ============================================================
// WEB SELECTION BOOKMARK
// ============================================================

async fn receive_selection(
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let selection: SelectionBookmark = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Invalid JSON: {err}")
                })),
            ));
        }
    };

    if selection.title.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Title cannot be empty"
            })),
        ));
    }

    if url::Url::parse(&selection.url).is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Invalid URL"
            })),
        ));
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_nanos();

    let id = format!("web-{timestamp:x}");

    let mut bookmark = Bookmark::new(
        id.clone(),
        "Other bookmarks".to_string(),
        selection.title.clone(),
        "web".to_string(),
    );

    bookmark.source_url = Some(selection.url.clone());
    bookmark.captured_text = Some(selection.selected_text.clone());

    let storage = match VaultStorage::new("moneta-vault/bookmarks") {
        Ok(storage) => storage,
        Err(error) => {
            eprintln!("Failed to open Moneta markdown vault: {error}");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": error.to_string()
                })),
            ));
        }
    };

    match storage.save(&bookmark) {
        Ok(_) => {
            println!("Web selection saved: {}.md", bookmark.id);
            Ok(Json(serde_json::json!({
                "status": "ok",
                "id": bookmark.id,
                "path": format!("moneta-vault/bookmarks/{}.md", bookmark.id)
            })))
        }
        Err(error) => {
            eprintln!("Failed to save web selection: {error}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": error.to_string()
                })),
            ))
        }
    }
}

// ============================================================
// CHROME BOOKMARK IMPORT (Single or Batch)
// ============================================================

async fn receive_bookmarks(
    body: Bytes,
) -> Result<(StatusCode, &'static str), (StatusCode, String)> {
    let json_val: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Malformed JSON: {e}")))?;

    let bookmarks_list: Vec<ChromeBookmark> = match json_val {
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return Err((StatusCode::BAD_REQUEST, "Empty bookmarks array".into()));
            }
            let mut list = Vec::new();
            for (idx, item) in items.into_iter().enumerate() {
                let bm: ChromeBookmark = serde_json::from_value(item).map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Item {idx} missing or invalid fields: {e}"),
                    )
                })?;
                bm.validate()
                    .map_err(|e| (StatusCode::BAD_REQUEST, format!("Item {idx}: {e}")))?;
                list.push(bm);
            }
            list
        }
        serde_json::Value::Object(map) => {
            let bm: ChromeBookmark = serde_json::from_value(serde_json::Value::Object(map))
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Missing or invalid bookmark fields: {e}"),
                    )
                })?;
            bm.validate()
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            vec![bm]
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Expected JSON object or array of bookmarks".into(),
            ));
        }
    };

    let storage = match VaultStorage::new("moneta-vault/bookmarks") {
        Ok(storage) => storage,
        Err(error) => {
            eprintln!("Failed to open Moneta vault: {error}");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to open vault: {error}"),
            ));
        }
    };

    for chrome_bookmark in bookmarks_list {
        let id = create_bookmark_id(&chrome_bookmark);

        let mut bookmark = Bookmark::new(
            id,
            chrome_bookmark.folder_path.clone(),
            chrome_bookmark.title.clone(),
            "web".into(),
        );

        bookmark.source_url = Some(chrome_bookmark.url.clone());

        // Chrome timestamp may be milliseconds or seconds
        let ts = if chrome_bookmark.created_at_in_chrome > 1_000_000_000_000 {
            chrome_bookmark.created_at_in_chrome / 1000
        } else {
            chrome_bookmark.created_at_in_chrome
        };
        bookmark.created_at = ts;
        bookmark.updated_at = ts;

        if let Err(error) = storage.save(&bookmark) {
            eprintln!("Failed to save '{}': {}", bookmark.title, error);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to save bookmark: {error}"),
            ));
        } else {
            println!("Saved: {} -> {}", bookmark.title, chrome_bookmark.url);
        }
    }

    Ok((StatusCode::OK, "OK"))
}

// For chrome bookmark ID generation
pub fn create_bookmark_id(bookmark: &ChromeBookmark) -> String {
    let mut hash: u64 = 5381;
    for byte in bookmark.url.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("web-{hash:x}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSuggestionRequest {
    pub title: Option<String>,
    pub url: Option<String>,
    pub content: Option<String>,
    pub bookmark_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSuggestionResponse {
    pub status: String,
    pub tags: Vec<TagInfo>,
}

async fn suggest_tags(
    Json(payload): Json<TagSuggestionRequest>,
) -> Result<Json<TagSuggestionResponse>, (StatusCode, Json<serde_json::Value>)> {
    let bookmark = if let Some(ref id) = payload.bookmark_id {
        let storage = VaultStorage::new("moneta-vault/bookmarks").map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;
        storage.load(id).map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("Bookmark not found: {e}") })),
            )
        })?
    } else {
        let mut bm = Bookmark::new(
            "temp-tag-query".to_string(),
            "".to_string(),
            payload.title.unwrap_or_default(),
            "web".to_string(),
        );
        bm.source_url = payload.url;
        bm.captured_text = payload.content;
        bm
    };

    let ai = crate::ai::OfflineAIService::new();
    let tags = ai.suggest_tags(&bookmark);

    Ok(Json(TagSuggestionResponse {
        status: "ok".to_string(),
        tags,
    }))
}

pub fn create_router() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/bookmarks", post(receive_bookmarks))
        .route("/selection", post(receive_selection))
        .route("/tags/suggest", post(suggest_tags))
}

// SERVER
pub async fn start_server() {
    let app = create_router();
    let addr = SocketAddr::from(([127, 0, 0, 1], 8765));

    println!("Moneta bookmark API listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind Moneta bookmark API");

    axum::serve(listener, app)
        .await
        .expect("Moneta bookmark API failed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_check() {
        let app = create_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_valid_single_bookmark() {
        let app = create_router();
        let payload = serde_json::json!({
            "title": "2024BCS0301 - Rust Research",
            "url": "https://www.rust-lang.org/",
            "folder_path": "Academic/2024BCS0301",
            "created_at_in_chrome": 1727520000
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/bookmarks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_invalid_bookmark_rejected() {
        let app = create_router();
        let payload = serde_json::json!({
            "title": "",
            "url": "not-a-valid-url",
            "folder_path": "",
            "created_at_in_chrome": -1
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/bookmarks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_missing_fields_rejected() {
        let app = create_router();
        let payload = serde_json::json!({
            "title": "2024BCS0301 - Incomplete Bookmark"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/bookmarks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_batch_bookmarks() {
        let app = create_router();
        let payload = serde_json::json!([
            {
                "title": "Google",
                "url": "https://google.com",
                "folder_path": "Bookmarks Bar",
                "created_at_in_chrome": 1757800000000i64
            },
            {
                "title": "Rust",
                "url": "https://www.rust-lang.org",
                "folder_path": "Bookmarks Bar/Programming",
                "created_at_in_chrome": 1757800001000i64
            }
        ]);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/bookmarks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_suggest_tags_endpoint() {
        let app = create_router();
        let payload = serde_json::json!({
            "title": "Rust Concurrency and Tokio Runtime",
            "url": "https://tokio.rs",
            "content": "Asynchronous event-driven network programming in Rust."
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tags/suggest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: TagSuggestionResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(resp.status, "ok");
        assert!(!resp.tags.is_empty());
    }
}