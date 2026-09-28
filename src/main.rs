mod capture;
mod vault;

use capture::ocr::OcrEngine;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent,
    GlobalHotKeyManager,
};
use rfd::FileDialog;
use std::path::PathBuf;
use winit::{
    event::Event,
    event_loop::{ControlFlow, EventLoop},
};

fn main() {
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
        }

        Err(error) => {
            eprintln!("OCR failed: {error}");
        }
    }
}