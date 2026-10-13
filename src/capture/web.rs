use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use scraper::{ElementRef, Html, Selector, node::Node};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ai::LocalAIEngine;
use crate::capture::orchestrator::CaptureService;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BookmarkSyncResponse {
    status: String,
    synced: usize,
    page_content_failures: Vec<String>,
}

async fn health_check() -> &'static str {
    "OK"
}

// ============================================================
// WEB SELECTION BOOKMARK
// ============================================================

async fn receive_selection(
    State(engine): State<Arc<LocalAIEngine>>,
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

    if !is_http_url(&selection.url) {
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
    let client = match webpage_client() {
        Ok(client) => client,
        Err(error) => {
            return Err(internal_error(error));
        }
    };
    let (page_text, page_content_error) = match fetch_webpage_text(&client, &selection.url).await {
        Ok(text) => (text, None),
        Err(error) => {
            eprintln!("Could not fetch selected webpage {}: {error}", selection.url);
            (String::new(), Some(error))
        }
    };
    bookmark.captured_text = Some(format!(
        "Selected text:\n{}\n\nFull webpage:\n{}",
        selection.selected_text, page_text
    ));

    let capture_service = match CaptureService::new_with_ai(
        "moneta-vault/bookmarks",
        engine,
    ) {
        Ok(service) => service,
        Err(error) => {
            return Err(internal_error(error));
        }
    };

    match capture_service.save_and_enrich(bookmark) {
        Ok(bookmark) => {
            println!("Web selection saved: {}.md", bookmark.id);
            Ok(Json(serde_json::json!({
                "status": "ok",
                "id": bookmark.id,
                "path": format!("moneta-vault/bookmarks/{}.md", bookmark.id),
                "page_content_error": page_content_error
            })))
        }
        Err(error) => {
            eprintln!("Failed to save web selection: {error}");
            Err(internal_error(error))
        }
    }
}

// ============================================================
// CHROME BOOKMARK IMPORT (Single or Batch)
// ============================================================

async fn receive_bookmarks(
    State(engine): State<Arc<LocalAIEngine>>,
    body: Bytes,
) -> Result<Json<BookmarkSyncResponse>, (StatusCode, String)> {
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

    let client = webpage_client().map_err(internal_error_text)?;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
    let mut fetch_tasks = tokio::task::JoinSet::new();
    for (index, chrome_bookmark) in bookmarks_list.iter().enumerate() {
        let client = client.clone();
        let semaphore = semaphore.clone();
        let url = chrome_bookmark.url.clone();
        fetch_tasks.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .map_err(|error| format!("Could not schedule webpage fetch: {error}"))?;
            Ok::<_, String>((index, fetch_webpage_text(&client, &url).await))
        });
    }

    let mut page_texts: Vec<Option<Result<String, String>>> =
        std::iter::repeat_with(|| None).take(bookmarks_list.len()).collect();
    while let Some(task_result) = fetch_tasks.join_next().await {
        let (index, page_text) = task_result
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(internal_error_text)?;
        page_texts[index] = Some(page_text);
    }

    let capture_service = CaptureService::new_with_ai("moneta-vault/bookmarks", engine)
        .map_err(internal_error_text)?;
    let mut page_content_failures = Vec::new();
    let synced = bookmarks_list.len();

    for (chrome_bookmark, page_text) in bookmarks_list.into_iter().zip(page_texts) {
        let id = create_bookmark_id(&chrome_bookmark);

        let mut bookmark = Bookmark::new(
            id,
            chrome_bookmark.folder_path.clone(),
            chrome_bookmark.title.clone(),
            "web".into(),
        );

        bookmark.source_url = Some(chrome_bookmark.url.clone());
        match page_text.expect("each bookmark webpage fetch task must complete") {
            Ok(text) => bookmark.captured_text = Some(text),
            Err(error) => {
                let failure = format!("{} ({}): {error}", chrome_bookmark.title, chrome_bookmark.url);
                eprintln!("Webpage text unavailable for bookmark: {failure}");
                page_content_failures.push(failure);
            }
        }

        // Chrome timestamp may be milliseconds or seconds
        let ts = if chrome_bookmark.created_at_in_chrome > 1_000_000_000_000 {
            chrome_bookmark.created_at_in_chrome / 1000
        } else {
            chrome_bookmark.created_at_in_chrome
        };
        bookmark.created_at = ts;
        bookmark.updated_at = ts;

        let title = bookmark.title.clone();
        if let Err(error) = capture_service.save_and_enrich(bookmark) {
            eprintln!("Failed to save '{}': {}", title, error);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to save bookmark: {error}"),
            ));
        } else {
            println!("Saved: {} -> {}", title, chrome_bookmark.url);
        }
    }

    Ok(Json(BookmarkSyncResponse {
        status: "ok".into(),
        synced,
        page_content_failures,
    }))
}

