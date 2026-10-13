mod ai;
mod capture;
mod presentation;
mod search;
mod settings;
mod vault;

use ai::{DocumentContext, LocalAIEngine};
use capture::{
    file::FileCaptureService,
    ocr::OcrEngine,
    orchestrator::CaptureService,
    selection::capture_selection,
};
use vault::{bookmark::Bookmark, storage::VaultStorage};

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use rfd::FileDialog;
use std::{
    collections::HashSet,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use winit::{
    event::Event,
    event_loop::{ControlFlow, EventLoop},
};

fn main() {
    println!("======================================");
    println!("              MONETA                  ");
    println!("======================================");
    println!();

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--test-ai" || arg == "--test") {
        run_test_mode();
        return;
    }

    println!("Initializing Moneta Local AI Engine...");
    let ai_engine = Arc::new(LocalAIEngine::default());
    if let Err(error) = ai_engine.init() {
        eprintln!("Local AI initialization warning: {error}");
    }
    println!();

    println!("Shortcuts:");
    println!("  Ctrl + Shift + O  -> Bookmark file");
    println!("  Ctrl + Shift + S  -> Take a screenshot");
    println!("  Ctrl + Shift + B  -> Bookmark selected text");
    println!("  Ctrl + Shift + F  -> Search bookmarks with a natural-language query");
    println!("  Ctrl + Shift + R  -> Analyze screen text and find relevant bookmarks");
    println!("  Ctrl + Shift + G  -> Open the bookmark graph");
    println!();

    /*
     * Start the bookmark web server in the background.
     * Keeps Chrome bookmark integration functioning.
     */
    let web_ai_engine = ai_engine.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        runtime.block_on(async {
            capture::web::start_server(web_ai_engine).await;
        });
    });

    /*
     * Create the Windows event loop.
     */
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let manager =
        GlobalHotKeyManager::new().expect("Failed to initialize global hotkey manager");

    let bookmark_file_hotkey =
        HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
    let screenshot_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
    let selection_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyB);
    let search_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyF);
    let screen_search_hotkey =
        HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
    let graph_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyG);

    manager
        .register(bookmark_file_hotkey)
        .expect("Failed to register bookmark file shortcut");
    manager.register(screenshot_hotkey).expect("Failed to register screenshot shortcut");
    manager.register(selection_hotkey).expect("Failed to register selection bookmark shortcut");
    manager
        .register(search_hotkey)
        .expect("Failed to register bookmark search shortcut");
    manager
        .register(screen_search_hotkey)
        .expect("Failed to register screen bookmark search shortcut");
    manager
        .register(graph_hotkey)
        .expect("Failed to register bookmark graph shortcut");

    println!("Bookmark file shortcut registered.");
    println!("Screenshot shortcut registered.");
    println!("Selection bookmark shortcut registered.");
    println!("Bookmark search shortcut registered.");
    println!("Screen bookmark search shortcut registered.");
    println!("Bookmark graph shortcut registered.");
    println!();
    println!("Waiting for shortcuts...");

    let receiver = GlobalHotKeyEvent::receiver();
    let search_input_lock = Arc::new(Mutex::new(()));

    event_loop
        .run(move |event, _event_loop| {
            let _ = &manager;

            if let Event::AboutToWait = event {
                while let Ok(event) = receiver.try_recv() {
                    if !matches!(event.state, global_hotkey::HotKeyState::Pressed) {
                        continue;
                    }

                    // Bookmark File Hotkey (formerly OCR)
                    if event.id == bookmark_file_hotkey.id() {
                        println!();
                        println!("======================================");
                        println!("BOOKMARK FILE HOTKEY TRIGGERED");
                        println!("======================================");
                        run_bookmark_file();
                    }

                    // Screenshot Hotkey
                    if event.id == screenshot_hotkey.id() {
                        println!();
                        println!("======================================");
                        println!("SCREENSHOT HOTKEY TRIGGERED");
                        println!("======================================");
                        capture::screenshot::run();
                    }

                    // Selection Bookmark Hotkey
                    if event.id == selection_hotkey.id() {
                        println!();
                        println!("======================================");
                        println!("SELECTION BOOKMARK HOTKEY TRIGGERED");
                        println!("======================================");

                        match capture_selection() {
                            Ok(selection) => {
                                println!();
                                println!("========== SELECTED TEXT ==========");
                                println!("{}", selection.text);
                                println!("===================================");

                                println!();
                                println!("========== SOURCE ==========");
                                println!("Window: {}", selection.window_title);
                                println!("Path: {:?}", selection.source_path);
                                println!("Type: {}", selection.source_type);
                                println!("============================");

                                // Perform expensive AI inference on a background thread
                                // to ensure the UI event loop is never frozen.
                                let ai_clone = ai_engine.clone();
                                std::thread::spawn(move || {
                                    match save_selection_as_bookmark(&selection, &ai_clone) {
                                        Ok(path) => {
                                            println!();
                                            println!("======================================");
                                            println!("BOOKMARK CREATED SUCCESSFULLY");
                                            println!("======================================");
                                            println!("Markdown file: {}", path.display());
                                        }
                                        Err(error) => {
                                            eprintln!();
                                            eprintln!("Failed to create bookmark: {error}");
                                        }
                                    }
                                });
                            }
                            Err(error) => {
                                eprintln!();
                                eprintln!("Selection capture failed: {error}");
                            }
                        }
                    }

                    if event.id == search_hotkey.id() {
                        let ai_clone = ai_engine.clone();
                        let input_lock = search_input_lock.clone();
                        std::thread::spawn(move || {
                            prompt_and_search_bookmarks(&ai_clone, &input_lock);
                        });
                    }

                    if event.id == screen_search_hotkey.id() {
                        let ai_clone = ai_engine.clone();
                        std::thread::spawn(move || {
                            println!("\n[Moneta Screen Search]");
                            match capture::screenshot::capture_screen_text() {
                                Ok(text) => {
                                    println!("\nScreen text recognized:");
                                    println!("{text}");
                                    if let Err(error) = search_bookmarks(&text, &ai_clone) {
                                        eprintln!("Screen bookmark search failed: {error}");
                                    }
                                }
                                Err(error) => {
                                    eprintln!("Could not analyze screen text: {error}");
                                }
                            }
                        });
                    }

                    if event.id == graph_hotkey.id() {
                        let ai_clone = ai_engine.clone();
                        std::thread::spawn(move || match generate_bookmark_graph(&ai_clone) {
                            Ok(path) => {
                                println!("Bookmark graph generated: {}", path.display());
                                if let Err(error) =
                                    std::process::Command::new("explorer.exe").arg(&path).spawn()
                                {
                                    eprintln!(
                                        "Could not open bookmark graph in the browser: {error}"
                                    );
                                }
                            }
                            Err(error) => eprintln!("Could not generate bookmark graph: {error}"),
                        });
                    }
                }
            }
        })
        .expect("Event loop failed");
}

