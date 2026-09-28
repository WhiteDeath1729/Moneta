pub trait EmbeddingProvider: Send + Sync {
    fn model_name(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn generate_embedding(&self, text: &str) -> Vec<f32>;
}

/// A deterministic local feature extractor producing normalized dense semantic vectors.
/// Operates completely offline with zero network or external dependencies.
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
            .filter(|t| !t.trim().is_empty())
            .collect();

        if tokens.is_empty() {
            return vector;
        }

        for token in &tokens {
            // Word hash
            let mut h1: u64 = 5381;
            for b in token.bytes() {
                h1 = h1.wrapping_mul(33).wrapping_add(b as u64);
            }
            let idx1 = (h1 as usize) % self.dimensions;
            vector[idx1] += 1.0;

            // Character tri-grams for subword morphology
            let chars: Vec<char> = token.chars().collect();
            if chars.len() >= 3 {
                for window in chars.windows(3) {
                    let mut h2: u64 = 17;
                    for &c in window {
                        h2 = h2.wrapping_mul(31).wrapping_add(c as u64);
                    }
                    let idx2 = (h2 as usize) % self.dimensions;
                    vector[idx2] += 0.3;
                }
            }
        }

        // L2 normalization
        let norm_sq: f32 = vector.iter().map(|v| v * v).sum();
        if norm_sq > 0.0 {
            let norm = norm_sq.sqrt();
            for v in vector.iter_mut() {
                *v /= norm;
            }
        }

        vector
    }
}

/// Computes the cosine similarity between two unit-normalized vectors.
pub fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
    let norm1: f32 = v1.iter().map(|v| v * v).sum::<f32>().sqrt();
    let norm2: f32 = v2.iter().map(|v| v * v).sum::<f32>().sqrt();

    if norm1 > 0.0 && norm2 > 0.0 {
        (dot_product / (norm1 * norm2)).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Serializes vector to JSON string for database storage.
pub fn serialize_vector(vector: &[f32]) -> String {
    serde_json::to_string(vector).unwrap_or_else(|_| "[]".to_string())
}

/// Deserializes vector from JSON string.
pub fn deserialize_vector(s: &str) -> Result<Vec<f32>, serde_json::Error> {
    serde_json::from_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_generation_and_dimension() {
        let provider = LocalFeatureEmbeddingProvider::default();
        let emb = provider.generate_embedding("Rust memory safety and concurrency");
        assert_eq!(emb.len(), 128);

        // Vector should be unit normalized
        let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_semantic_similarity() {
        let provider = LocalFeatureEmbeddingProvider::default();
        let v_rust1 = provider.generate_embedding("Rust programming language systems concurrency");
        let v_rust2 = provider.generate_embedding("Rust systems programming and memory safety");
        let v_cooking = provider.generate_embedding("Italian pasta recipe with garlic tomato sauce");

        let sim_rust = cosine_similarity(&v_rust1, &v_rust2);
        let sim_diff = cosine_similarity(&v_rust1, &v_cooking);

        assert!(sim_rust > sim_diff);
        assert!(sim_rust > 0.5);
    }

    #[test]
    fn test_serialization() {
        let v = vec![0.1f32, 0.5, 0.9];
        let serialized = serialize_vector(&v);
        let deserialized = deserialize_vector(&serialized).unwrap();
        assert_eq!(v, deserialized);
    }
}