fn is_http_url(value: &str) -> bool {
    url::Url::parse(value)
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

fn webpage_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("Could not initialize webpage fetcher: {error}"))
}

async fn fetch_webpage_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    if !is_http_url(url) {
        return Err("Bookmark URL is not an HTTP or HTTPS webpage".into());
    }

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    let html = response
        .text()
        .await
        .map_err(|error| format!("Could not read webpage response: {error}"))?;
    Ok(extract_webpage_text(&html))
}

pub(crate) fn extract_webpage_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let body_selector = Selector::parse("body").expect("static body selector is valid");
    let Some(body) = document.select(&body_selector).next() else {
        return String::new();
    };

    let mut text_parts = Vec::new();
    collect_page_text(body, &mut text_parts);
    text_parts.join(" ")
}

fn collect_page_text(element: ElementRef<'_>, text_parts: &mut Vec<String>) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                let text = text.trim();
                if !text.is_empty() {
                    text_parts.push(text.to_string());
                }
            }
            Node::Element(node)
                if matches!(
                    node.name(),
                    "script" | "style" | "noscript" | "template" | "svg"
                ) => {}
            Node::Element(_) => {
                if let Some(child_element) = ElementRef::wrap(child) {
                    collect_page_text(child_element, text_parts);
                }
            }
            _ => {}
        }
    }
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "status": "error",
            "message": error.to_string()
        })),
    )
}

fn internal_error_text(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
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
    create_router_with_engine(Arc::new(LocalAIEngine::default()))
}

fn create_router_with_engine(engine: Arc<LocalAIEngine>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route(
            "/bookmarks",
            post(receive_bookmarks).layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route("/selection", post(receive_selection))
        .route("/tags/suggest", post(suggest_tags))
        .with_state(engine)
}

// SERVER
pub async fn start_server(engine: Arc<LocalAIEngine>) {
    let app = create_router_with_engine(engine);
    let addr = SocketAddr::from(([127, 0, 0, 1], 8765));

    println!("Moneta bookmark API listening on {addr}");

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Warning: Could not bind Moneta bookmark API on {addr}: {e}. Continuing without local bookmark server.");
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Moneta bookmark API stopped: {e}");
    }
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

    #[test]
    fn test_extract_webpage_text_uses_page_body_and_skips_nonvisible_content() {
        let text = extract_webpage_text(
            "<html><head><title>Head title</title></head><body>\
             <h1>Page heading</h1><p>Full article text.</p>\
             <script>ignored_script()</script><style>.hidden{display:none}</style>\
             </body></html>",
        );

        assert!(text.contains("Page heading"));
        assert!(text.contains("Full article text."));
        assert!(!text.contains("ignored_script"));
        assert!(!text.contains("display:none"));
    }

    #[tokio::test]
    async fn test_valid_single_bookmark() {
        let app = create_router();
        let payload = serde_json::json!({
            "title": "2024BCS0301 - Rust Research",
            "url": "http://127.0.0.1:1/",
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
                "url": "http://127.0.0.1:1/google",
                "folder_path": "Bookmarks Bar",
                "created_at_in_chrome": 1757800000000i64
            },
            {
                "title": "Rust",
                "url": "http://127.0.0.1:1/rust",
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
    async fn test_bookmark_import_accepts_payloads_over_default_body_limit() {
        let app = create_router();
        let body = vec![b' '; 3 * 1024 * 1024];

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/bookmarks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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