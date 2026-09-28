use axum::{
    routing::{get, post},
    Json, Router,
};

use std::time::{SystemTime, UNIX_EPOCH};
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::vault::bookmark::Bookmark;
use crate::vault::storage::VaultStorage;


#[derive(Debug, Deserialize)]
pub struct SelectionBookmark {
    pub title: String,
    pub url: String,
    pub selected_text: String,
}


#[derive(Debug, Serialize, Deserialize)]
pub struct ChromeBookmark {
    pub created_at_in_chrome: i64,
    pub title: String,
    pub url: String,
    pub folder_path: String,
}


async fn health_check() -> &'static str {
    "OK"
}


// ============================================================
// WEB SELECTION BOOKMARK
// ============================================================

async fn receive_selection(
    Json(selection): Json<SelectionBookmark>,
) -> Json<serde_json::Value> {

    println!("Received selection bookmark:");
    println!("Title: {}", selection.title);
    println!("URL: {}", selection.url);
    println!("Selected text: {}", selection.selected_text);


    // --------------------------------------------------------
    // Generate unique ID
    // --------------------------------------------------------

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_nanos();

    let id = format!("web-{timestamp:x}");


    // --------------------------------------------------------
    // Create Moneta Bookmark
    // --------------------------------------------------------

    let mut bookmark = Bookmark::new(
        id.clone(),
        "Other bookmarks".to_string(),
        selection.title.clone(),
        "web".to_string(),
    );


    // Store URL
    bookmark.source_url = Some(
        selection.url.clone()
    );


    // Store selected text
    bookmark.captured_text = Some(
        selection.selected_text.clone()
    );


    // --------------------------------------------------------
    // Open Markdown vault
    // --------------------------------------------------------

    let storage = match VaultStorage::new(
        "moneta-vault/md"
    ) {

        Ok(storage) => storage,

        Err(error) => {

            eprintln!(
                "Failed to open Moneta markdown vault: {}",
                error
            );

            return Json(
                serde_json::json!({
                    "status": "error",
                    "message": error.to_string()
                })
            );
        }
    };


    // --------------------------------------------------------
    // Save
    // --------------------------------------------------------

    match storage.save(&bookmark) {

        Ok(_) => {

            println!(
                "Web selection saved: {}.md",
                bookmark.id
            );

            Json(
                serde_json::json!({
                    "status": "ok",
                    "id": bookmark.id,
                    "path": format!(
                        "moneta-vault/md/{}.md",
                        bookmark.id
                    )
                })
            )
        }


        Err(error) => {

            eprintln!(
                "Failed to save web selection: {}",
                error
            );

            Json(
                serde_json::json!({
                    "status": "error",
                    "message": error.to_string()
                })
            )
        }
    }
}


// ============================================================
// CHROME BOOKMARK IMPORT
// ============================================================

async fn receive_bookmarks(
    Json(bookmarks): Json<Vec<ChromeBookmark>>,
) -> &'static str {

    println!(
        "Received {} bookmarks",
        bookmarks.len()
    );


    let storage = match VaultStorage::new(
        "moneta-vault/bookmarks"
    ) {

        Ok(storage) => storage,

        Err(error) => {

            eprintln!(
                "Failed to open Moneta vault: {error}"
            );

            return "ERROR";
        }
    };


    for chrome_bookmark in bookmarks {

        let id = create_bookmark_id(
            &chrome_bookmark
        );


        let mut bookmark = Bookmark::new(
            id,
            chrome_bookmark.folder_path.clone(),
            chrome_bookmark.title.clone(),
            "web".into(),
        );


        bookmark.source_url = Some(
            chrome_bookmark.url.clone()
        );


        bookmark.created_at =
            chrome_bookmark.created_at_in_chrome / 1000;

        bookmark.updated_at =
            bookmark.created_at;


        match storage.save(&bookmark) {

            Ok(_) => {

                println!(
                    "Saved: {} -> {}",
                    bookmark.title,
                    chrome_bookmark.url
                );
            }


            Err(error) => {

                eprintln!(
                    "Failed to save '{}': {}",
                    bookmark.title,
                    error
                );
            }
        }
    }


    "OK"
}

//for chrome
fn create_bookmark_id(
    bookmark: &ChromeBookmark
) -> String {

    let mut hash: u64 = 5381;


    for byte in bookmark.url.bytes() {

        hash = hash
            .wrapping_mul(33)
            .wrapping_add(byte as u64);
    }


    format!("web-{hash:x}")
}

// SERVER
pub async fn start_server() {

    let app = Router::new()

        .route(
            "/health",
            get(health_check)
        )

        .route(
            "/bookmarks",
            post(receive_bookmarks)
        )

        .route(
            "/selection",
            post(receive_selection)
        );


    let addr = SocketAddr::from(
        ([127, 0, 0, 1], 8765)
    );


    println!(
        "Moneta bookmark API listening on {}",
        addr
    );


    let listener =
        tokio::net::TcpListener::bind(addr)
            .await
            .expect(
                "Failed to bind Moneta bookmark API"
            );


    axum::serve(
        listener,
        app
    )
    .await
    .expect(
        "Moneta bookmark API failed"
    );
}