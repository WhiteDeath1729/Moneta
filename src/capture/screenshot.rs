use arboard::Clipboard;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::capture::ocr::OcrEngine;
use crate::vault::bookmark::Bookmark;
use crate::vault::storage::VaultStorage;

pub fn run() {
    println!("\n[Moneta Screenshot]");

    let mut clipboard = match Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(error) => {
            eprintln!("Failed to access clipboard: {error}");
            return;
        }
    };

    if let Err(error) = clipboard.clear() {
        eprintln!("Failed to clear clipboard: {error}");
        return;
    }

    println!("Opening Windows Snipping Tool...");
    println!("Select an area of the screen.");

    if let Err(error) = Command::new("explorer.exe")
        .arg("ms-screenclip:")
        .spawn()
    {
        eprintln!("Failed to launch Snipping Tool: {error}");
        return;
    }

    println!("Waiting for screenshot...");

    let image = loop {
        thread::sleep(Duration::from_millis(300));

        match clipboard.get_image() {
            Ok(image) => break image,
            Err(_) => continue,
        }
    };

    println!("Screenshot received from clipboard.");

    let width = image.width as u32;
    let height = image.height as u32;

    let rgba = image.bytes.into_owned();

    let Some(image_buffer) =
        image::RgbaImage::from_raw(width, height, rgba)
    else {
        eprintln!("Failed to create image buffer.");
        return;
    };

    let images_dir = PathBuf::from("moneta-vault/images");

    if let Err(error) = std::fs::create_dir_all(&images_dir) {
        eprintln!("Failed to create image directory: {error}");
        return;
    }

    let timestamp = current_timestamp();

    let image_path =
        images_dir.join(format!("screenshot-{timestamp}.png"));

    if let Err(error) = image_buffer.save(&image_path) {
        eprintln!("Failed to save screenshot: {error}");
        return;
    }

    println!(
        "Screenshot saved: {}",
        image_path.display()
    );

    run_ocr(image_path);
}

fn run_ocr(image_path: PathBuf) {
    println!("Running OCR...");

    let mut ocr = match OcrEngine::new() {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("OCR initialization failed: {error}");
            return;
        }
    };

    match ocr.extract_text(&image_path) {
        Ok(text) => {
            println!(
                "\n========== SCREENSHOT OCR ==========\n"
            );

            println!("{text}");

            println!(
                "===================================="
            );

            create_bookmark(image_path, text);
        }

        Err(error) => {
            eprintln!("Screenshot OCR failed: {error}");
        }
    }
}

fn create_bookmark(
    image_path: PathBuf,
    ocr_text: String,
) {
    println!("Creating Moneta bookmark...");

    let timestamp = current_timestamp();

    let id = format!("image-{timestamp}");

    let mut bookmark = Bookmark::new(
        id,
        image_path.to_string_lossy().to_string(),
        "Screenshot".into(),
        "image".into(),
    );

    bookmark.ocr_text = Some(ocr_text);

    bookmark.created_at = timestamp;
    bookmark.updated_at = timestamp;

    let storage = match VaultStorage::new(
        "moneta-vault/bookmarks"
    ) {
        Ok(storage) => storage,

        Err(error) => {
            eprintln!(
                "Failed to open Moneta vault: {error}"
            );
            return;
        }
    };

    match storage.save(&bookmark) {
        Ok(_) => {
            println!(
                "Bookmark saved: {}",
                bookmark.id
            );
        }

        Err(error) => {
            eprintln!(
                "Failed to save bookmark: {error}"
            );
        }
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs() as i64
}