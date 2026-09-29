mod ai;
mod capture;
mod vault;

use capture::{
    ocr::OcrEngine,
    orchestrator::CaptureService,
    selection::capture_selection,
};

use vault::{
    bookmark::Bookmark,
    storage::VaultStorage,
};

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent,
    GlobalHotKeyManager,
};

use rfd::FileDialog;

use std::{
    path::{Path, PathBuf},
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
    println!("Shortcuts:");
    println!("  Ctrl + Shift + O  -> OCR an image");
    println!("  Ctrl + Shift + S  -> Take a screenshot");
    println!("  Ctrl + Shift + B  -> Bookmark selected text");
    println!();

    /*
     * Start the bookmark web server in the background.
     *
     * This keeps the Chrome bookmark integration working.
     */
    std::thread::spawn(|| {
        let runtime = tokio::runtime::Runtime::new()
            .expect("Failed to create Tokio runtime");

        runtime.block_on(async {
            capture::web::start_server().await;
        });
    });

    /*
     * Create the Windows event loop.
     *
     * IMPORTANT:
     * GlobalHotKeyManager must be created on the same
     * thread as the event loop.
     */
    let event_loop = EventLoop::new()
        .expect("Failed to create event loop");

    event_loop.set_control_flow(ControlFlow::Poll);

    let manager = GlobalHotKeyManager::new()
        .expect("Failed to initialize global hotkey manager");

    /*
     * OCR:
     * Ctrl + Shift + O
     */
    let ocr_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::SHIFT),
        Code::KeyO,
    );

    /*
     * Screenshot:
     * Ctrl + Shift + S
     */
    let screenshot_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::SHIFT),
        Code::KeyS,
    );

    /*
     * Selected text bookmark:
     * Ctrl + Shift + B
     */
    let selection_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::SHIFT),
        Code::KeyB,
    );

    manager
        .register(ocr_hotkey)
        .expect("Failed to register OCR shortcut");

    manager
        .register(screenshot_hotkey)
        .expect("Failed to register screenshot shortcut");

    manager
        .register(selection_hotkey)
        .expect("Failed to register selection bookmark shortcut");

    println!("OCR shortcut registered.");
    println!("Screenshot shortcut registered.");
    println!("Selection bookmark shortcut registered.");
    println!();
    println!("Waiting for shortcuts...");

    let receiver = GlobalHotKeyEvent::receiver();

    event_loop
        .run(move |event, _event_loop| {
            /*
             * Keep the hotkey manager alive for the entire
             * lifetime of the application.
             */
            let _ = &manager;

            if let Event::AboutToWait = event {
                while let Ok(event) = receiver.try_recv() {
                    println!("EVENT RECEIVED: {:?}", event);

                    /*
                     * We only care about key presses.
                     */
                    if !matches!(
                        event.state,
                        global_hotkey::HotKeyState::Pressed
                    ) {
                        continue;
                    }

                    /*
                     * ==============================
                     * OCR
                     * ==============================
                     */
                    if event.id == ocr_hotkey.id() {
                        println!();
                        println!("======================================");
                        println!("OCR HOTKEY TRIGGERED");
                        println!("======================================");

                        run_ocr();
                    }

                    /*
                     * ==============================
                     * SCREENSHOT
                     * ==============================
                     */
                    if event.id == screenshot_hotkey.id() {
                        println!();
                        println!("======================================");
                        println!("SCREENSHOT HOTKEY TRIGGERED");
                        println!("======================================");

                        capture::screenshot::run();
                    }

                    /*
                     * ==============================
                     * SELECTED TEXT BOOKMARK
                     * ==============================
                     */
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
                                println!(
                                    "Window: {}",
                                    selection.window_title
                                );

                                println!(
                                    "Path: {:?}",
                                    selection.source_path
                                );

                                println!(
                                    "Type: {}",
                                    selection.source_type
                                );

                                println!("============================");

                                match save_selection_as_bookmark(
                                    &selection,
                                ) {
                                    Ok(path) => {
                                        println!();
                                        println!(
                                            "======================================"
                                        );
                                        println!(
                                            "BOOKMARK CREATED SUCCESSFULLY"
                                        );
                                        println!(
                                            "======================================"
                                        );

                                        println!(
                                            "Markdown file: {}",
                                            path.display()
                                        );
                                    }

                                    Err(error) => {
                                        eprintln!();
                                        eprintln!(
                                            "Failed to create bookmark: {}",
                                            error
                                        );
                                    }
                                }
                            }

                            Err(error) => {
                                eprintln!(
                                    "Selection capture failed: {}",
                                    error
                                );
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
 *
 * Opens an image picker, runs Tesseract OCR and then creates
 * an image bookmark in the Moneta vault.
 */
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
            eprintln!(
                "OCR initialization failed: {error}"
            );
            return;
        }
    };

    match ocr.extract_text(&path) {
        Ok(text) => {
            println!();
            println!("========== OCR RESULT ==========");
            println!();
            println!("{text}");
            println!("================================");

            /*
             * Automatically create the image bookmark.
             */
            let vault_dir = Path::new(
                "moneta-vault/bookmarks"
            );

            match CaptureService::new(vault_dir) {
                Ok(capture_service) => {
                    match capture_service.capture_image(
                        &path,
                        false,
                    ) {
                        Ok(mut bookmark) => {
                            bookmark.ocr_text = Some(text);

                            match capture_service
                                .save_and_enrich(bookmark)
                            {
                                Ok(_) => {
                                    println!(
                                        "Bookmark created and saved to vault."
                                    );
                                }

                                Err(error) => {
                                    eprintln!(
                                        "Failed to save image bookmark: {error}"
                                    );
                                }
                            }
                        }

                        Err(error) => {
                            eprintln!(
                                "Failed to create image bookmark: {error}"
                            );
                        }
                    }
                }

                Err(error) => {
                    eprintln!(
                        "Failed to initialize vault: {error}"
                    );
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
 * SELECTED TEXT -> MARKDOWN BOOKMARK
 * ============================================================
 */
fn save_selection_as_bookmark(
    selection: &capture::selection::SelectionData,
) -> Result<PathBuf, String> {
    /*
     * A selected-text bookmark currently requires a source
     * file path.
     */
    let source_path = selection
        .source_path
        .as_ref()
        .ok_or_else(|| {
            "No source file path was detected.".to_string()
        })?;

    /*
     * Don't create an empty bookmark.
     */
    if selection.text.trim().is_empty() {
        return Err(
            "Selected text is empty.".into()
        );
    }

    /*
     * Generate a unique ID.
     */
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| {
            format!("Could not get system time: {e}")
        })?
        .as_nanos()
        .to_string();

    /*
     * Use the source document filename as the bookmark title.
     *
     * Example:
     *
     * C:\Documents\Research Paper.docx
     *
     * becomes:
     *
     * Research Paper
     */
    let title = Path::new(source_path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Untitled Bookmark")
        .to_string();

    /*
     * Create the bookmark using the existing Moneta
     * Bookmark structure.
     */
    let mut bookmark = Bookmark::new(
        id,
        source_path.clone(),
        title,
        selection.source_type.clone(),
    );

    /*
     * Store the selected text.
     */
    bookmark.captured_text =
        Some(selection.text.clone());

    /*
     * Local files don't currently have a URL.
     */
    bookmark.source_url = None;

    /*
     * Store it in the existing Markdown vault.
     */
    let vault_path =
        "moneta-vault/bookmarks";

    let storage = VaultStorage::new(vault_path)
        .map_err(|e| {
            format!(
                "Could not initialize bookmark vault: {e}"
            )
        })?;

    let bookmark_id =
        bookmark.id.clone();

    storage
        .save(&bookmark)
        .map_err(|e| {
            format!(
                "Could not save bookmark: {e}"
            )
        })?;

    Ok(
        PathBuf::from(vault_path)
            .join(format!("{bookmark_id}.md"))
    )
}