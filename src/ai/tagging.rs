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

        let (generated_tags, source, confidence) =
            match self.engine.analyze_context(&ctx, &bookmark.title) {
                Ok(analysis) => (analysis.tags, TagSource::Ai, Some(0.95)),
                Err(_) => (
                    self.fallback
                        .suggest_tags(bookmark)
                        .into_iter()
                        .map(|tag| tag.name)
                        .collect(),
                    TagSource::Ai,
                    Some(0.75),
                ),
            };
        ensure_contextual_tags(
            bookmark,
            generated_tags,
            source,
            confidence,
            &self.fallback,
        )
    }
}

fn ensure_contextual_tags(
    bookmark: &Bookmark,
    generated_tags: Vec<String>,
    source: TagSource,
    confidence: Option<f64>,
    fallback: &RuleBasedTaggingProvider,
) -> Vec<TagInfo> {
    let mut names = Vec::new();
    let title_tags = crate::ai::analysis::sanitize_tags(&[bookmark.title.clone()]);
    if let Some(title_tag) = title_tags.first() {
        push_contextual_tag(&mut names, title_tag);
    }
    for title_word in bookmark.title.split(|ch: char| !ch.is_alphanumeric()) {
        if title_word.chars().count() >= 3 {
            push_contextual_tag(&mut names, title_word);
            if names.len() >= 2 {
                break;
            }
        }
    }

    let normalized_source_type = bookmark.source_type.to_lowercase();
    let content_types: &[&str] = match normalized_source_type.as_str() {
        "epub" | "book" => &["book", "ebook"],
        "web" | "webpage" => &["webpage", "web-reference"],
        "image" | "screenshot" => &["image", "screen-capture"],
        "pdf" => &["pdf", "reference-document"],
        "word" | "doc" | "docx" | "file" => &["document", "reference-document"],
        "note" | "text" | "clipboard" => &["note", "quick-note"],
        "audio" => &["audio", "recording"],
        "record" | "recording" => &["record", "audio"],
        "video" => &["video", "video-reference"],
        other => &[other],
    };
    for content_type in content_types {
        push_contextual_tag(&mut names, content_type);
    }

    if names.len() < 8 {
        let title_type_tag = format!(
            "{}-{}",
            MetadataService::normalize_tag(&bookmark.title),
            content_types[0]
        );
        push_contextual_tag(&mut names, &title_type_tag);
    }

    for tag in &generated_tags {
        for sanitized in crate::ai::analysis::sanitize_tags(&[tag.clone()]) {
            push_contextual_tag(&mut names, &sanitized);
        }
    }

    if names.len() < 4 {
        for tag in fallback.suggest_tags(bookmark) {
            push_contextual_tag(&mut names, &tag.name);
        }
    }
    for tag in ["knowledge-capture", "personal-reference", "topic-index"] {
        if names.len() >= 4 {
            break;
        }
        push_contextual_tag(&mut names, tag);
    }

    names
        .into_iter()
        .map(|name| TagInfo {
            name,
            source: source.clone(),
            confidence,
        })
        .collect()
}

fn push_contextual_tag(names: &mut Vec<String>, value: &str) {
    if names.len() >= 8 {
        return;
    }
    let normalized = MetadataService::normalize_tag(value);
    if !(2..=40).contains(&normalized.len()) || names.contains(&normalized) {
        return;
    }
    if crate::ai::analysis::sanitize_tags(&[normalized.clone()]).is_empty() {
        return;
    }
    names.push(normalized);
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

    #[test]
    fn contextual_tags_include_title_and_content_type_and_stay_within_range() {
        let provider = RuleBasedTaggingProvider::new();
        let mut bookmark = Bookmark::new(
            "id".into(),
            "path".into(),
            "Rust Concurrency".into(),
            "epub".into(),
        );
        bookmark.captured_text =
            Some("Threads mutexes channels ownership and safe parallel execution".into());

        let tags = ensure_contextual_tags(
            &bookmark,
            vec!["parallel-programming".into()],
            TagSource::Ai,
            Some(0.95),
            &provider,
        );
        let names: Vec<_> = tags.iter().map(|tag| tag.name.as_str()).collect();

        assert!((4..=8).contains(&tags.len()));
        assert!(names.contains(&"rust-concurrency"));
        assert!(names.contains(&"book"));
    }

    #[test]
    fn sparse_bookmarks_still_receive_four_tags() {
        let provider = RuleBasedTaggingProvider::new();
        let bookmark = Bookmark::new("id".into(), "path".into(), "AI".into(), "".into());
        let tags = ensure_contextual_tags(
            &bookmark,
            Vec::new(),
            TagSource::Ai,
            Some(0.75),
            &provider,
        );

        assert_eq!(tags.len(), 4);
        assert!(tags.iter().any(|tag| tag.name == "ai"));
    }
}