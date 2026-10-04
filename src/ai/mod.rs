pub mod analysis;
pub mod embeddings;
pub mod engine;
pub mod inference;
pub mod model;
pub mod tagging;

use std::sync::Arc;

pub use analysis::{AIAnalysis, DocumentContext};
pub use embeddings::{
    cosine_similarity, deserialize_vector, load_embedding_binary, save_embedding_binary,
    serialize_vector, EmbeddingProvider, LocalFeatureEmbeddingProvider,
    LocalGgufEmbeddingProvider,
};
pub use engine::LocalAIEngine;
pub use inference::{InferenceProvider, LocalInferenceEngine, LocalLlmInference};
pub use model::{find_gguf_in_dir, ModelConfig};
pub use tagging::{LocalAiTaggingProvider, RuleBasedTaggingProvider, TaggingProvider};

use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::TagInfo;

/// High-level offline AI service for Moneta, wrapping the embedded LocalAIEngine.
pub struct OfflineAIService {
    engine: Arc<LocalAIEngine>,
    tagging_provider: Arc<dyn TaggingProvider>,
    inference_provider: Arc<dyn InferenceProvider>,
    model_name: String,
}

impl Default for OfflineAIService {
    fn default() -> Self {
        let engine = Arc::new(LocalAIEngine::default());
        let tagging_provider = Arc::new(LocalAiTaggingProvider::new(engine.clone()));
        let inference_provider = Arc::new(LocalInferenceEngine::new());
        let model_name = engine.embedding_model_name();

        Self {
            engine,
            tagging_provider,
            inference_provider,
            model_name,
        }
    }
}

impl OfflineAIService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_engine(engine: Arc<LocalAIEngine>) -> Self {
        let tagging_provider = Arc::new(LocalAiTaggingProvider::new(engine.clone()));
        let inference_provider = Arc::new(LocalInferenceEngine::new());
        let model_name = engine.embedding_model_name();

        Self {
            engine,
            tagging_provider,
            inference_provider,
            model_name,
        }
    }

    pub fn engine(&self) -> &Arc<LocalAIEngine> {
        &self.engine
    }

    pub fn suggest_tags(&self, bookmark: &Bookmark) -> Vec<TagInfo> {
        self.tagging_provider.suggest_tags(bookmark)
    }

    pub fn generate_embedding(&self, text: &str) -> Vec<f32> {
        self.engine.generate_embedding(text)
    }

    pub fn embedding_model_name(&self) -> &str {
        &self.model_name
    }

    pub fn summarize(&self, text: &str, max_sentences: usize) -> String {
        self.inference_provider.summarize(text, max_sentences)
    }

    pub fn cluster_bookmarks<'a>(
        &self,
        bookmarks: &'a [Bookmark],
        min_similarity: f32,
    ) -> Vec<Vec<&'a Bookmark>> {
        self.inference_provider.cluster_bookmarks(bookmarks, min_similarity)
    }
}