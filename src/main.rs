/*mod capture;

use capture::ocr::OcrEngine;
use rfd::FileDialog;
use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW,
    GetMessageW,
    RegisterHotKey,
    TranslateMessage,
    MSG,
    WM_HOTKEY,
};

const OCR_HOTKEY_ID: i32 = 1;

fn main() {
    println!("Moneta started.");
    println!("Press Ctrl + Shift + O to OCR an image.");
    println!("Press Ctrl + C to exit.");

    unsafe {
        RegisterHotKey(
            HWND::default(),
            OCR_HOTKEY_ID,
            MOD_CONTROL | MOD_SHIFT,
            'O' as u32,
        )
        .expect("Failed to register Ctrl+Shift+O");
    }

    println!("OCR shortcut registered.");

    let mut msg = MSG::default();

    unsafe {
        while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
            if msg.message == WM_HOTKEY {
                if msg.wParam.0 as i32 == OCR_HOTKEY_ID {
                    run_ocr();
                }
            }

            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn run_ocr() {
    println!("\n[Moneta Capture]");
    println!("Select an image...");

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

    println!("Image: {}", path.display());
    println!("Running OCR...");

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
            println!("================================\n");
        }
        Err(error) => {
            eprintln!("OCR failed: {error}");
        }
    }
} */
mod capture;

use capture::ocr::OcrEngine;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent,
    GlobalHotKeyManager,
};
use rfd::FileDialog;
use std::path::PathBuf;

fn main() {
    println!("Moneta started.");
    println!("Press Ctrl + Shift + O to OCR an image.");

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

    let receiver = GlobalHotKeyEvent::receiver();

    loop {
        if let Ok(event) = receiver.try_recv() {
            if event.id == hotkey.id() {
                run_ocr();
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
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