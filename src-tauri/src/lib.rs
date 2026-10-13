use moneta::ai::LocalAIEngine;
use moneta::capture::{
    file::FileCaptureService, ocr::OcrEngine, orchestrator::CaptureService,
    screenshot, selection::capture_selection as moneta_capture_selection,
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultStructure {
    pub folders: Vec<String>,
    pub bookmarks: Vec<BookmarkItem>,
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
        "Writing".to_string(),
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

        let summary = bm.captured_text.as_ref().map(|t| safe_truncate(t, 180));

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
            content_preview: bm.captured_text.clone(),
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
fn get_bookmark_detail(id: String) -> Result<Bookmark, String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage = VaultStorage::new(&bm_dir)
        .map_err(|e| format!("Could not open vault storage: {e}"))?;

    storage
        .load(&id)
        .map_err(|e| format!("Could not load bookmark {id}: {e}"))
}

#[tauri::command]
fn save_bookmark_detail(bookmark: Bookmark) -> Result<(), String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage = VaultStorage::new(&bm_dir)
        .map_err(|e| format!("Could not open vault storage: {e}"))?;

    storage
        .save(&bookmark)
        .map_err(|e| format!("Could not save bookmark {}: {e}", bookmark.id))
}

#[tauri::command]
fn get_graph_data(state: State<'_, AppState>) -> Result<GraphData, String> {
    let bm_dir = resolve_bookmarks_dir();
    let storage = VaultStorage::new(&bm_dir)
        .map_err(|e| format!("Could not open vault: {e}"))?;
    let bookmarks = storage.list_all().unwrap_or_default();

    let db_path = resolve_db_path();
    let index = SqliteIndex::open(&db_path)
        .map_err(|e| format!("Could not open search index: {e}"))?;

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

    let (search_history, history_status) = moneta::presentation::graph::load_chrome_search_history();
    graph_data.search_history = search_history;
    graph_data.search_history_status = history_status;

    Ok(graph_data)
}

#[tauri::command]
fn bookmark_file() -> Result<Option<BookmarkItem>, String> {
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
        return Ok(None);
    };

    let bm_dir = resolve_bookmarks_dir();
    let capture_service = CaptureService::new(&bm_dir)
        .map_err(|e| format!("Vault initialization error: {e}"))?;

    let bookmark = capture_service
        .capture_file(&path)
        .map_err(|e| format!("Capture failed: {e}"))?;

    let summary = bookmark.captured_text.as_ref().map(|t| safe_truncate(t, 180));

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
        content_preview: bookmark.captured_text.clone(),
        folder: "Documents".to_string(),
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

#[tauri::command]
fn capture_screenshot() -> Result<String, String> {
    screenshot::run();
    Ok("Screenshot captured".into())
}

#[tauri::command]
fn capture_selection(state: State<'_, AppState>) -> Result<Option<BookmarkItem>, String> {
    let selection = moneta_capture_selection().map_err(|e| e.to_string())?;
    let bm_dir = resolve_bookmarks_dir();
    let capture_service = CaptureService::new(&bm_dir)
        .map_err(|e| format!("Vault initialization error: {e}"))?;

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
}

#[tauri::command]
fn analyze_screen_text(state: State<'_, AppState>) -> Result<Vec<BookmarkItem>, String> {
    let text = screenshot::capture_screen_text().map_err(|e| e.to_string())?;
    let query: String = if text.chars().count() > 100 {
        text.chars().take(100).collect()
    } else {
        text
    };
    search_bookmarks(query, state)
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
    window.close().map_err(|e| e.to_string())
}

// -------------------------------------------------------------
// APP RUNNER
// -------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
        .manage(app_state)
        .setup(move |app| {
            let handle = app.handle().clone();

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
            let show_i = MenuItem::with_id(
                app,
                "show",
                "🪟 Show Moneta Vault",
                true,
                None::<&str>,
            )?;
            let quit_i = MenuItem::with_id(
                app,
                "quit",
                "❌ Quit Moneta",
                true,
                None::<&str>,
            )?;

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
                            let _ = app.emit("tray-screen-search", "");
                            let _ = screenshot::capture_screen_text();
                        }
                        "bookmark_file" => {
                            let _ = app.emit("tray-bookmark-file", "");
                        }
                        "screenshot" => {
                            screenshot::run();
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

            // Setup Global Hotkeys in a background thread
            let hotkey_handle = handle.clone();
            std::thread::spawn(move || {
                let Ok(manager) = GlobalHotKeyManager::new() else {
                    return;
                };

                let bookmark_file_hk =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
                let screenshot_hk =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
                let selection_hk =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyB);
                let search_hk =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyF);
                let screen_search_hk =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
                let graph_hk =
                    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyG);

                let _ = manager.register(bookmark_file_hk);
                let _ = manager.register(screenshot_hk);
                let _ = manager.register(selection_hk);
                let _ = manager.register(search_hk);
                let _ = manager.register(screen_search_hk);
                let _ = manager.register(graph_hk);

                let receiver = GlobalHotKeyEvent::receiver();
                loop {
                    if let Ok(event) = receiver.recv() {
                        if !matches!(event.state, global_hotkey::HotKeyState::Pressed) {
                            continue;
                        }

                        if event.id == bookmark_file_hk.id() {
                            let _ = hotkey_handle.emit("hotkey-bookmark-file", "");
                        } else if event.id == screenshot_hk.id() {
                            screenshot::run();
                            let _ = hotkey_handle.emit("hotkey-screenshot-taken", "");
                        } else if event.id == selection_hk.id() {
                            let _ = hotkey_handle.emit("hotkey-capture-selection", "");
                        } else if event.id == search_hk.id() {
                            let _ = hotkey_handle.emit("hotkey-search", "");
                            if let Some(w) = hotkey_handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        } else if event.id == screen_search_hk.id() {
                            let _ = hotkey_handle.emit("hotkey-screen-search", "");
                        } else if event.id == graph_hk.id() {
                            let _ = hotkey_handle.emit("open-graph-view", "");
                            if let Some(w) = hotkey_handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
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
            save_bookmark_detail,
            get_graph_data,
            bookmark_file,
            search_bookmarks,
            capture_screenshot,
            capture_selection,
            analyze_screen_text,
            register_windows_context_menu,
            minimize_window,
            toggle_maximize_window,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("error while building Moneta Tauri application");
}
