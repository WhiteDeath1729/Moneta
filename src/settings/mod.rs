use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppSettings {
    pub vault_path: PathBuf,
    pub theme: String,
    pub auto_ocr: bool,
    pub auto_tagging: bool,
    pub server_port: u16,
    pub ocr_hotkey: String,
    pub screenshot_hotkey: String,
    pub ai_model_name: String,
    #[serde(default)]
    pub ai_api_url: Option<String>,
    #[serde(default)]
    pub ai_api_key: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            vault_path: PathBuf::from("moneta-vault/bookmarks"),
            theme: "dark".to_string(),
            auto_ocr: true,
            auto_tagging: true,
            server_port: 8765,
            ocr_hotkey: "Ctrl+Shift+O".to_string(),
            screenshot_hotkey: "Ctrl+Shift+S".to_string(),
            ai_model_name: "llama3".to_string(),
            ai_api_url: None,
            ai_api_key: None,
        }
    }
}

pub struct SettingsService {
    settings: AppSettings,
}

impl SettingsService {
    pub fn new(settings: AppSettings) -> Self {
        Self { settings }
    }

    pub fn current(&self) -> &AppSettings {
        &self.settings
    }

    pub fn current_mut(&mut self) -> &mut AppSettings {
        &mut self.settings
    }

    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();
        if path.exists()
            && let Ok(content) = fs::read_to_string(path)
                && let Ok(settings) = serde_json::from_str::<AppSettings>(&content) {
                    return Self::new(settings);
                }
        Self::new(AppSettings::default())
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.settings)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let service = SettingsService::new(AppSettings::default());
        assert_eq!(service.current().server_port, 8765);
        assert_eq!(service.current().theme, "dark");
        assert!(service.current().auto_ocr);
    }

    #[test]
    fn test_save_and_load_settings() {
        let temp_dir = std::env::temp_dir();
        let config_file = temp_dir.join("moneta_test_settings.json");

        let mut service = SettingsService::new(AppSettings::default());
        service.current_mut().server_port = 9000;
        service.current_mut().theme = "light".into();

        service.save_to_file(&config_file).unwrap();

        let loaded = SettingsService::load_or_default(&config_file);
        assert_eq!(loaded.current().server_port, 9000);
        assert_eq!(loaded.current().theme, "light");

        let _ = fs::remove_file(config_file);
    }
}
