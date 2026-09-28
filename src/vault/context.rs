use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Context {
    pub id: i64,
    pub bookmark_id: String,
    pub application: Option<String>,
    pub captured_at: i64,
}

impl Context {
    pub fn new(
        id: i64,
        bookmark_id: String,
        application: Option<String>,
        captured_at: i64,
    ) -> Self {
        Self {
            id,
            bookmark_id,
            application,
            captured_at,
        }
    }
}

pub struct ContextService;

impl ContextService {
    /// Captures the current timestamp in seconds.
    pub fn current_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before UNIX epoch")
            .as_secs() as i64
    }

    /// Captures contextual information for a bookmark.
    /// Provides active application context if available, otherwise safely returns None.
    pub fn capture_context(bookmark_id: &str) -> Context {
        let app = Self::detect_active_application();
        Context::new(
            Self::current_timestamp(),
            bookmark_id.to_string(),
            app,
            Self::current_timestamp(),
        )
    }

    /// Safely attempts to detect the active foreground application.
    /// Fails gracefully to None if not supported or disabled.
    pub fn detect_active_application() -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
            unsafe {
                let hwnd = GetForegroundWindow();
                if hwnd.0.is_null() {
                    return None;
                }
                let mut buffer = [0u16; 512];
                let len = GetWindowTextW(hwnd, &mut buffer);
                if len > 0 {
                    let title = String::from_utf16_lossy(&buffer[..len as usize]);
                    let trimmed = title.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let ctx = Context::new(1, "test-bm-1".into(), Some("Moneta".into()), 1727520000);
        assert_eq!(ctx.id, 1);
        assert_eq!(ctx.bookmark_id, "test-bm-1");
        assert_eq!(ctx.application.as_deref(), Some("Moneta"));
        assert_eq!(ctx.captured_at, 1727520000);
    }

    #[test]
    fn test_context_service_capture() {
        let ctx = ContextService::capture_context("bm-123");
        assert_eq!(ctx.bookmark_id, "bm-123");
        assert!(ctx.captured_at > 0);
    }
}