fn prompt_and_search_bookmarks(ai: &LocalAIEngine, input_lock: &Mutex<()>) {
    let _input_guard = match input_lock.lock() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("Could not acquire console input: {error}");
            return;
        }
    };

    print!("\nDescribe the bookmarks you want to find: ");
    if let Err(error) = io::stdout().flush() {
        eprintln!("Could not display search prompt: {error}");
        return;
    }

    let mut query = String::new();
    if let Err(error) = io::stdin().lock().read_line(&mut query) {
        eprintln!("Could not read bookmark search query: {error}");
        return;
    }

    let query = query.trim();
    if query.is_empty() {
        println!("Bookmark search cancelled: query was empty.");
        return;
    }

    if let Err(error) = search_bookmarks(query, ai) {
        eprintln!("Bookmark search failed: {error}");
    }
}

fn search_bookmarks(query: &str, ai: &LocalAIEngine) -> Result<(), String> {
    let vault_path = Path::new("moneta-vault/bookmarks");
    let storage = VaultStorage::new(vault_path)
        .map_err(|error| format!("Could not open bookmark vault: {error}"))?;
    let bookmarks = storage
        .list_all()
        .map_err(|error| format!("Could not list bookmarks: {error}"))?;

    if bookmarks.is_empty() {
        println!("No bookmarks are available to search.");
        return Ok(());
    }

    let index_path = Path::new("moneta-vault/moneta_index.db");
    let index = search::indexing::SqliteIndex::open(index_path)
        .map_err(|error| format!("Could not open bookmark search index: {error}"))?;
    let indexed_ids: HashSet<String> = index
        .get_all_embeddings()
        .map_err(|error| format!("Could not load bookmark embeddings: {error}"))?
        .into_iter()
        .filter_map(|(id, embedding)| (!embedding.is_empty()).then_some(id))
        .collect();

    let offline_ai = ai::OfflineAIService::with_engine(Arc::new(ai.clone()));
    for bookmark in &bookmarks {
        if !indexed_ids.contains(&bookmark.id) {
            index
                .index_bookmark(bookmark, Some(&offline_ai))
                .map_err(|error| {
                    format!(
                        "Could not create search embedding for '{}': {error}",
                        bookmark.title
                    )
                })?;
        }
    }

    let engine = search::semantic::SearchEngine::new(Arc::new(index), Arc::new(offline_ai));
    let results = engine.semantic_search(query, bookmarks.len());
    if results.is_empty() {
        println!("No relevant bookmarks found for: {query}");
        return Ok(());
    }

    let bookmarks_by_id: std::collections::HashMap<&str, &Bookmark> = bookmarks
        .iter()
        .map(|bookmark| (bookmark.id.as_str(), bookmark))
        .collect();

    println!("\n========== RELEVANT BOOKMARKS ==========");
    for (rank, result) in results.iter().enumerate() {
        let Some(bookmark) = bookmarks_by_id.get(result.bookmark_id.as_str()) else {
            continue;
        };

        println!(
            "\n{}. {} (similarity: {:.3})",
            rank + 1,
            bookmark.title,
            result.score
        );
        println!("   Type: {}", bookmark.source_type);
        if let Some(url) = &bookmark.source_url {
            println!("   URL: {url}");
        } else if !bookmark.path.is_empty() {
            println!("   Path: {}", bookmark.path);
        }
        if !bookmark.tags.is_empty() {
            println!("   Tags: {}", bookmark.tags.join(", "));
        }
        if let Some(summary) = &bookmark.summary {
            if !summary.trim().is_empty() {
                println!("   Summary: {summary}");
            }
        }
    }
    println!("\n========================================");

    Ok(())
}

