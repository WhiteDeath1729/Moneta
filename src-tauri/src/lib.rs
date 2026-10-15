use moneta::ai::LocalAIEngine;
use moneta::capture::{
    orchestrator::CaptureService, screenshot,
    selection::capture_selection as moneta_capture_selection,
};
use moneta::presentation::graph::{GraphData, KnowledgeGraph, SIMILARITY_EDGE_THRESHOLD};
use moneta::search::indexing::SqliteIndex;
use moneta::vault::{bookmark::Bookmark, storage::VaultStorage};

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, Window};

#[derive(Clone)]
pub struct AppState {
    pub ai_engine: Arc<LocalAIEngine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkItem {
    pub id: String,
    pub title: String,
    pub path: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub summary: Option<String>,
    pub content_preview: Option<String>,
    pub folder: String,
}

#[derive(Default)]
struct ShortcutDispatchStatus {
    listeners_ready: bool,
    pending: Vec<String>,
}

#[derive(Default)]
struct ShortcutDispatchState(Mutex<ShortcutDispatchStatus>);

#[derive(Clone, Copy)]
struct HotkeyIds {
    bookmark_file: u32,
    screenshot: u32,
    selection: u32,
    search: u32,
    screen_search: u32,
    graph: u32,
}

fn shortcut_key_from_args(args: &[String]) -> Vec<String> {
    let mut keys = Vec::new();
    let mut arguments = args.iter();

    while let Some(argument) = arguments.next() {
        let action = if let Some(action) = argument.strip_prefix("--moneta-action=") {
            Some(action)
        } else if argument == "--moneta-action" {
            arguments.next().map(String::as_str)
        } else if argument == "--bookmark-file" {
            Some("bookmark-file")
        } else if argument == "--analyze-screen" {
            Some("screen-search")
        } else {
            None
        };

        let key = match action {
            Some("bookmark-file") => Some("o"),
            Some("screenshot") => Some("s"),
            Some("selection") => Some("b"),
            Some("search") => Some("f"),
            Some("screen-search") => Some("r"),
            Some("graph") => Some("g"),
            _ => None,
        };
        if let Some(key) = key {
            keys.push(key.to_string());
        }
    }
    keys
}

fn shortcut_event(key: &str) -> Option<&'static str> {
    match key {
        "o" => Some("hotkey-bookmark-file"),
        "s" => Some("hotkey-capture-screenshot"),
        "b" => Some("hotkey-capture-selection"),
        "f" => Some("hotkey-search"),
        "r" => Some("hotkey-analyze-screen"),
        "g" => Some("open-graph-view"),
        _ => None,
    }
}

fn requires_background_context(key: &str) -> bool {
    matches!(key, "b" | "s" | "r")
}

fn dispatch_shortcut_key(app: &AppHandle, key: &str) {
    if matches!(key, "f" | "g") {
        show_main_window(app);
    }

    if let Some(event) = shortcut_event(key) {
        if let Err(error) = app.emit(event, "") {
            eprintln!("Failed to dispatch Moneta shortcut {key}: {error}");
        }
    }
}

fn dispatch_hotkey_event(event: GlobalHotKeyEvent, app: &AppHandle, ids: HotkeyIds) {
    if !matches!(event.state, global_hotkey::HotKeyState::Pressed) {
        return;
    }

    let key = if event.id == ids.bookmark_file {
        Some("o")
    } else if event.id == ids.screenshot {
        Some("s")
    } else if event.id == ids.selection {
        Some("b")
    } else if event.id == ids.search {
        Some("f")
    } else if event.id == ids.screen_search {
        Some("r")
    } else if event.id == ids.graph {
        Some("g")
    } else {
        None
    };

    if let Some(key) = key {
        dispatch_shortcut_key(app, key);
    }
}

fn queue_or_dispatch_shortcut_args(app: &AppHandle, args: &[String]) {
    let keys = shortcut_key_from_args(args);
    if keys.is_empty() {
        return;
    }

    let state = app.state::<ShortcutDispatchState>();
    let mut status = match state.0.lock() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("Moneta shortcut queue is unavailable: {error}");
            return;
        }
    };

    if status.listeners_ready {
        drop(status);
        for key in keys {
            dispatch_shortcut_key(app, &key);
        }
    } else {
        status.pending.extend(keys);
    }
}

