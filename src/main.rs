use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent,
    GlobalHotKeyManager,
};
use rfd::FileDialog;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use winit::{
    event::Event,
    event_loop::{ControlFlow, EventLoop},
};

use moneta::ai::OfflineAIService;
use moneta::capture::ocr::OcrEngine;
use moneta::capture::orchestrator::CaptureService;
use moneta::capture::{self};
use moneta::search::indexing::SqliteIndex;
use moneta::search::semantic::SearchEngine;
use moneta::vault::storage::VaultStorage;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Check for CLI subcommands
    if args.len() > 1 {
        match args[1].as_str() {
            "--server" => {
                println!("Starting Moneta bookmark server on 127.0.0.1:8765...");
                let runtime = tokio::runtime::Runtime::new()
                    .expect("Failed to create Tokio runtime");
                runtime.block_on(async {
                    capture::web::start_server().await;
                });
                return;
            }
            "--rebuild" => {
                println!("Rebuilding Moneta index from markdown vault...");
                let vault_dir = Path::new("moneta-vault/bookmarks");
                let storage = match VaultStorage::new(vault_dir) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error opening vault: {e}");
                        return;
                    }
                };
                let db_path = Path::new("moneta-vault/moneta_index.db");
                let index = match SqliteIndex::open(db_path) {
                    Ok(idx) => idx,
                    Err(e) => {
                        eprintln!("Error opening SQLite index: {e}");
                        return;
                    }
                };
                let ai = OfflineAIService::new();
                match index.rebuild_from_vault(&storage, Some(&ai)) {
                    Ok(count) => println!("Successfully rebuilt index: {count} bookmarks indexed."),
                    Err(e) => eprintln!("Error during rebuild: {e}"),
                }
                return;
            }
            "--search" => {
                if args.len() < 3 {
                    eprintln!("Usage: moneta --search <query>");
                    return;
                }
                let query = &args[2..].join(" ");
                println!("Searching for: '{query}'");
                let db_path = Path::new("moneta-vault/moneta_index.db");
                let index = Arc::new(match SqliteIndex::open(db_path) {
                    Ok(idx) => idx,
                    Err(e) => {
                        eprintln!("Error opening SQLite index: {e}");
                        return;
                    }
                });
                let ai = Arc::new(OfflineAIService::new());
                let search_engine = SearchEngine::new(index, ai);
                let results = search_engine.hybrid_search(query, 10);
                println!("\nFound {} results:", results.len());
                for (i, res) in results.iter().enumerate() {
                    println!("  {}. [{}] Score: {:.3} (Type: {})", i + 1, res.bookmark_id, res.score, res.match_type);
                }
                return;
            }
            "--tag" => {
                if args.len() < 3 {
                    eprintln!("Usage: moneta --tag <text or title>");
                    return;
                }
                let input = args[2..].join(" ");
                println!("Generating AI tags for: '{input}'...");
                let ai = OfflineAIService::new_with_ai_model(moneta::ai::tagging::AiModelConfig::default());
                let mut bm = moneta::vault::bookmark::Bookmark::new(
                    "cli-tag".into(),
                    "".into(),
                    input.clone(),
                    "cli".into(),
                );
                bm.captured_text = Some(input);
                let tags = ai.suggest_tags(&bm);
                println!("\nSuggested Tags ({}):", tags.len());
                for t in tags {
                    println!("  - #{} (confidence: {:.0}%, source: {})", t.name, t.confidence.unwrap_or(0.9) * 100.0, t.source);
                }
                return;
            }
            "--help" | "-h" => {
                println!("Moneta - Context-Aware, Local-First Bookmarking System");
                println!("Usage:");
                println!("  moneta                Start desktop hotkey listener and background API server");
                println!("  moneta --server       Run the HTTP API server synchronously");
                println!("  moneta --rebuild      Rebuild SQLite derived index from Markdown vault");
                println!("  moneta --search <q>   Perform hybrid search across bookmarked content");
                println!("  moneta --tag <text>   Generate AI tags for text, title, or content");
                return;
            }
            _ => {}
        }
    }

    println!("Moneta started.");
    println!("Press Ctrl + Shift + O to OCR an image.");
    println!("Press Ctrl + Shift + S to take a screenshot.");

    std::thread::spawn(|| {
        let runtime = tokio::runtime::Runtime::new()
            .expect("Failed to create Tokio runtime");

        runtime.block_on(async {
            capture::web::start_server().await;
        });
    });

    let event_loop = EventLoop::new()
        .expect("Failed to create event loop");

    event_loop.set_control_flow(ControlFlow::Poll);

    // IMPORTANT:
    // The hotkey manager must be created on the same thread
    // as the Windows event loop.
    let manager = GlobalHotKeyManager::new()
        .expect("Failed to initialize global hotkey manager");

    let ocr_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::SHIFT),
        Code::KeyO,
    );

    let screenshot_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::SHIFT),
        Code::KeyS,
    );

    manager
        .register(ocr_hotkey)
        .expect("Failed to register OCR shortcut");

    manager
        .register(screenshot_hotkey)
        .expect("Failed to register screenshot shortcut");

    println!("OCR shortcut registered.");
    println!("Screenshot shortcut registered.");
    println!("Waiting for shortcuts...");

    let receiver = GlobalHotKeyEvent::receiver();

    event_loop
        .run(move |event, _event_loop| {
            // Keep manager alive for the lifetime of the application.
            let _ = &manager;

            if let Event::AboutToWait = event {
                while let Ok(event) = receiver.try_recv() {
                    println!("EVENT RECEIVED: {:?}", event);

                    if !matches!(
                        event.state,
                        global_hotkey::HotKeyState::Pressed
                    ) {
                        continue;
                    }

                    if event.id == ocr_hotkey.id() {
                        println!("OCR HOTKEY TRIGGERED!");
                        run_ocr();
                    }

                    if event.id == screenshot_hotkey.id() {
                        println!("SCREENSHOT HOTKEY TRIGGERED!");
                        capture::screenshot::run();
                    }
                }
            }
        })
        .expect("Event loop failed");
}

fn run_ocr() {
    println!("\n[Moneta Capture]");

    let path: Option<PathBuf> = FileDialog::new()
        .add_filter(
            "Images",
            &["png", "jpg", "jpeg", "bmp", "webp"],
        )
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
            println!("\n========== OCR RESULT ==========\n");
            println!("{text}");
            println!("================================");

            // Automatically create and enrich bookmark
            let vault_dir = Path::new("moneta-vault/bookmarks");
            if let Ok(capture_service) = CaptureService::new(vault_dir) {
                match capture_service.capture_image(&path, false) {
                    Ok(mut bm) => {
                        bm.ocr_text = Some(text);
                        let _ = capture_service.save_and_enrich(bm);
                        println!("Bookmark created and saved to vault.");
                    }
                    Err(e) => eprintln!("Failed to save image bookmark: {e}"),
                }
            }
        }

        Err(error) => {
            eprintln!("OCR failed: {error}");
        }
    }
}