fn generate_bookmark_graph(ai: &LocalAIEngine) -> Result<PathBuf, String> {
    let storage = VaultStorage::new("moneta-vault/bookmarks")
        .map_err(|error| format!("Could not open bookmark vault: {error}"))?;
    let bookmarks = storage
        .list_all()
        .map_err(|error| format!("Could not list bookmarks: {error}"))?;
    if bookmarks.is_empty() {
        return Err("There are no bookmarks to display.".into());
    }

    let index = search::indexing::SqliteIndex::open("moneta-vault/moneta_index.db")
        .map_err(|error| format!("Could not open bookmark search index: {error}"))?;
    let offline_ai = ai::OfflineAIService::with_engine(Arc::new(ai.clone()));
    let mut embeddings: std::collections::HashMap<String, Vec<f32>> = index
        .get_all_embeddings()
        .map_err(|error| format!("Could not load bookmark embeddings: {error}"))?
        .into_iter()
        .filter(|(_, embedding)| !embedding.is_empty())
        .collect();

    for bookmark in &bookmarks {
        if !embeddings.contains_key(&bookmark.id) {
            index
                .index_bookmark(bookmark, Some(&offline_ai))
                .map_err(|error| {
                    format!(
                        "Could not generate embedding for '{}': {error}",
                        bookmark.title
                    )
                })?;
        }
    }
    embeddings = index
        .get_all_embeddings()
        .map_err(|error| format!("Could not reload bookmark embeddings: {error}"))?
        .into_iter()
        .filter(|(_, embedding)| !embedding.is_empty())
        .collect();

    let mut graph = presentation::graph::KnowledgeGraph::new();
    for bookmark in &bookmarks {
        graph.add_bookmark(bookmark);
    }
    let mut graph_data = graph.build_graph_with_similarities(
        &embeddings,
        presentation::graph::SIMILARITY_EDGE_THRESHOLD,
    );
    let (search_history, history_status) =
        presentation::graph::load_chrome_search_history();
    graph_data.search_history = search_history;
    graph_data.search_history_status = history_status;
    let html = graph_data
        .to_interactive_html()
        .map_err(|error| format!("Could not render bookmark graph: {error}"))?;
    let graph_path = Path::new("moneta-vault/bookmark-graph.html");
    std::fs::write(graph_path, html)
        .map_err(|error| format!("Could not save bookmark graph: {error}"))?;

    Ok(graph_path.to_path_buf())
}