#[tauri::command]
fn take_pending_shortcut_actions(
    state: tauri::State<'_, ShortcutDispatchState>,
) -> Result<Vec<String>, String> {
    let mut status = state
        .0
        .lock()
        .map_err(|error| format!("Moneta shortcut queue is unavailable: {error}"))?;
    status.listeners_ready = true;
    Ok(std::mem::take(&mut status.pending))
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultStructure {
    pub folders: Vec<String>,
    pub bookmarks: Vec<BookmarkItem>,
}

#[derive(Debug, Clone, Serialize)]
struct ChromeProfile {
    id: String,
    name: String,
    bookmark_count: usize,
}

#[derive(Debug, Serialize)]
struct ChromeSyncSummary {
    synced: usize,
    page_content_failures: usize,
}

#[derive(Debug)]
struct ChromeProfileFile {
    profile: ChromeProfile,
    bookmarks_path: PathBuf,
}

fn chrome_profiles() -> Result<Vec<ChromeProfileFile>, String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "Could not locate the Windows local application data folder.".to_string())?;
    let user_data_dir = PathBuf::from(local_app_data)
        .join("Google")
        .join("Chrome")
        .join("User Data");
    if !user_data_dir.is_dir() {
        return Err(format!(
            "Chrome profile directory was not found at {}.",
            user_data_dir.display()
        ));
    }

    let entries = fs::read_dir(&user_data_dir)
        .map_err(|error| format!("Could not list Chrome profiles: {error}"))?;
    let mut profiles = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Could not read a Chrome profile entry: {error}"))?;
        let profile_dir = entry.path();
        if !profile_dir.is_dir() {
            continue;
        }
        let bookmarks_path = profile_dir.join("Bookmarks");
        if !bookmarks_path.is_file() {
            continue;
        }

        let profile_json = fs::read(profile_dir.join("Preferences"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        let profile_name = profile_json
            .as_ref()
            .and_then(|value| value.pointer("/profile/name"))
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if entry.file_name() == "Default" {
                    "Default".to_string()
                } else {
                    entry.file_name().to_string_lossy().into_owned()
                }
            });
        let bookmark_count = read_chrome_bookmarks(&bookmarks_path)?.len();
        profiles.push(ChromeProfileFile {
            profile: ChromeProfile {
                id: entry.file_name().to_string_lossy().into_owned(),
                name: profile_name,
                bookmark_count,
            },
            bookmarks_path,
        });
    }
    profiles.sort_by(|left, right| left.profile.name.cmp(&right.profile.name));
    Ok(profiles)
}

fn read_chrome_bookmarks(path: &Path) -> Result<Vec<moneta::capture::web::ChromeBookmark>, String> {
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "Could not read Chrome bookmarks at {}: {error}",
            path.display()
        )
    })?;
    let root: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Chrome bookmark file is invalid JSON: {error}"))?;
    let roots = root
        .get("roots")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Chrome bookmark file does not contain bookmark roots.".to_string())?;
    let mut bookmarks = Vec::new();
    for (root_name, root_node) in roots {
        let folder = root_node
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or(root_name);
        collect_chrome_bookmarks(root_node, folder, &mut bookmarks)?;
    }
    Ok(bookmarks)
}

fn collect_chrome_bookmarks(
    node: &serde_json::Value,
    folder_path: &str,
    bookmarks: &mut Vec<moneta::capture::web::ChromeBookmark>,
) -> Result<(), String> {
    if let Some(url) = node.get("url").and_then(serde_json::Value::as_str) {
        if url.starts_with("http://") || url.starts_with("https://") {
            let created_at_in_chrome = node
                .get("date_added")
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|number| number.parse().ok())
                        .or_else(|| value.as_i64())
                })
                .ok_or_else(|| format!("Chrome bookmark has an invalid creation time: {url}"))?;
            bookmarks.push(moneta::capture::web::ChromeBookmark {
                created_at_in_chrome,
                title: node
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or(url)
                    .to_string(),
                url: url.to_string(),
                folder_path: if folder_path.is_empty() {
                    "Bookmarks".to_string()
                } else {
                    folder_path.to_string()
                },
            });
        }
    }

    if let Some(children) = node.get("children").and_then(serde_json::Value::as_array) {
        for child in children {
            let child_folder = child
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.is_empty());
            let child_path = child_folder
                .map(|name| {
                    if folder_path.is_empty() {
                        name.to_string()
                    } else {
                        format!("{folder_path}/{name}")
                    }
                })
                .unwrap_or_else(|| folder_path.to_string());
            collect_chrome_bookmarks(child, &child_path, bookmarks)?;
        }
    }
    Ok(())
}

#[tauri::command]
fn get_chrome_profiles() -> Result<Vec<ChromeProfile>, String> {
    Ok(chrome_profiles()?
        .into_iter()
        .map(|profile| profile.profile)
        .collect())
}

#[tauri::command]
async fn sync_chrome_bookmarks(profile_id: String) -> Result<ChromeSyncSummary, String> {
    let profile = chrome_profiles()?
        .into_iter()
        .find(|profile| profile.profile.id == profile_id)
        .ok_or_else(|| "The selected Chrome profile is no longer available.".to_string())?;
    let bookmarks = read_chrome_bookmarks(&profile.bookmarks_path)?;
    if bookmarks.is_empty() {
        return Ok(ChromeSyncSummary {
            synced: 0,
            page_content_failures: 0,
        });
    }

    let (synced, page_content_failures) =
        moneta::capture::web::sync_chrome_bookmarks_to_local(&bookmarks).await?;
    let summary = ChromeSyncSummary {
        synced,
        page_content_failures,
    };
    Ok(summary)
}

