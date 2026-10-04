use std::sync::{Arc, RwLock};

use crate::ai::analysis::{AIAnalysis, DocumentContext};
use crate::ai::embeddings::{EmbeddingProvider, LocalGgufEmbeddingProvider};
use crate::ai::inference::LocalLlmInference;
use crate::ai::model::ModelConfig;

/// Central embedded offline AI engine for Moneta.
/// Holds loaded quantized models in memory once and serves concurrent inference requests.
#[derive(Clone)]
pub struct LocalAIEngine {
    config: ModelConfig,
    llm: Arc<RwLock<LocalLlmInference>>,
    embeddings: Arc<RwLock<LocalGgufEmbeddingProvider>>,
}

impl Default for LocalAIEngine {
    fn default() -> Self {
        Self::new(ModelConfig::default())
    }
}

impl LocalAIEngine {
    pub fn new(config: ModelConfig) -> Self {
        let llm = LocalLlmInference::new(config.llm_path.clone());
        let embeddings = LocalGgufEmbeddingProvider::new(config.embedding_path.clone());

        Self {
            config,
            llm: Arc::new(RwLock::new(llm)),
            embeddings: Arc::new(RwLock::new(embeddings)),
        }
    }

    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    /// Explicitly loads both local models, reporting progress and errors cleanly.
    pub fn init(&self) -> Result<(), String> {
        println!("Loading local AI model...");
        println!("Model: {}", self.config.llm_path.display());

        let mut llm_guard = self.llm.write().map_err(|e| format!("Lock error: {e}"))?;
        match llm_guard.load() {
            Ok(_) => {
                println!("Local LLM loaded successfully.");
            }
            Err(err) => {
                eprintln!("{err}");
                println!("AI engine will run in deterministic offline fallback mode until model files are installed.");
            }
        }

        println!("Loading embedding model...");
        println!("Model: {}", self.config.embedding_path.display());

        let mut emb_guard = self.embeddings.write().map_err(|e| format!("Lock error: {e}"))?;
        match emb_guard.load() {
            Ok(_) => {
                println!("Local embedding model loaded successfully.");
            }
            Err(err) => {
                eprintln!("{err}");
                println!("Embedding engine will use dense feature embeddings until embedding model is installed.");
            }
        }

        println!("AI engine ready.");
        Ok(())
    }

    pub fn is_llm_ready(&self) -> bool {
        self.llm
            .read()
            .map(|guard| guard.is_loaded())
            .unwrap_or(false)
    }

    pub fn is_embedding_ready(&self) -> bool {
        self.embeddings
            .read()
            .map(|guard| guard.is_loaded())
            .unwrap_or(false)
    }

    /// Analyzes a complete document/file context and produces title, summary, tags, and embedding.
    pub fn analyze_context(
        &self,
        context: &DocumentContext,
        fallback_title: &str,
    ) -> Result<AIAnalysis, String> {
        // 1. Generate Title, Summary, and Semantic Tags
        let (title, summary, tags) = {
            let llm = self.llm.read().map_err(|e| format!("Lock error: {e}"))?;
            llm.analyze_document_context(context, fallback_title)?
        };

        // 2. Format composite representation for semantic embedding
        let text_to_embed = context.format_text_for_embedding(&title, &summary);

        // 3. Generate semantic embedding vector
        let embedding = self.generate_embedding(&text_to_embed);

        Ok(AIAnalysis {
            title,
            summary,
            tags,
            embedding,
        })
    }

    /// Generates a semantic embedding vector from text.
    pub fn generate_embedding(&self, text: &str) -> Vec<f32> {
        if let Ok(emb) = self.embeddings.read() {
            emb.generate_embedding(text)
        } else {
            Vec::new()
        }
    }

    pub fn embedding_dimensions(&self) -> usize {
        if let Ok(emb) = self.embeddings.read() {
            emb.dimensions()
        } else {
            128
        }
    }

    pub fn embedding_model_name(&self) -> String {
        if let Ok(emb) = self.embeddings.read() {
            emb.model_name().to_string()
        } else {
            "moneta-offline-embedding".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_ai_engine_initialization() {
        let engine = LocalAIEngine::default();
        let init_result = engine.init();
        assert!(init_result.is_ok());
    }

    #[test]
    fn test_engine_embedding_generation_and_dimensionality() {
        let engine = LocalAIEngine::default();
        let emb = engine.generate_embedding("Systems programming in Rust with fearless concurrency");
        assert!(!emb.is_empty());
        assert_eq!(emb.len(), engine.embedding_dimensions());
    }

    #[test]
    fn test_engine_context_aware_analysis() {
        let engine = LocalAIEngine::default();
        let mut ctx = DocumentContext::new(
            "Pierre Fatou and Gaston Julia independently arrive at Julia sets",
            "pdf",
        );
        ctx.surrounding_context = Some(
            "Iterative rational maps on Riemann spheres lead to fractal boundaries. Pierre Fatou and Gaston Julia independently arrive at Julia sets in complex dynamics theory.".into()
        );
        ctx.source_path = Some(r"C:\Math\ComplexDynamics.pdf".into());

        let analysis = engine.analyze_context(&ctx, "Complex Dynamics Paper").unwrap();

        assert!(!analysis.title.trim().is_empty());
        assert!(!analysis.summary.trim().is_empty());
        assert!(!analysis.tags.is_empty());
        assert!(!analysis.embedding.is_empty());
        assert_eq!(analysis.embedding.len(), engine.embedding_dimensions());

        // Verify tags do not contain generic junk
        for tag in &analysis.tags {
            assert_ne!(tag, "text");
            assert_ne!(tag, "document");
            assert_ne!(tag, "content");
            assert!(!tag.starts_with('#'));
        }
    }
}
