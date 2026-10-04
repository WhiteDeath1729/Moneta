use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub llm_path: PathBuf,
    pub embedding_path: PathBuf,
    pub temperature: f32,
    pub max_new_tokens: usize,
    pub context_window: usize,
}

impl Default for ModelConfig {
    fn default() -> Self {
        let llm_path = std::env::var("MONETA_LLM_MODEL")
            .or_else(|_| std::env::var("MONETA_LLM_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                find_gguf_in_dir(Path::new("models/llm"))
                    .unwrap_or_else(|| PathBuf::from("models/llm/qwen2.5-0.5b-instruct-q4_k_m.gguf"))
            });

        let embedding_path = std::env::var("MONETA_EMBEDDING_MODEL")
            .or_else(|_| std::env::var("MONETA_EMBEDDING_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                find_gguf_in_dir(Path::new("models/embeddings"))
                    .unwrap_or_else(|| PathBuf::from("models/embeddings/bge-small-en-v1.5-q4_k_m.gguf"))
            });

        Self {
            llm_path,
            embedding_path,
            temperature: 0.3,
            max_new_tokens: 512,
            context_window: 2048,
        }
    }
}

impl ModelConfig {
    pub fn new(llm_path: impl Into<PathBuf>, embedding_path: impl Into<PathBuf>) -> Self {
        Self {
            llm_path: llm_path.into(),
            embedding_path: embedding_path.into(),
            temperature: 0.3,
            max_new_tokens: 512,
            context_window: 2048,
        }
    }

    pub fn validate_llm_path(&self) -> Result<&Path, String> {
        if self.llm_path.exists() && self.llm_path.is_file() {
            Ok(&self.llm_path)
        } else {
            Err(format!(
                "Local AI model not found.\nExpected:\n{}",
                self.llm_path.display()
            ))
        }
    }

    pub fn validate_embedding_path(&self) -> Result<&Path, String> {
        if self.embedding_path.exists() && self.embedding_path.is_file() {
            Ok(&self.embedding_path)
        } else {
            Err(format!(
                "Local embedding model not found.\nExpected:\n{}",
                self.embedding_path.display()
            ))
        }
    }

    pub fn is_llm_available(&self) -> bool {
        self.llm_path.exists() && self.llm_path.is_file()
    }

    pub fn is_embedding_available(&self) -> bool {
        self.embedding_path.exists() && self.embedding_path.is_file()
    }
}

/// Discovers the first `.gguf` file inside a directory.
pub fn find_gguf_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.exists() || !dir.is_dir() {
        return None;
    }

    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext.to_string_lossy().eq_ignore_ascii_case("gguf") {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_config_defaults() {
        let config = ModelConfig::default();
        assert!(!config.llm_path.as_os_str().is_empty());
        assert!(!config.embedding_path.as_os_str().is_empty());
        assert_eq!(config.temperature, 0.3);
        assert_eq!(config.max_new_tokens, 512);
    }

    #[test]
    fn test_model_path_validation_missing() {
        let config = ModelConfig::new("models/llm/nonexistent_model.gguf", "models/embeddings/nonexistent_emb.gguf");
        assert!(!config.is_llm_available());
        assert!(!config.is_embedding_available());

        let llm_err = config.validate_llm_path().unwrap_err();
        assert!(llm_err.contains("Local AI model not found"));
        assert!(llm_err.contains("nonexistent_model.gguf"));

        let emb_err = config.validate_embedding_path().unwrap_err();
        assert!(emb_err.contains("Local embedding model not found"));
        assert!(emb_err.contains("nonexistent_emb.gguf"));
    }
}