pub fn resolve_vault_dir() -> PathBuf {
    if Path::new("moneta-vault/bookmarks").exists() {
        PathBuf::from("moneta-vault")
    } else if Path::new("../moneta-vault/bookmarks").exists() {
        PathBuf::from("../moneta-vault")
    } else {
        PathBuf::from("moneta-vault")
    }
}

pub fn resolve_bookmarks_dir() -> PathBuf {
    resolve_vault_dir().join("bookmarks")
}

pub fn resolve_db_path() -> PathBuf {
    resolve_vault_dir().join("moneta_index.db")
}

fn safe_truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() > max_chars {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{truncated}...")
    } else {
        text.to_string()
    }
}

// -------------------------------------------------------------
// TAURI COMMANDS
// -------------------------------------------------------------

#[tauri::command]
fn get_vault_structure() -> Result<VaultStructure, String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage = VaultStorage::new(&bm_dir)
        .map_err(|e| format!("Could not open vault storage at {}: {e}", bm_dir.display()))?;

    let all_bms = storage.list_all().unwrap_or_default();
    let mut folders = vec![
        "Web Bookmarks".to_string(),
        "Images & OCR".to_string(),
        "Documents".to_string(),
        "Quick Notes".to_string(),
    ];

    let mut items = Vec::new();
    for bm in all_bms {
        let folder = match bm.source_type.as_str() {
            "web" => "Web Bookmarks".to_string(),
            "image" | "screenshot" => "Images & OCR".to_string(),
            "file" | "epub" | "pdf" => "Documents".to_string(),
            _ => "Quick Notes".to_string(),
        };

        let content_text = bm.captured_text.clone().or_else(|| bm.ocr_text.clone());
        let summary = content_text.as_ref().map(|t| safe_truncate(t, 180));

        items.push(BookmarkItem {
            id: bm.id.clone(),
            title: if bm.title.trim().is_empty() {
                bm.id.clone()
            } else {
                bm.title.clone()
            },
            path: bm.path.clone(),
            source_type: bm.source_type.clone(),
            source_url: bm.source_url.clone(),
            tags: bm.tags.clone(),
            created_at: bm.created_at,
            updated_at: bm.updated_at,
            summary,
            content_preview: content_text,
            folder,
        });
    }

    folders.sort();
    Ok(VaultStructure {
        folders,
        bookmarks: items,
    })
}

#[tauri::command]
fn get_bookmark_detail(app: AppHandle, id: String) -> Result<Bookmark, String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage =
        VaultStorage::new(&bm_dir).map_err(|e| format!("Could not open vault storage: {e}"))?;

    let mut bookmark = storage
        .load(&id)
        .map_err(|e| format!("Could not load bookmark {id}: {e}"))?;

    // Auto OCR for any image if OCR text is missing
    let is_image = bookmark.source_type == "image" || bookmark.source_type == "screenshot";
    if is_image {
        let image_path = PathBuf::from(&bookmark.path);
        let image_path = if image_path.is_absolute() {
            image_path
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Could not resolve screenshot path: {e}"))?
                .join(image_path)
        };
        let image_path = image_path.canonicalize().map_err(|e| {
            format!(
                "Could not open captured image {}: {e}",
                image_path.display()
            )
        })?;
        app.asset_protocol_scope()
            .allow_file(&image_path)
            .map_err(|e| format!("Could not authorize captured image for display: {e}"))?;
        bookmark.path = image_path.to_string_lossy().into_owned();
    }

    let lacks_ocr = bookmark
        .ocr_text
        .as_ref()
        .map(|t| t.trim().is_empty())
        .unwrap_or(true);
    if is_image && lacks_ocr {
        let img_path = std::path::PathBuf::from(&bookmark.path);
        if img_path.exists() {
            if let Ok(mut engine) = moneta::capture::ocr::OcrEngine::new() {
                if let Ok(text) = engine.extract_text(&img_path) {
                    let trimmed = text.trim();
                    let ocr_res = if trimmed.is_empty() {
                        "(No text detected in image)".to_string()
                    } else {
                        trimmed.to_string()
                    };
                    bookmark.ocr_text = Some(ocr_res.clone());
                    bookmark.captured_text = Some(ocr_res);
                    let _ = storage.save(&bookmark);
                }
            }
        }
    }

    Ok(bookmark)
}

