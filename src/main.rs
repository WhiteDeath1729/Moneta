mod capture;

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

    // Windows requires a real Win32 event loop for global-hotkey.
    let event_loop = EventLoop::new()
        .expect("Failed to create event loop");

    event_loop.set_control_flow(ControlFlow::Poll);

    // IMPORTANT:
    // The hotkey manager must be created on the same thread
    // as the Windows event loop.
    let manager = GlobalHotKeyManager::new()
        .expect("Failed to initialize global hotkey manager");

    let hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::SHIFT),
        Code::KeyO,
    );

    manager
        .register(hotkey)
        .expect("Failed to register OCR shortcut");

    println!("OCR shortcut registered.");
    println!("Waiting for Ctrl + Shift + O...");

    let receiver = GlobalHotKeyEvent::receiver();

    event_loop
        .run(move |event, _event_loop| {
            // Keep manager alive for the lifetime of the application.
            let _ = &manager;

            if let Event::AboutToWait = event {
                while let Ok(event) = receiver.try_recv() {
                    println!("EVENT RECEIVED: {:?}", event);

                    if event.id == hotkey.id() {
                        println!("OCR HOTKEY TRIGGERED!");

                        if matches!(
                            event.state,
                            global_hotkey::HotKeyState::Pressed
                        ) {
                            run_ocr();
                        }
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