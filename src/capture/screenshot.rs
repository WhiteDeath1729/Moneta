use arboard::Clipboard;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::capture::ocr::OcrEngine;
use crate::capture::orchestrator::CaptureService;
use crate::vault::bookmark::Bookmark;

pub fn run() {
    println!("\n[Moneta Screenshot]");

    let image = match capture_screen_region() {
        Ok(image) => image,
        Err(error) => {
            eprintln!("Screenshot capture failed: {error}");
            return;
        }
    };

    let images_dir = PathBuf::from("moneta-vault/images");

    if let Err(error) = std::fs::create_dir_all(&images_dir) {
        eprintln!("Failed to create image directory: {error}");
        return;
    }

    let timestamp = current_timestamp();

    let image_path =
        images_dir.join(format!("screenshot-{timestamp}.png"));

    if let Err(error) = image.save(&image_path) {
        eprintln!("Failed to save screenshot: {error}");
        return;
    }

    println!(
        "Screenshot saved: {}",
        image_path.display()
    );

    run_ocr(image_path);
}

pub fn capture_screen_text() -> Result<String, String> {
    println!("Select the part of the screen you want Moneta to analyze.");
    let image = capture_screen_region()?;

    let image_path = std::env::temp_dir().join(format!(
        "moneta-screen-search-{}.png",
        current_timestamp_nanos()?
    ));
    image
        .save(&image_path)
        .map_err(|error| format!("Could not prepare screen image for OCR: {error}"))?;

    let ocr_result = OcrEngine::new()
        .and_then(|mut ocr| ocr.extract_text(&image_path));
    let cleanup_result = std::fs::remove_file(&image_path);

    let text = ocr_result?;
    cleanup_result.map_err(|error| format!("Could not remove temporary screen image: {error}"))?;
    if text.trim().is_empty() {
        return Err("No readable text was found in the selected screen area.".into());
    }

    Ok(text)
}

fn capture_screen_region() -> Result<image::RgbaImage, String> {
    let mut clipboard =
        Clipboard::new().map_err(|error| format!("Failed to access clipboard: {error}"))?;
    clipboard
        .clear()
        .map_err(|error| format!("Failed to prepare clipboard for screen capture: {error}"))?;

    println!("Opening Windows Snipping Tool...");
    Command::new("explorer.exe")
        .arg("ms-screenclip:")
        .spawn()
        .map_err(|error| format!("Failed to launch Snipping Tool: {error}"))?;

    println!("Waiting for screen selection...");
    let timeout = Duration::from_secs(120);
    let started = std::time::Instant::now();
    let image = loop {
        if started.elapsed() >= timeout {
            return Err("Screen selection timed out or was cancelled.".into());
        }
        thread::sleep(Duration::from_millis(300));

        if let Ok(image) = clipboard.get_image() {
            break image;
        }
    };

    image::RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or_else(|| "Could not decode the selected screen image.".into())
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

    let capture_service = match CaptureService::new("moneta-vault/bookmarks") {
        Ok(capture_service) => capture_service,

        Err(error) => {
            eprintln!(
                "Failed to open Moneta vault: {error}"
            );
            return;
        }
    };

    match capture_service.save_and_enrich(bookmark) {
        Ok(bookmark) => {
            println!(
                "Enriched screenshot bookmark saved: {}",
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

fn current_timestamp_nanos() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| format!("Could not get system time: {error}"))
}