#[tauri::command]
fn delete_bookmark(id: String) -> Result<(), String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage =
        VaultStorage::new(&bm_dir).map_err(|e| format!("Could not open vault storage: {e}"))?;

    // 1. Delete markdown file from vault
    let _ = storage.delete(&id);

    // 2. Delete embedding .bin file
    let emb_path = storage.embeddings_dir().join(format!("{id}.bin"));
    if emb_path.exists() {
        let _ = std::fs::remove_file(emb_path);
    }

    // 3. Delete from Sqlite index
    let db_path = resolve_db_path();
    if let Ok(conn) = rusqlite::Connection::open(&db_path) {
        let _ = conn.execute("DELETE FROM bookmarks WHERE id = ?1", [&id]);
        let _ = conn.execute("DELETE FROM bookmarks_fts WHERE id = ?1", [&id]);
        let _ = conn.execute("DELETE FROM embeddings WHERE bookmark_id = ?1", [&id]);
        let _ = conn.execute(
            "DELETE FROM links WHERE source_id = ?1 OR target_id = ?1",
            [&id],
        );
    }

    Ok(())
}

#[tauri::command]
fn delete_folder(folder: String) -> Result<usize, String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage =
        VaultStorage::new(&bm_dir).map_err(|e| format!("Could not open vault storage: {e}"))?;

    let all_bms = storage.list_all().unwrap_or_default();
    let mut deleted_count = 0;
    let db_path = resolve_db_path();
    let conn = rusqlite::Connection::open(&db_path).ok();

    for bm in all_bms {
        let item_folder = match bm.source_type.as_str() {
            "web" => "Web Bookmarks",
            "image" | "screenshot" => "Images & OCR",
            "file" | "epub" | "pdf" => "Documents",
            _ => "Quick Notes",
        };

        if item_folder == folder || bm.path.contains(&folder) {
            let _ = storage.delete(&bm.id);
            let emb_path = storage.embeddings_dir().join(format!("{}.bin", bm.id));
            if emb_path.exists() {
                let _ = std::fs::remove_file(emb_path);
            }
            if let Some(ref c) = conn {
                let _ = c.execute("DELETE FROM bookmarks WHERE id = ?1", [&bm.id]);
                let _ = c.execute("DELETE FROM bookmarks_fts WHERE id = ?1", [&bm.id]);
                let _ = c.execute("DELETE FROM embeddings WHERE bookmark_id = ?1", [&bm.id]);
                let _ = c.execute(
                    "DELETE FROM links WHERE source_id = ?1 OR target_id = ?1",
                    [&bm.id],
                );
            }
            deleted_count += 1;
        }
    }

    Ok(deleted_count)
}

#[tauri::command]
fn save_bookmark_detail(bookmark: Bookmark) -> Result<(), String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage =
        VaultStorage::new(&bm_dir).map_err(|e| format!("Could not open vault storage: {e}"))?;

    storage
        .save(&bookmark)
        .map_err(|e| format!("Could not save bookmark {}: {e}", bookmark.id))
}

#[tauri::command]
fn get_graph_data(state: State<'_, AppState>) -> Result<GraphData, String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage = VaultStorage::new(&bm_dir).map_err(|e| format!("Could not open vault: {e}"))?;
    let bookmarks = storage
        .list_all()
        .map_err(|e| format!("Could not list vault bookmarks for the graph: {e}"))?;

    let db_path = resolve_db_path();
    let index =
        SqliteIndex::open(&db_path).map_err(|e| format!("Could not open search index: {e}"))?;

    let offline_ai = moneta::ai::OfflineAIService::with_engine(state.ai_engine.clone());
    let mut embeddings: HashMap<String, Vec<f32>> = index
        .get_all_embeddings()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, emb)| !emb.is_empty())
        .collect();

    // Check if new bookmarks need local embedding
    for bookmark in &bookmarks {
        if !embeddings.contains_key(&bookmark.id) {
            let _ = index.index_bookmark(bookmark, Some(&offline_ai));
        }
    }

    embeddings = index
        .get_all_embeddings()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, emb)| !emb.is_empty())
        .collect();

    let mut graph = KnowledgeGraph::new();
    for bookmark in &bookmarks {
        graph.add_bookmark(bookmark);
    }

    let mut graph_data =
        graph.build_graph_with_similarities(&embeddings, SIMILARITY_EDGE_THRESHOLD);

    let (search_history, history_status) =
        moneta::presentation::graph::load_chrome_search_history();
    graph_data.search_history = search_history;
    graph_data.search_history_status = history_status;

    Ok(graph_data)
}

