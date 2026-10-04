use crate::ai::analysis::DocumentContext;
use crate::ai::engine::LocalAIEngine;
use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::{
    MetadataService,
    TagInfo,
    TagSource,
};

use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;

pub trait TaggingProvider: Send + Sync {
    fn suggest_tags(
        &self,
        bookmark: &Bookmark,
    ) -> Vec<TagInfo>;
}

/// Tagging provider backed by Moneta's embedded local AI engine.
#[derive(Clone)]
pub struct LocalAiTaggingProvider {
    engine: Arc<LocalAIEngine>,
    fallback: RuleBasedTaggingProvider,
}

impl LocalAiTaggingProvider {
    pub fn new(engine: Arc<LocalAIEngine>) -> Self {
        Self {
            engine,
            fallback: RuleBasedTaggingProvider::new(),
        }
    }
}

impl TaggingProvider for LocalAiTaggingProvider {
    fn suggest_tags(&self, bookmark: &Bookmark) -> Vec<TagInfo> {
        let mut ctx = DocumentContext::new(
            bookmark.captured_text.clone().unwrap_or_default(),
            bookmark.source_type.clone(),
        );
        ctx.ocr_text = bookmark.ocr_text.clone();
        ctx.source_path = Some(bookmark.path.clone());
        ctx.window_title = Some(bookmark.title.clone());

        match self.engine.analyze_context(&ctx, &bookmark.title) {
            Ok(analysis) => {
                if !analysis.tags.is_empty() {
                    return analysis
                        .tags
                        .into_iter()
                        .map(|name| TagInfo {
                            name,
                            source: TagSource::Ai,
                            confidence: Some(0.95),
                        })
                        .collect();
                }
                self.fallback.suggest_tags(bookmark)
            }
            Err(_) => self.fallback.suggest_tags(bookmark),
        }
    }
}

#[derive(Clone)]
pub struct RuleBasedTaggingProvider {
    stopwords: HashSet<&'static str>,
}

impl Default for RuleBasedTaggingProvider {
    fn default() -> Self {
        let words = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of",
            "with", "by", "from", "this", "that", "these", "those", "is", "are", "was",
            "were", "be", "been", "it", "its", "they", "them", "their", "we", "our",
            "you", "your", "http", "https", "www", "text", "document", "file", "content",
        ];

        Self {
            stopwords: words.iter().copied().collect(),
        }
    }
}

impl RuleBasedTaggingProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TaggingProvider for RuleBasedTaggingProvider {
    fn suggest_tags(&self, bookmark: &Bookmark) -> Vec<TagInfo> {
        let mut scores: HashMap<String, f64> = HashMap::new();

        self.accumulate(&bookmark.title, 3.0, &mut scores);

        if let Some(url) = &bookmark.source_url {
            self.accumulate(url, 2.0, &mut scores);
        }

        if let Some(text) = &bookmark.captured_text {
            self.accumulate(text, 1.0, &mut scores);
        }

        if let Some(text) = &bookmark.ocr_text {
            self.accumulate(text, 1.5, &mut scores);
        }

        let max_score = scores.values().copied().fold(1.0, f64::max);

        let mut values: Vec<(String, f64)> = scores.into_iter().collect();

        values.sort_by(|a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });

        values
            .into_iter()
            .take(6)
            .map(|(name, score)| {
                let confidence = 0.50 + 0.49 * (score / max_score);
                TagInfo {
                    name,
                    source: TagSource::Ai,
                    confidence: Some(confidence.min(0.99)),
                }
            })
            .collect()
    }
}

impl RuleBasedTaggingProvider {
    fn accumulate(&self, text: &str, weight: f64, scores: &mut HashMap<String, f64>) {
        for raw in text.split(|c: char| !c.is_alphanumeric() && c != '-') {
            let token = raw.trim().to_lowercase();

            if token.len() < 3 || token.len() > 24 {
                continue;
            }

            if self.stopwords.contains(token.as_str()) {
                continue;
            }

            if token.chars().all(|c| c.is_numeric()) {
                continue;
            }

            let normalized = MetadataService::normalize_tag(&token);

            if !normalized.is_empty() {
                *scores.entry(normalized).or_insert(0.0) += weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_based_tagging() {
        let provider = RuleBasedTaggingProvider::new();
        let mut bm = Bookmark::new("id".into(), "path".into(), "Rust Concurrency".into(), "web".into());
        bm.captured_text = Some("Threads, mutexes, and channels in systems programming.".into());

        let tags = provider.suggest_tags(&bm);
        assert!(!tags.is_empty());
        let names: Vec<String> = tags.into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"rust".to_string()) || names.contains(&"concurrency".to_string()));
    }
}