use leptess::LepTess;
use std::path::Path;

pub struct OcrEngine {
    tess: LepTess,
}

impl OcrEngine {
    pub fn new() -> Result<Self, String> {
        let tessdata = r"C:\vcpkg\installed\x64-windows-static-md\share\tessdata";

        let tess = LepTess::new(Some(tessdata), "eng")
            .map_err(|e| format!("Failed to initialize Tesseract: {e:?}"))?;
        Ok(Self { tess })
    }

    pub fn extract_text<P: AsRef<Path>>(
        &mut self,
        image_path: P,
    ) -> Result<String, String> {
        self.tess
            .set_image(image_path.as_ref())
            .map_err(|e| format!("Failed to load image: {e:?}"))?;

        self.tess
            .get_utf8_text()
            .map_err(|e| format!("OCR failed: {e:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr() {
        let mut ocr = OcrEngine::new().unwrap();

        let text = ocr
            .extract_text("test.jpg")
            .unwrap();

        println!("\nOCR RESULT:\n{}", text);

        assert!(!text.trim().is_empty());
    }
}

#[cfg(target_os = "windows")]
#[link(name = "xmllite")]
unsafe extern "system" {}

#[cfg(target_os = "windows")]
#[link(name = "iphlpapi")]
unsafe extern "system" {}

#[cfg(target_os = "windows")]
#[link(name = "crypt32")]
unsafe extern "system" {}

#[cfg(target_os = "windows")]
#[link(name = "secur32")]
unsafe extern "system" {}

#[cfg(target_os = "windows")]
#[link(name = "advapi32")]
unsafe extern "system" {}