#[tauri::command]
fn bookmark_file(window: Window) -> Result<Option<BookmarkItem>, String> {
    let path = rfd::FileDialog::new()
        .add_filter(
            "Supported Files",
            &[
                "png", "jpg", "jpeg", "bmp", "webp", "txt", "md", "epub", "pdf", "docx", "json",
                "rs", "py", "js", "html",
            ],
        )
        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "webp"])
        .add_filter("Documents", &["txt", "md", "epub", "pdf", "docx"])
        .add_filter("All Files", &["*"])
        .pick_file();

    let Some(path) = path else {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(None);
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();

    let bm_dir = resolve_bookmarks_dir();
    let capture_service =
        CaptureService::new(&bm_dir).map_err(|e| format!("Vault initialization error: {e}"))?;

    let mut bookmark = capture_service
        .capture_file(&path)
        .map_err(|e| format!("Capture failed: {e}"))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "webp");

    if is_img {
        // Run OCR if text not yet extracted
        if bookmark
            .ocr_text
            .as_ref()
            .map(|t| t.trim().is_empty())
            .unwrap_or(true)
        {
            if let Ok(mut engine) = moneta::capture::ocr::OcrEngine::new() {
                if let Ok(text) = engine.extract_text(&path) {
                    let trimmed = text.trim();
                    let ocr_res = if trimmed.is_empty() {
                        "(No text detected in image)".to_string()
                    } else {
                        trimmed.to_string()
                    };
                    bookmark.ocr_text = Some(ocr_res.clone());
                    bookmark.captured_text = Some(ocr_res);
                    let _ = capture_service.storage().save(&bookmark);
                }
            }
        }
    }

    let preview_text = bookmark
        .captured_text
        .clone()
        .or_else(|| bookmark.ocr_text.clone());
    let summary = preview_text.as_ref().map(|t| safe_truncate(t, 180));
    let folder = if is_img {
        "Images & OCR".to_string()
    } else {
        "Documents".to_string()
    };

    Ok(Some(BookmarkItem {
        id: bookmark.id.clone(),
        title: bookmark.title.clone(),
        path: bookmark.path.clone(),
        source_type: bookmark.source_type.clone(),
        source_url: bookmark.source_url.clone(),
        tags: bookmark.tags.clone(),
        created_at: bookmark.created_at,
        updated_at: bookmark.updated_at,
        summary,
        content_preview: preview_text,
        folder,
    }))
}

#[tauri::command]
fn search_bookmarks(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<BookmarkItem>, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        let structure = get_vault_structure()?;
        return Ok(structure.bookmarks);
    }

    let structure = get_vault_structure()?;
    let mut results = Vec::new();

    // 100% offline search: title, tags, content match, plus local embeddings if available
    for bm in structure.bookmarks {
        let title_match = bm.title.to_lowercase().contains(&q);
        let tag_match = bm.tags.iter().any(|t| t.to_lowercase().contains(&q));
        let content_match = bm
            .content_preview
            .as_ref()
            .map(|c| c.to_lowercase().contains(&q))
            .unwrap_or(false);

        if title_match || tag_match || content_match {
            results.push(bm);
        }
    }

    Ok(results)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenSearchResult {
    pub query: String,
    pub results: Vec<BookmarkItem>,
}

pub fn do_capture_screenshot() -> Result<BookmarkItem, String> {
    let image = screenshot::capture_screen_region()?;

    let vault_dir = resolve_vault_dir();
    let images_dir = vault_dir.join("images");
    let _ = std::fs::create_dir_all(&images_dir);

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let image_filename = format!("screenshot-{timestamp}.png");
    let image_path = images_dir.join(&image_filename);
    image
        .save(&image_path)
        .map_err(|e| format!("Failed to save screenshot image: {e}"))?;

    let mut extracted_ocr: Option<String> = None;
    if let Ok(mut engine) = moneta::capture::ocr::OcrEngine::new() {
        if let Ok(text) = engine.extract_text(&image_path) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                extracted_ocr = Some(trimmed.to_string());
            }
        }
    }

    let id = format!("screenshot-{timestamp:x}");
    let title = if let Some(ref text) = extracted_ocr {
        let first_line = text.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            safe_truncate(first_line, 60)
        } else {
            format!("Screenshot {timestamp}")
        }
    } else {
        format!("Screenshot {timestamp}")
    };

    let mut bookmark = Bookmark::new(
        id.clone(),
        image_path.to_string_lossy().to_string(),
        title,
        "screenshot".to_string(),
    );
    bookmark.ocr_text = extracted_ocr.clone();
    bookmark.captured_text = extracted_ocr.clone();
    bookmark.created_at = timestamp;
    bookmark.updated_at = timestamp;

    let bm_dir = resolve_bookmarks_dir();
    let capture_service = CaptureService::new(&bm_dir).map_err(|e| format!("Vault error: {e}"))?;
    let saved = capture_service
        .save_and_enrich(bookmark)
        .map_err(|e| format!("Failed to save screenshot bookmark: {e}"))?;

    let preview = saved
        .captured_text
        .clone()
        .or_else(|| saved.ocr_text.clone());
    let summary = preview.as_ref().map(|t| safe_truncate(t, 180));

    Ok(BookmarkItem {
        id: saved.id,
        title: saved.title,
        path: saved.path,
        source_type: saved.source_type,
        source_url: saved.source_url,
        tags: saved.tags,
        created_at: saved.created_at,
        updated_at: saved.updated_at,
        summary,
        content_preview: preview,
        folder: "Images & OCR".to_string(),
    })
}

