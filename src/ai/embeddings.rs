use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_llama::ModelWeights;
use tokenizers::Tokenizer;

pub trait EmbeddingProvider: Send + Sync {
    fn model_name(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn generate_embedding(&self, text: &str) -> Vec<f32>;
}

/// Local GGUF embedding provider for semantic vector representation.
pub struct LocalGgufEmbeddingProvider {
    model_path: PathBuf,
    model_name: String,
    dimensions: usize,
    model: Option<Mutex<ModelWeights>>,
    tokenizer: Option<Tokenizer>,
    device: Device,
    fallback: LocalFeatureEmbeddingProvider,
}

impl LocalGgufEmbeddingProvider {
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        let path = model_path.into();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("local-gguf-embedding")
            .to_string();

        let dimensions = 384;
        Self {
            model_path: path,
            model_name: name,
            dimensions,
            model: None,
            tokenizer: None,
            device: Device::Cpu,
            fallback: LocalFeatureEmbeddingProvider::new(dimensions),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Loads the GGUF embedding model from disk.
    pub fn load(&mut self) -> Result<(), String> {
        if self.model.is_some() {
            return Ok(());
        }

        if !self.model_path.exists() {
            return Err(format!(
                "Local embedding model not found.\nExpected:\n{}",
                self.model_path.display()
            ));
        }

        let mut file = File::open(&self.model_path).map_err(|e| {
            format!("Failed to open embedding file {}: {e}", self.model_path.display())
        })?;

        let content = gguf_file::Content::read(&mut file).map_err(|e| {
            format!("Failed to read GGUF metadata from {}: {e}", self.model_path.display())
        })?;

        // Detect companion tokenizer
        let companion_tokenizer = self.model_path.with_file_name("tokenizer.json");
        let parent_tokenizer = self.model_path.parent().map(|p| p.join("tokenizer.json"));

        let tokenizer = if companion_tokenizer.exists() {
            Tokenizer::from_file(companion_tokenizer).ok()
        } else if let Some(p) = parent_tokenizer.filter(|p| p.exists()) {
            Tokenizer::from_file(p).ok()
        } else {
            None
        };

        let weights = ModelWeights::from_gguf(content, &mut file, &self.device).map_err(|e| {
            format!("Failed to load quantized embedding model weights: {e}")
        })?;

        self.model = Some(Mutex::new(weights));
        self.tokenizer = tokenizer;

        Ok(())
    }
}

impl EmbeddingProvider for LocalGgufEmbeddingProvider {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn generate_embedding(&self, text: &str) -> Vec<f32> {
        if text.trim().is_empty() {
            return vec![0.0f32; self.dimensions];
        }

        if let (Some(model_mutex), Some(tokenizer)) = (&self.model, &self.tokenizer) {
            let encoding = match tokenizer.encode(text, true) {
                Ok(enc) => enc,
                Err(_) => return self.fallback.generate_embedding(text),
            };

            let tokens = encoding.get_ids();
            if tokens.is_empty() {
                return self.fallback.generate_embedding(text);
            }

            let input = match Tensor::new(tokens, &self.device) {
                Ok(t) => match t.unsqueeze(0) {
                    Ok(u) => u,
                    Err(_) => return self.fallback.generate_embedding(text),
                },
                Err(_) => return self.fallback.generate_embedding(text),
            };

            let mut model = match model_mutex.lock() {
                Ok(m) => m,
                Err(_) => return self.fallback.generate_embedding(text),
            };

            match model.forward(&input, 0) {
                Ok(output) => {
                    // Mean pooling over sequence dimension
                    match output.mean(1) {
                        Ok(pooled) => match pooled.squeeze(0) {
                            Ok(vec_tensor) => match vec_tensor.to_vec1::<f32>() {
                                Ok(mut raw_vec) => {
                                    // Normalize L2
                                    let norm: f32 = raw_vec.iter().map(|v| v * v).sum::<f32>().sqrt();
                                    if norm > 0.0 {
                                        for v in &mut raw_vec {
                                            *v /= norm;
                                        }
                                    }
                                    return raw_vec;
                                }
                                Err(_) => {}
                            },
                            Err(_) => {}
                        },
                        Err(_) => {}
                    }
                }
                Err(_) => {}
            }
        }

        // Clean offline fallback when model is not loaded
        self.fallback.generate_embedding(text)
    }
}

/// Local deterministic feature embedding provider.
///
/// Hashing-based dense subword embedding that runs with zero external dependencies.
#[derive(Debug, Clone)]
pub struct LocalFeatureEmbeddingProvider {
    dimensions: usize,
    model_name: String,
}

impl Default for LocalFeatureEmbeddingProvider {
    fn default() -> Self {
        Self {
            dimensions: 128,
            model_name: "moneta-local-dense-v1".to_string(),
        }
    }
}

impl LocalFeatureEmbeddingProvider {
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            model_name: format!("moneta-local-dense-d{dimensions}"),
        }
    }
}

