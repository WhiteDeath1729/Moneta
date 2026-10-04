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
    path::{Path, PathBuf},
    sync::Arc,
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
    println!("  Ctrl + Shift + O  -> OCR an image");
    println!("  Ctrl + Shift + S  -> Take a screenshot");
    println!("  Ctrl + Shift + B  -> Bookmark selected text");
    println!();

    /*
     * Start the bookmark web server in the background.
     * Keeps Chrome bookmark integration functioning.
     */
    std::thread::spawn(|| {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        runtime.block_on(async {
            capture::web::start_server().await;
        });
    });

    /*
     * Create the Windows event loop.
     */
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let manager =
        GlobalHotKeyManager::new().expect("Failed to initialize global hotkey manager");

    let ocr_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
    let screenshot_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
    let selection_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyB);

    manager.register(ocr_hotkey).expect("Failed to register OCR shortcut");
    manager.register(screenshot_hotkey).expect("Failed to register screenshot shortcut");
    manager.register(selection_hotkey).expect("Failed to register selection bookmark shortcut");

    println!("OCR shortcut registered.");
    println!("Screenshot shortcut registered.");
    println!("Selection bookmark shortcut registered.");
    println!();
    println!("Waiting for shortcuts...");

    let receiver = GlobalHotKeyEvent::receiver();

    event_loop
        .run(move |event, _event_loop| {
            let _ = &manager;

            if let Event::AboutToWait = event {
                while let Ok(event) = receiver.try_recv() {
                    if !matches!(event.state, global_hotkey::HotKeyState::Pressed) {
                        continue;
                    }

                    // OCR Hotkey
                    if event.id == ocr_hotkey.id() {
                        println!();
                        println!("======================================");
                        println!("OCR HOTKEY TRIGGERED");
                        println!("======================================");
                        run_ocr();
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
                }
            }
        })
        .expect("Event loop failed");
}

/*
 * ============================================================
 * OCR
 * ============================================================
 */
fn run_ocr() {
    println!("\n[Moneta Capture]");

    let path: Option<PathBuf> = FileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "webp"])
        .pick_file();

    let Some(path) = path else {
        println!("OCR cancelled.");
        return;
    };

    println!("Reading: {}", path.display());

    let mut ocr = match OcrEngine::new() {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("OCR initialization failed: {error}");
            return;
        }
    };

    match ocr.extract_text(&path) {
        Ok(text) => {
            println!();
            println!("========== OCR RESULT ==========");
            println!("{text}");
            println!("================================");

            let vault_dir = Path::new("moneta-vault/bookmarks");
            match CaptureService::new(vault_dir) {
                Ok(capture_service) => {
                    match capture_service.capture_image(&path, false) {
                        Ok(mut bookmark) => {
                            bookmark.ocr_text = Some(text);
                            match capture_service.save_and_enrich(bookmark) {
                                Ok(_) => {
                                    println!("Bookmark created and saved to vault.");
                                }
                                Err(error) => {
                                    eprintln!("Failed to save image bookmark: {error}");
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("Failed to create image bookmark: {error}");
                        }
                    }
                }
                Err(error) => {
                    eprintln!("Failed to initialize vault: {error}");
                }
            }
        }
        Err(error) => {
            eprintln!("OCR failed: {error}");
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

    // 5. Persist markdown bookmark
    storage.save(&bookmark)
        .map_err(|e| format!("Could not save bookmark: {e}"))?;

    // 6. Persist embedding vector to vault embeddings/ directory
    if !analysis.embedding.is_empty() {
        if let Err(e) = storage.save_embedding(&bookmark.id, &analysis.embedding) {
            eprintln!("Warning: failed to persist binary embedding: {e}");
        }
    }

    // 7. Index in local SQLite database
    let db_path = Path::new("moneta-vault/moneta_index.db");
    if let Ok(index) = search::indexing::SqliteIndex::open(db_path) {
        let offline_service = ai::OfflineAIService::with_engine(Arc::new(ai.clone()));
        let _ = index.index_bookmark(&bookmark, Some(&offline_service));
    }

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