/*
 * ============================================================
 * BOOKMARK FILE
 * ============================================================
 */
fn run_bookmark_file() {
    println!("\n[Moneta Bookmark File]");

    let path: Option<PathBuf> = FileDialog::new()
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
        println!("File bookmarking cancelled.");
        return;
    };

    println!("Bookmarking file: {}", path.display());
    let vault_dir = Path::new("moneta-vault/bookmarks");
    match CaptureService::new(vault_dir) {
        Ok(capture_service) => match capture_service.capture_file(&path) {
            Ok(bookmark) => {
                println!();
                println!("======================================");
                println!("BOOKMARK CREATED SUCCESSFULLY");
                println!("======================================");
                println!("Title: {}", bookmark.title);
                println!("ID: {}", bookmark.id);
            }
            Err(error) => {
                eprintln!("Failed to bookmark file: {error}");
            }
        },
        Err(error) => {
            eprintln!("Failed to initialize vault: {error}");
        }
    }
}

/*
 * ============================================================
 * SELECTED TEXT -> LOCAL AI ENGINE -> MARKDOWN BOOKMARK
 * ============================================================
 */
fn save_selection_as_bookmark(
    selection: &capture::selection::SelectionData,
    ai: &LocalAIEngine,
) -> Result<PathBuf, String> {
    let source_path = selection
        .source_path
        .as_ref()
        .ok_or_else(|| "No source file path was detected.".to_string())?;

    if selection.text.trim().is_empty() {
        return Err("Selected text is empty.".into());
    }

    println!();
    println!("======================================");
    println!("EXTRACTING COMPLETE SOURCE CONTEXT");
    println!("======================================");

    // 1. Extract complete source document text if available
    let full_source_text = FileCaptureService::extract_full_source_text(source_path);
    if let Some(ref doc) = full_source_text {
        println!(
            "Full source document extracted ({} characters).",
            doc.len()
        );
    } else {
        println!("Full source document not directly extractable; using metadata and selection context.");
    }

    // 2. Identify local surrounding context around the selection
    let surrounding_context = full_source_text
        .as_deref()
        .and_then(|doc| FileCaptureService::find_surrounding_context(doc, &selection.text, 600));

    // 3. Construct rich DocumentContext
    let mut doc_ctx = DocumentContext::new(
        selection.text.clone(),
        selection.source_type.clone(),
    );
    doc_ctx.source_path = Some(source_path.clone());
    doc_ctx.window_title = Some(selection.window_title.clone());
    doc_ctx.surrounding_context = surrounding_context;
    doc_ctx.full_document_text = full_source_text;

    let fallback_title = Path::new(source_path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Untitled Bookmark")
        .to_string();

    println!();
    println!("======================================");
    println!("RUNNING LOCAL EMBEDDED AI INFERENCE");
    println!("======================================");

    // 4. Run context-aware analysis with local LLM and embedding model
    let analysis = ai.analyze_context(&doc_ctx, &fallback_title)?;

    println!();
    println!("========== AI ANALYSIS ==========");
    println!("Title: {}", analysis.title);
    println!("Summary: {}", analysis.summary);
    println!("Tags: {:?}", analysis.tags);
    println!("Embedding vector size: {}", analysis.embedding.len());
    println!("=================================");

    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Could not get system time: {e}"))?
        .as_nanos()
        .to_string();

    let title = if analysis.title.trim().is_empty() {
        fallback_title
    } else {
        analysis.title.trim().to_string()
    };

    let mut bookmark = Bookmark::new(
        id.clone(),
        source_path.clone(),
        title,
        selection.source_type.clone(),
    );

    bookmark.captured_text = Some(selection.text.clone());
    bookmark.source_url = None;
    bookmark.summary = Some(analysis.summary);
    bookmark.tags = analysis.tags;

    let hash_input = format!(
        "{}:{}:{}:{}",
        bookmark.title,
        bookmark.source_url.as_deref().unwrap_or(""),
        bookmark.captured_text.as_deref().unwrap_or(""),
        bookmark.ocr_text.as_deref().unwrap_or("")
    );
    bookmark.content_hash = vault::metadata::MetadataService::compute_content_hash(&hash_input);

    let vault_path = "moneta-vault/bookmarks";
    let storage = VaultStorage::new(vault_path)
        .map_err(|e| format!("Could not initialize bookmark vault: {e}"))?;
    let offline_service = ai::OfflineAIService::with_engine(Arc::new(ai.clone()));
    if bookmark.tags.is_empty() {
        bookmark.tags = offline_service
            .suggest_tags(&bookmark)
            .into_iter()
            .map(|tag| vault::metadata::MetadataService::normalize_tag(&tag.name))
            .filter(|tag| !tag.is_empty())
            .collect();
    }

    // 5. Persist markdown bookmark
    storage.save(&bookmark)
        .map_err(|e| format!("Could not save bookmark: {e}"))?;

    // 6. Persist embedding vector to vault embeddings/ directory
    if analysis.embedding.is_empty() {
        return Err("Could not generate a bookmark embedding.".into());
    }
    storage
        .save_embedding(&bookmark.id, &analysis.embedding)
        .map_err(|e| format!("Could not save bookmark embedding: {e}"))?;

    // 7. Index in local SQLite database
    let db_path = Path::new("moneta-vault/moneta_index.db");
    let index = search::indexing::SqliteIndex::open(db_path)
        .map_err(|e| format!("Could not open bookmark search index: {e}"))?;
    index
        .index_bookmark_with_embedding(&bookmark, &offline_service, &analysis.embedding)
        .map_err(|e| format!("Could not index bookmark: {e}"))?;

    println!();
    println!("======================================");
    println!("AI-ENRICHED BOOKMARK SAVED");
    println!("======================================");
    println!("Title: {}", bookmark.title);
    println!("Tags: {:?}", bookmark.tags);
    println!("Summary: {:?}", bookmark.summary.as_deref().unwrap_or(""));
    println!("Markdown file: moneta-vault/bookmarks/{bookmark_id}.md", bookmark_id = bookmark.id);
    println!("Embedding file: moneta-vault/embeddings/{bookmark_id}.bin", bookmark_id = bookmark.id);
    println!("======================================");

    Ok(PathBuf::from(vault_path).join(format!("{}.md", bookmark.id)))
}

/*
 * ============================================================
 * LOCAL AI DEVELOPMENT & TEST MODE
 * ============================================================
 */
fn run_test_mode() {
    println!("Starting Moneta Local AI Engine in Standalone Test Mode...");
    let engine = LocalAIEngine::default();
    let _ = engine.init();

    let sample_selection = "Pierre Fatou and Gaston Julia independently arrive at Julia sets in the early 20th century.";
    let sample_doc = r#"
Complex dynamics is the study of dynamical systems defined by iteration of functions on complex number spaces.
In one-dimensional complex dynamics, the central objects of study are rational maps on the Riemann sphere.
Pierre Fatou and Gaston Julia independently arrive at Julia sets in the early 20th century.
Their work laid the groundwork for modern fractal geometry, holomorphic dynamics, and chaotic systems.
Today, Julia and Fatou sets are studied in relation to the Mandelbrot set, bifurcation theory, and renormalization.
"#;

    let mut doc_ctx = DocumentContext::new(sample_selection, "document");
    doc_ctx.full_document_text = Some(sample_doc.to_string());
    doc_ctx.surrounding_context = FileCaptureService::find_surrounding_context(sample_doc, sample_selection, 300);
    doc_ctx.source_path = Some(r"C:\Math\ComplexDynamics.pdf".to_string());
    doc_ctx.window_title = Some("Complex Dynamics - Fatou and Julia".to_string());

    println!();
    println!("Running Context-Aware Analysis...");
    match engine.analyze_context(&doc_ctx, "Complex Dynamics") {
        Ok(analysis) => {
            println!();
            println!("========== TEST ANALYSIS RESULT ==========");
            println!("Title: {}", analysis.title);
            println!("Summary: {}", analysis.summary);
            println!("Tags: {:?}", analysis.tags);
            println!("Embedding dimensions: {}", analysis.embedding.len());
            println!("==========================================");
        }
        Err(err) => {
            eprintln!("Analysis failed: {err}");
        }
    }
}