impl EmbeddingProvider for LocalFeatureEmbeddingProvider {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn generate_embedding(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; self.dimensions];

        let lower = text.to_lowercase();

        let tokens: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|token| !token.trim().is_empty())
            .collect();

        if tokens.is_empty() {
            return vector;
        }

        for token in &tokens {
            let mut h1: u64 = 5381;

            for byte in token.bytes() {
                h1 = h1.wrapping_mul(33).wrapping_add(byte as u64);
            }

            let index = (h1 as usize) % self.dimensions;
            vector[index] += 1.0;

            let chars: Vec<char> = token.chars().collect();

            if chars.len() >= 3 {
                for window in chars.windows(3) {
                    let mut h2: u64 = 17;

                    for character in window {
                        h2 = h2.wrapping_mul(31).wrapping_add(*character as u64);
                    }

                    let index = (h2 as usize) % self.dimensions;
                    vector[index] += 0.3;
                }
            }
        }

        let norm_sq: f32 = vector.iter().map(|value| value * value).sum();

        if norm_sq > 0.0 {
            let norm = norm_sq.sqrt();
            for value in &mut vector {
                *value /= norm;
            }
        }

        vector
    }
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();

    let norm1: f32 = v1.iter().map(|value| value * value).sum::<f32>().sqrt();
    let norm2: f32 = v2.iter().map(|value| value * value).sum::<f32>().sqrt();

    if norm1 > 0.0 && norm2 > 0.0 {
        (dot_product / (norm1 * norm2)).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Serializes an embedding for SQLite storage.
pub fn serialize_vector(vector: &[f32]) -> String {
    serde_json::to_string(vector).unwrap_or_else(|_| "[]".to_string())
}

/// Deserializes an embedding from SQLite storage.
pub fn deserialize_vector(value: &str) -> Result<Vec<f32>, serde_json::Error> {
    serde_json::from_str(value)
}

/// Saves an embedding as a binary f32 vector in the vault embeddings directory.
pub fn save_embedding_binary<P: AsRef<Path>>(path: P, vector: &[f32]) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = File::create(path)?;
    for &val in vector {
        file.write_all(&val.to_le_bytes())?;
    }
    file.flush()?;
    Ok(())
}

/// Loads an embedding from a binary f32 vector file.
pub fn load_embedding_binary<P: AsRef<Path>>(path: P) -> io::Result<Vec<f32>> {
    let path = path.as_ref();
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    if buffer.len() % 4 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid embedding binary length: not a multiple of 4 bytes",
        ));
    }

    let count = buffer.len() / 4;
    let mut vector = Vec::with_capacity(count);
    for chunk in buffer.chunks_exact(4) {
        let bytes: [u8; 4] = chunk.try_into().unwrap();
        vector.push(f32::from_le_bytes(bytes));
    }

    Ok(vector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_feature_embedding_dimensions_and_norm() {
        let provider = LocalFeatureEmbeddingProvider::new(128);
        assert_eq!(provider.dimensions(), 128);

        let emb = provider.generate_embedding("Rust fearless concurrency and systems programming");
        assert_eq!(emb.len(), 128);

        let norm_sq: f32 = emb.iter().map(|v| v * v).sum();
        assert!((norm_sq.sqrt() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_cosine_similarity() {
        let provider = LocalFeatureEmbeddingProvider::new(64);
        let v1 = provider.generate_embedding("Rust memory safety");
        let v2 = provider.generate_embedding("Rust memory safety and concurrency");
        let v3 = provider.generate_embedding("Cooking Italian pasta recipe");

        let sim_close = cosine_similarity(&v1, &v2);
        let sim_far = cosine_similarity(&v1, &v3);

        assert!(sim_close > sim_far, "Similar text should have higher cosine similarity: {sim_close} vs {sim_far}");
    }

    #[test]
    fn test_binary_embedding_persistence() {
        let temp_dir = std::env::temp_dir();
        let bin_path = temp_dir.join("moneta_test_emb.bin");

        let original = vec![0.1f32, -0.4f32, 0.85f32, 1.2f32];
        save_embedding_binary(&bin_path, &original).unwrap();
        assert!(bin_path.exists());

        let loaded = load_embedding_binary(&bin_path).unwrap();
        assert_eq!(original, loaded);

        let _ = std::fs::remove_file(bin_path);
    }
}