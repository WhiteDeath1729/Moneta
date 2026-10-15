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
    println!("Capturing the full screen for context analysis.");
    let image = capture_full_screen()?;

    let temp_name = format!(
        "moneta-screen-search-{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let image_path = std::env::temp_dir().join(temp_name);
    image
        .save(&image_path)
        .map_err(|error| format!("Could not prepare screen image for OCR: {error}"))?;

    let ocr_result = match OcrEngine::new() {
        Ok(mut ocr) => ocr.extract_text(&image_path),
        Err(e) => Err(format!("OCR engine initialization failed: {e}")),
    };

    let _ = std::fs::remove_file(&image_path);

    let text = ocr_result?;
    if text.trim().is_empty() {
        return Err("No readable text was found on the screen.".into());
    }

    Ok(text)
}

#[cfg(windows)]
fn capture_full_screen() -> Result<image::RgbaImage, String> {
    use windows::Win32::{
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
            CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits,
            HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
        },
        UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        },
    };

    struct CaptureHandles {
        screen_dc: Option<HDC>,
        memory_dc: Option<HDC>,
        bitmap: Option<HBITMAP>,
        old_bitmap: Option<HGDIOBJ>,
    }

    impl Drop for CaptureHandles {
        fn drop(&mut self) {
            unsafe {
                if let (Some(memory_dc), Some(old_bitmap)) = (self.memory_dc, self.old_bitmap) {
                    let _ = SelectObject(memory_dc, old_bitmap);
                }
                if let Some(bitmap) = self.bitmap {
                    let _ = DeleteObject(bitmap.into());
                }
                if let Some(memory_dc) = self.memory_dc {
                    let _ = DeleteDC(memory_dc);
                }
                if let Some(screen_dc) = self.screen_dc {
                    let _ = ReleaseDC(None, screen_dc);
                }
            }
        }
    }

    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    if width <= 0 || height <= 0 {
        return Err("Could not determine the screen dimensions.".into());
    }

    let mut handles = CaptureHandles {
        screen_dc: None,
        memory_dc: None,
        bitmap: None,
        old_bitmap: None,
    };
    let screen_dc = unsafe { GetDC(None) };
    if screen_dc.0.is_null() {
        return Err("Could not access the screen for context reading.".into());
    }
    handles.screen_dc = Some(screen_dc);

    let memory_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
    if memory_dc.0.is_null() {
        return Err("Could not prepare a screen image for context reading.".into());
    }
    handles.memory_dc = Some(memory_dc);

    let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
    if bitmap.0.is_null() {
        return Err("Could not allocate a full-screen image.".into());
    }
    handles.bitmap = Some(bitmap);

    let old_bitmap = unsafe { SelectObject(memory_dc, bitmap.into()) };
    if old_bitmap.0.is_null() {
        return Err("Could not initialize the full-screen image.".into());
    }
    handles.old_bitmap = Some(old_bitmap);

    let raster_operation = windows::Win32::Graphics::Gdi::ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0);
    unsafe { BitBlt(memory_dc, 0, 0, width, height, Some(screen_dc), x, y, raster_operation) }
        .map_err(|error| format!("Could not capture the full screen: {error}"))?;

    unsafe {
        SelectObject(memory_dc, old_bitmap);
    }
    handles.old_bitmap = None;

    let image_size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "The full-screen image is too large to process.".to_string())?;
    let mut bgra = vec![0; image_size];
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: image_size as u32,
        ..Default::default()
    };
    let copied_lines = unsafe {
        GetDIBits(
            screen_dc,
            bitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    if copied_lines != height {
        return Err(format!(
            "Could not read the full-screen image (copied {copied_lines} of {height} rows)."
        ));
    }

    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    image::RgbaImage::from_raw(width as u32, height as u32, bgra)
        .ok_or_else(|| "Could not decode the full-screen image.".into())
}

#[cfg(not(windows))]
fn capture_full_screen() -> Result<image::RgbaImage, String> {
    Err("Automatic full-screen reading is currently supported on Windows.".into())
}

pub fn capture_screen_region() -> Result<image::RgbaImage, String> {
    if let Ok(mut cb) = Clipboard::new() {
        let _ = cb.clear();
    }

    println!("Opening Windows Snipping Tool...");
    let _ = Command::new("explorer.exe")
        .arg("ms-screenclip:")
        .spawn()
        .map_err(|error| format!("Failed to launch Snipping Tool: {error}"))?;

    println!("Waiting for screen selection...");
    let timeout = Duration::from_secs(90);
    let started = std::time::Instant::now();

    loop {
        if started.elapsed() >= timeout {
            return Err("Screen selection timed out or was cancelled.".into());
        }
        thread::sleep(Duration::from_millis(250));

        if let Ok(mut cb) = Clipboard::new() {
            if let Ok(image) = cb.get_image() {
                if image.width > 0 && image.height > 0 {
                    return image::RgbaImage::from_raw(
                        image.width as u32,
                        image.height as u32,
                        image.bytes.into_owned(),
                    )
                    .ok_or_else(|| "Could not decode the selected screen image.".into());
                }
            }
        }
    }
}

fn run_ocr(image_path: PathBuf) {
    println!("Running OCR...");

    let mut ocr_text = None;
    if let Ok(mut ocr) = OcrEngine::new() {
        if let Ok(text) = ocr.extract_text(&image_path) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                println!("\n========== SCREENSHOT OCR ==========\n{trimmed}\n====================================");
                ocr_text = Some(trimmed.to_string());
            }
        }
    }

    create_bookmark(image_path, ocr_text);
}

fn create_bookmark(
    image_path: PathBuf,
    ocr_text: Option<String>,
) {
    println!("Creating Moneta bookmark...");

    let timestamp = current_timestamp();
    let id = format!("image-{timestamp}");

    let title = if let Some(ref text) = ocr_text {
        let first_line = text.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            first_line.chars().take(60).collect()
        } else {
            format!("Screenshot {timestamp}")
        }
    } else {
        format!("Screenshot {timestamp}")
    };

    let mut bookmark = Bookmark::new(
        id,
        image_path.to_string_lossy().to_string(),
        title,
        "image".into(),
    );

    bookmark.ocr_text = ocr_text.clone();
    bookmark.captured_text = ocr_text;
    bookmark.created_at = timestamp;
    bookmark.updated_at = timestamp;

    let vault_dir = if std::path::Path::new("moneta-vault/bookmarks").exists() {
        PathBuf::from("moneta-vault/bookmarks")
    } else if std::path::Path::new("../moneta-vault/bookmarks").exists() {
        PathBuf::from("../moneta-vault/bookmarks")
    } else {
        PathBuf::from("moneta-vault/bookmarks")
    };

    let capture_service = match CaptureService::new(&vault_dir) {
        Ok(capture_service) => capture_service,
        Err(error) => {
            eprintln!("Failed to open Moneta vault: {error}");
            return;
        }
    };

    match capture_service.save_and_enrich(bookmark) {
        Ok(bookmark) => {
            println!("Enriched screenshot bookmark saved: {}", bookmark.id);
        }
        Err(error) => {
            eprintln!("Failed to save bookmark: {error}");
        }
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs() as i64
}