pub fn do_analyze_screen_text() -> Result<ScreenSearchResult, String> {
    let text = screenshot::capture_screen_text()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("No readable text was found on the screen.".into());
    }

    let structure = get_vault_structure()?;
    let query_lower = trimmed.to_lowercase();
    let words: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored_results: Vec<(usize, BookmarkItem)> = Vec::new();

    for bm in structure.bookmarks {
        let mut score = 0;
        let title_lower = bm.title.to_lowercase();
        let content_lower = bm.content_preview.as_deref().unwrap_or("").to_lowercase();

        // Exact match
        if title_lower.contains(&query_lower) {
            score += 50;
        }
        if content_lower.contains(&query_lower) {
            score += 30;
        }

        // Word matches
        for w in &words {
            if w.len() >= 3 {
                if title_lower.contains(w) {
                    score += 10;
                }
                if content_lower.contains(w) {
                    score += 5;
                }
                if bm.tags.iter().any(|t| t.to_lowercase().contains(w)) {
                    score += 10;
                }
            }
        }

        if score > 0 {
            scored_results.push((score, bm));
        }
    }

    scored_results.sort_by(|a, b| b.0.cmp(&a.0));
    let matching_items: Vec<BookmarkItem> =
        scored_results.into_iter().map(|(_, item)| item).collect();

    Ok(ScreenSearchResult {
        query: trimmed.to_string(),
        results: matching_items,
    })
}

#[tauri::command]
fn capture_screenshot(window: Window) -> Result<BookmarkItem, String> {
    let result = do_capture_screenshot();
    show_window(&window);
    result
}

#[tauri::command]
fn capture_selection(window: Window) -> Result<Option<BookmarkItem>, String> {
    let result = (|| {
        let selection = moneta_capture_selection().map_err(|e| e.to_string())?;
        let bm_dir = resolve_bookmarks_dir();
        let capture_service =
            CaptureService::new(&bm_dir).map_err(|e| format!("Vault initialization error: {e}"))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let id = format!("sel-{timestamp:x}");

        let mut bookmark = Bookmark::new(
            id,
            selection.source_path.clone().unwrap_or_default(),
            if selection.window_title.trim().is_empty() {
                "Selection Bookmark".to_string()
            } else {
                format!("Selection from {}", selection.window_title)
            },
            selection.source_type.clone(),
        );
        bookmark.captured_text = Some(selection.text.clone());
        bookmark.created_at = timestamp;
        bookmark.updated_at = timestamp;

        let saved = capture_service
            .save_and_enrich(bookmark)
            .map_err(|e| format!("Failed to save selection bookmark: {e}"))?;

        let summary = saved.captured_text.as_ref().map(|t| safe_truncate(t, 180));

        Ok(Some(BookmarkItem {
            id: saved.id.clone(),
            title: saved.title.clone(),
            path: saved.path.clone(),
            source_type: saved.source_type.clone(),
            source_url: saved.source_url.clone(),
            tags: saved.tags.clone(),
            created_at: saved.created_at,
            updated_at: saved.updated_at,
            summary,
            content_preview: saved.captured_text.clone(),
            folder: "Quick Notes".to_string(),
        }))
    })();

    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    result
}

#[tauri::command]
fn analyze_screen_text(window: Window) -> Result<ScreenSearchResult, String> {
    let result = do_analyze_screen_text();
    show_window(&window);
    result
}

fn show_window(window: &Window) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

#[tauri::command]
fn register_windows_context_menu() -> Result<String, String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let exe_path = std::env::current_exe().map_err(|e| format!("Could not find exe path: {e}"))?;
    let exe_str = exe_path.to_string_lossy().to_string();

    // 1. Directory Background context menu (Desktop & folder background right-click)
    let (bg_key, _) = hkcu
        .create_subkey(r"Software\Classes\Directory\Background\shell\MonetaScreenSearch")
        .map_err(|e| format!("Failed to create registry key: {e}"))?;
    bg_key
        .set_value("", &"Moneta: Analyze Screen & Bookmarks (Ctrl+Shift+R)")
        .map_err(|e| format!("Failed to set value: {e}"))?;
    bg_key
        .set_value("Icon", &exe_str)
        .map_err(|e| format!("Failed to set icon: {e}"))?;

    let (bg_cmd, _) = bg_key
        .create_subkey("command")
        .map_err(|e| format!("Failed to create command key: {e}"))?;
    bg_cmd
        .set_value("", &format!("\"{}\" --analyze-screen", exe_str))
        .map_err(|e| format!("Failed to set command: {e}"))?;

    // 2. Any File context menu
    let (file_key, _) = hkcu
        .create_subkey(r"Software\Classes\*\shell\MonetaBookmark")
        .map_err(|e| format!("Failed to create registry key: {e}"))?;
    file_key
        .set_value("", &"Moneta: Bookmark File (Ctrl+Shift+O)")
        .map_err(|e| format!("Failed to set value: {e}"))?;
    file_key
        .set_value("Icon", &exe_str)
        .map_err(|e| format!("Failed to set icon: {e}"))?;

    let (file_cmd, _) = file_key
        .create_subkey("command")
        .map_err(|e| format!("Failed to create file command: {e}"))?;
    file_cmd
        .set_value("", &format!("\"{}\" --bookmark-file \"%1\"", exe_str))
        .map_err(|e| format!("Failed to set file command: {e}"))?;

    Ok("Windows Explorer right-click context menu options registered successfully!".to_string())
}

