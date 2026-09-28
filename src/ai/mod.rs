pub mod embeddings;
pub mod inference;
pub mod tagging;

use std::sync::Arc;

use self::embeddings::{EmbeddingProvider, LocalFeatureEmbeddingProvider};
use self::inference::{InferenceProvider, LocalInferenceEngine};
use self::tagging::{RuleBasedTaggingProvider, TaggingProvider};
use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::TagInfo;

pub struct OfflineAIService {
    tagging_provider: Arc<dyn TaggingProvider>,
    embedding_provider: Arc<dyn EmbeddingProvider>,
    inference_provider: Arc<dyn InferenceProvider>,
}

impl Default for OfflineAIService {
    fn default() -> Self {
        Self {
            tagging_provider: Arc::new(RuleBasedTaggingProvider::new()),
            embedding_provider: Arc::new(LocalFeatureEmbeddingProvider::default()),
            inference_provider: Arc::new(LocalInferenceEngine::new()),
        }
    }
}

impl OfflineAIService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_with_ai_model(config: tagging::AiModelConfig) -> Self {
        let ai_tagger = tagging::AiModelTaggingProvider::new(config)
            .with_fallback(tagging::RuleBasedTaggingProvider::new());
        Self {
            tagging_provider: Arc::new(ai_tagger),
            embedding_provider: Arc::new(LocalFeatureEmbeddingProvider::default()),
            inference_provider: Arc::new(LocalInferenceEngine::new()),
        }
    }

    pub fn with_tagging_provider(mut self, tagging: Arc<dyn TaggingProvider>) -> Self {
        self.tagging_provider = tagging;
        self
    }

    pub fn with_custom(
        tagging: Arc<dyn TaggingProvider>,
        embedding: Arc<dyn EmbeddingProvider>,
        inference: Arc<dyn InferenceProvider>,
    ) -> Self {
        Self {
            tagging_provider: tagging,
            embedding_provider: embedding,
            inference_provider: inference,
        }
    }

    pub fn suggest_tags(&self, bookmark: &Bookmark) -> Vec<TagInfo> {
        self.tagging_provider.suggest_tags(bookmark)
    }

    pub fn generate_embedding(&self, text: &str) -> Vec<f32> {
        self.embedding_provider.generate_embedding(text)
    }

    pub fn embedding_model_name(&self) -> &str {
        self.embedding_provider.model_name()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offline_ai_service() {
        let ai = OfflineAIService::new();

        let mut bm = Bookmark::new(
            "ai-test".into(),
            "Vault".into(),
            "Introduction to Rust Machine Learning".into(),
            "web".into(),
        );
        bm.captured_text = Some("Machine learning and neural networks in Rust without cloud dependency.".into());

        let tags = ai.suggest_tags(&bm);
        assert!(!tags.is_empty());

        let emb = ai.generate_embedding("Machine learning Rust");
        assert_eq!(emb.len(), 128);

        let summary = ai.summarize("First sentence about rust. Second sentence about machine learning. Third sentence about data.", 1);
        assert!(!summary.is_empty());
    }
}