#[tauri::command]
fn minimize_window(window: Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn toggle_maximize_window(window: Window) -> Result<bool, String> {
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        window.maximize().map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[tauri::command]
fn close_window(window: Window) -> Result<(), String> {
    // Keep Moneta available from the system tray.
    window.hide().map_err(|e| e.to_string())
}

// -------------------------------------------------------------
// APP RUNNER
// -------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_shortcuts = shortcut_key_from_args(&std::env::args().skip(1).collect::<Vec<_>>());
    let show_window_on_startup = !initial_shortcuts
        .iter()
        .any(|key| requires_background_context(key));

    let ai_engine = Arc::new(LocalAIEngine::default());
    let _ = ai_engine.init();

    // Start background Chrome bookmark integration web server
    let web_ai_engine = ai_engine.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("Tokio runtime failed");
        runtime.block_on(async {
            moneta::capture::web::start_server(web_ai_engine).await;
        });
    });

    let app_state = AppState {
        ai_engine: ai_engine.clone(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if shortcut_key_from_args(&args).is_empty() {
                show_main_window(app);
            } else {
                queue_or_dispatch_shortcut_args(app, &args);
            }
        }))
        .manage(app_state)
        .manage(ShortcutDispatchState(Mutex::new(
            ShortcutDispatchStatus {
                listeners_ready: false,
                pending: initial_shortcuts,
            },
        )))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep Moneta available from the system tray.
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .setup(move |app| {
            // Setup System Tray Menu
            let screen_search_i = MenuItem::with_id(
                app,
                "screen_search",
                "🔍 Analyze Screen & Find Bookmarks (Ctrl+Shift+R)",
                true,
                None::<&str>,
            )?;
            let bookmark_file_i = MenuItem::with_id(
                app,
                "bookmark_file",
                "📑 Bookmark File... (Ctrl+Shift+O)",
                true,
                None::<&str>,
            )?;
            let screenshot_i = MenuItem::with_id(
                app,
                "screenshot",
                "📸 Take Screenshot (Ctrl+Shift+S)",
                true,
                None::<&str>,
            )?;
            let graph_i = MenuItem::with_id(
                app,
                "graph",
                "🕸️ Open Bookmark Graph (Ctrl+Shift+G)",
                true,
                None::<&str>,
            )?;
            let reg_ctx_i = MenuItem::with_id(
                app,
                "reg_ctx",
                "⚙️ Register Windows Explorer Context Menu",
                true,
                None::<&str>,
            )?;
            let show_i =
                MenuItem::with_id(app, "show", "🪟 Show Moneta Vault", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "❌ Quit Moneta", true, None::<&str>)?;

            let tray_menu = Menu::with_items(
                app,
                &[
                    &screen_search_i,
                    &bookmark_file_i,
                    &screenshot_i,
                    &graph_i,
                    &reg_ctx_i,
                    &show_i,
                    &quit_i,
                ],
            )?;

            let _tray = TrayIconBuilder::new()
                .menu(&tray_menu)
                .on_menu_event(move |app, event| {
                    let id = event.id().as_ref();
                    match id {
                        "screen_search" => {
                            let app_h = app.clone();
                            std::thread::spawn(move || {
                                let _ = app_h.emit("screen-search-in-progress", "");
                                match do_analyze_screen_text() {
                                    Ok(res) => {
                                        let _ = app_h.emit("screen-search-results", &res);
                                    }
                                    Err(err) => {
                                        let _ = app_h.emit("screen-search-error", &err);
                                    }
                                }
                                if let Some(w) = app_h.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                            });
                        }
                        "bookmark_file" => {
                            let _ = app.emit("tray-bookmark-file", "");
                        }
                        "screenshot" => {
                            let app_h = app.clone();
                            std::thread::spawn(move || {
                                let _ = app_h.emit("screenshot-in-progress", "");
                                match do_capture_screenshot() {
                                    Ok(bm) => {
                                        let _ = app_h.emit("screenshot-captured", &bm);
                                    }
                                    Err(err) => {
                                        let _ = app_h.emit("screenshot-error", &err);
                                    }
                                }
                                if let Some(w) = app_h.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                            });
                        }
                        "graph" => {
                            let _ = app.emit("open-graph-view", "");
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "reg_ctx" => {
                            let _ = register_windows_context_menu();
                        }
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Context-sensitive shortcuts must run before Moneta takes focus.
            if show_window_on_startup {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }

            let hotkey_handle = app.handle().clone();
            std::thread::spawn(move || {
                let manager = match GlobalHotKeyManager::new() {
                    Ok(manager) => manager,
                    Err(error) => {
                        eprintln!("Failed to initialize global shortcuts: {error}");
                        return;
                    }
                };

                let bookmark_file = HotKey::new(
                    Some(Modifiers::CONTROL | Modifiers::SHIFT),
                    Code::KeyO,
                );
                let screenshot =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
                let selection =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyB);
                let search = HotKey::new(
                    Some(Modifiers::CONTROL | Modifiers::SHIFT),
                    Code::KeyF,
                );
                let screen_search =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
                let graph =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyG);
                let ids = HotkeyIds {
                    bookmark_file: bookmark_file.id(),
                    screenshot: screenshot.id(),
                    selection: selection.id(),
                    search: search.id(),
                    screen_search: screen_search.id(),
                    graph: graph.id(),
                };

                for (name, hotkey) in [
                    ("bookmark file", bookmark_file),
                    ("screenshot", screenshot),
                    ("selection bookmark", selection),
                    ("search", search),
                    ("screen analysis", screen_search),
                    ("bookmark graph", graph),
                ] {
                    if let Err(error) = manager.register(hotkey) {
                        eprintln!("Failed to register {name} shortcut: {error}");
                    }
                }

                #[cfg(windows)]
                {
                    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
                        dispatch_hotkey_event(event, &hotkey_handle, ids);
                    }));

                    use windows::Win32::UI::WindowsAndMessaging::{
                        DispatchMessageW, GetMessageW, TranslateMessage, MSG,
                    };

                    let mut message = MSG::default();
                    loop {
                        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                        if result.0 <= 0 {
                            if result.0 < 0 {
                                eprintln!(
                                    "Windows message loop for global shortcuts failed: {}",
                                    std::io::Error::last_os_error()
                                );
                            }
                            break;
                        }
                        unsafe {
                            let _ = TranslateMessage(&message);
                            let _ = DispatchMessageW(&message);
                        }
                    }
                }

                #[cfg(not(windows))]
                {
                    let receiver = GlobalHotKeyEvent::receiver();
                    loop {
                        match receiver.recv() {
                            Ok(event) => dispatch_hotkey_event(event, &hotkey_handle, ids),
                            Err(error) => {
                                eprintln!("Global shortcut event channel closed: {error}");
                                break;
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_vault_structure,
            get_bookmark_detail,
            get_chrome_profiles,
            sync_chrome_bookmarks,
            save_bookmark_detail,
            delete_bookmark,
            delete_folder,
            get_graph_data,
            bookmark_file,
            search_bookmarks,
            capture_screenshot,
            capture_selection,
            analyze_screen_text,
            register_windows_context_menu,
            minimize_window,
            toggle_maximize_window,
            close_window,
            take_pending_shortcut_actions
        ])
        .build(tauri::generate_context!())
        .expect("error while building Moneta Tauri application")
        .run(move |app, event| {
            if matches!(event, tauri::RunEvent::Ready) && show_window_on_startup {
                show_main_window(app);
            }
        });
}

#[cfg(test)]
mod shortcut_args_tests {
    use super::{requires_background_context, shortcut_key_from_args};

    #[test]
    fn maps_installer_shortcut_arguments_to_ui_actions() {
        let args = vec![
            "--moneta-action=bookmark-file".to_string(),
            "--moneta-action=screenshot".to_string(),
            "--moneta-action=selection".to_string(),
            "--moneta-action=search".to_string(),
            "--moneta-action=screen-search".to_string(),
            "--moneta-action=graph".to_string(),
        ];

        assert_eq!(
            shortcut_key_from_args(&args),
            ["o", "s", "b", "f", "r", "g"]
        );
    }

    #[test]
    fn accepts_existing_context_menu_action_arguments() {
        let args = vec![
            "--bookmark-file".to_string(),
            "C:\\docs\\notes.txt".to_string(),
            "--analyze-screen".to_string(),
        ];

        assert_eq!(shortcut_key_from_args(&args), ["o", "r"]);
    }

    #[test]
    fn keeps_screen_context_shortcuts_from_showing_the_window_first() {
        assert!(requires_background_context("b"));
        assert!(requires_background_context("s"));
        assert!(requires_background_context("r"));
        assert!(!requires_background_context("o"));
        assert!(!requires_background_context("f"));
        assert!(!requires_background_context("g"));
    }
}
