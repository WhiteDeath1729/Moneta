use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::{MetadataService, TagInfo, TagSource};

pub trait TaggingProvider: Send + Sync {
    fn suggest_tags(&self, bookmark: &Bookmark) -> Vec<TagInfo>;
}

/// Configuration for connecting to an AI model API endpoint
/// (e.g., OpenAI, Ollama, LM Studio, vLLM, Groq, or Anthropic-compatible).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiModelConfig {
    pub api_url: String,
    pub model_name: String,
    pub api_key: Option<String>,
    pub temperature: f32,
    pub max_tokens: usize,
    pub timeout_secs: u64,
}

impl Default for AiModelConfig {
    fn default() -> Self {
        let api_url = std::env::var("MONETA_AI_API_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".to_string());
        let model_name = std::env::var("MONETA_AI_MODEL")
            .unwrap_or_else(|_| "llama3".to_string());
        let api_key = std::env::var("MONETA_AI_API_KEY")
            .ok()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok());

        Self {
            api_url,
            model_name,
            api_key,
            temperature: 0.2,
            max_tokens: 120,
            timeout_secs: 5,
        }
    }
}

/// AI model API tagging provider that prompts an LLM via HTTP.
/// Includes automatic fallback to RuleBasedTaggingProvider when offline.
#[derive(Clone)]
pub struct AiModelTaggingProvider {
    config: AiModelConfig,
    client: reqwest::Client,
    fallback: Option<RuleBasedTaggingProvider>,
}

impl AiModelTaggingProvider {
    pub fn new(config: AiModelConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_default();

        Self {
            config,
            client,
            fallback: Some(RuleBasedTaggingProvider::new()),
        }
    }

    pub fn from_env() -> Self {
        Self::new(AiModelConfig::default())
    }

    pub fn with_fallback(mut self, fallback: RuleBasedTaggingProvider) -> Self {
        self.fallback = Some(fallback);
        self
    }

    pub fn without_fallback(mut self) -> Self {
        self.fallback = None;
        self
    }

    pub fn config(&self) -> &AiModelConfig {
        &self.config
    }

    /// Generates structured prompt input from a bookmark's metadata and content.
    pub fn generate_prompt(&self, bookmark: &Bookmark) -> String {
        let mut parts = Vec::new();
        parts.push(format!("Title: {}", bookmark.title));

        if let Some(ref url) = bookmark.source_url {
            parts.push(format!("URL: {}", url));
        }

        if let Some(ref text) = bookmark.captured_text {
            let snippet: String = text.chars().take(1000).collect();
            parts.push(format!("Content: {}", snippet));
        }

        if let Some(ref ocr) = bookmark.ocr_text {
            let snippet: String = ocr.chars().take(600).collect();
            parts.push(format!("OCR Text: {}", snippet));
        }

        parts.join("\n")
    }

    /// Asynchronously requests tag suggestions from the configured AI model API.
    pub async fn suggest_tags_async(&self, bookmark: &Bookmark) -> Result<Vec<TagInfo>, String> {
        let prompt = self.generate_prompt(bookmark);

        let system_message = "You are an AI tagging engine for a knowledge vault. Given the title, URL, and content of a bookmark, generate between 3 and 6 relevant, descriptive tags. Return ONLY a valid JSON array of strings in lowercase, for example: [\"rust\", \"concurrency\", \"tokio\"]. Do not provide explanations or Markdown codeblocks.";

        let payload = serde_json::json!({
            "model": self.config.model_name,
            "messages": [
                {
                    "role": "system",
                    "content": system_message
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
        });

        let mut req = self
            .client
            .post(&self.config.api_url)
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(self.config.timeout_secs));

        if let Some(ref key) = self.config.api_key {
            if !key.trim().is_empty() {
                req = req.header("Authorization", format!("Bearer {key}"));
            }
        }

        let res = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("HTTP request error: {e}"))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(format!("AI model API error ({status}): {body}"));
        }

        let json_val: serde_json::Value = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {e}"))?;

        // 1. Try standard OpenAI/Ollama choices[0].message.content
        if let Some(content) = json_val
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
        {
            let tags = self.parse_tags_response(content);
            if !tags.is_empty() {
                return Ok(tags);
            }
        }

        // 2. Try Ollama direct response format
        if let Some(content) = json_val.get("response").and_then(|v| v.as_str()) {
            let tags = self.parse_tags_response(content);
            if !tags.is_empty() {
                return Ok(tags);
            }
        }

        // 3. Try Anthropic messages format
        if let Some(content) = json_val.pointer("/content/0/text").and_then(|v| v.as_str()) {
            let tags = self.parse_tags_response(content);
            if !tags.is_empty() {
                return Ok(tags);
            }
        }

        Err("No recognizable content field in AI API response".to_string())
    }

    /// Parses AI output into normalized TagInfo structs, supporting JSON arrays,
    /// markdown-fenced code blocks, and comma-separated text.
    pub fn parse_tags_response(&self, content: &str) -> Vec<TagInfo> {
        let trimmed = content.trim();

        // Strip markdown code fences if model returned ```json ... ```
        let clean = if let Some(stripped) = trimmed.strip_prefix("```json") {
            stripped.strip_suffix("```").unwrap_or(stripped).trim()
        } else if let Some(stripped) = trimmed.strip_prefix("```") {
            stripped.strip_suffix("```").unwrap_or(stripped).trim()
        } else {
            trimmed
        };

        // 1. Try parsing directly as JSON array of strings
        if let Ok(raw_tags) = serde_json::from_str::<Vec<String>>(clean) {
            return self.normalize_and_dedup_tags(raw_tags);
        }

        // 2. Try extracting [ ... ] substring if there was text around the JSON
        if let Some(start) = clean.find('[') {
            if let Some(end) = clean.rfind(']') {
                if end > start {
                    let slice = &clean[start..=end];
                    if let Ok(raw_tags) = serde_json::from_str::<Vec<String>>(slice) {
                        return self.normalize_and_dedup_tags(raw_tags);
                    }
                }
            }
        }

        // 3. Fallback: split on commas or newlines
        let raw_tags: Vec<String> = clean
            .split([',', '\n'])
            .map(|s| {
                s.trim_matches(|c: char| {
                    c == '"' || c == '\'' || c == '-' || c == '*' || c == '`' || c.is_whitespace()
                })
                .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();

        self.normalize_and_dedup_tags(raw_tags)
    }

    fn normalize_and_dedup_tags(&self, raw_tags: Vec<String>) -> Vec<TagInfo> {
        let mut seen = HashSet::new();
        let mut tags = Vec::new();

        for raw in raw_tags {
            let normalized = MetadataService::normalize_tag(&raw);
            if !normalized.is_empty() && normalized.len() >= 2 && seen.insert(normalized.clone()) {
                tags.push(TagInfo {
                    name: normalized,
                    source: TagSource::Ai,
                    confidence: Some(0.95),
                });
            }
        }

        tags
    }

    /// Synchronous wrapper with fallback to RuleBasedTaggingProvider.
    pub fn suggest_tags_blocking(&self, bookmark: &Bookmark) -> Vec<TagInfo> {
        let provider = self.clone();
        let bm = bookmark.clone();

        let res = if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match rt {
                    Ok(runtime) => runtime.block_on(provider.suggest_tags_async(&bm)),
                    Err(e) => Err(format!("Runtime error: {e}")),
                }
            })
            .join()
            .unwrap_or_else(|_| Err("Thread error".to_string()))
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match rt {
                Ok(runtime) => runtime.block_on(provider.suggest_tags_async(&bm)),
                Err(e) => Err(format!("Runtime error: {e}")),
            }
        };

        match res {
            Ok(tags) if !tags.is_empty() => tags,
            _ => {
                if let Some(ref fallback) = self.fallback {
                    fallback.suggest_tags(bookmark)
                } else {
                    Vec::new()
                }
            }
        }
    }
}

impl TaggingProvider for AiModelTaggingProvider {
    fn suggest_tags(&self, bookmark: &Bookmark) -> Vec<TagInfo> {
        self.suggest_tags_blocking(bookmark)
    }
}

#[derive(Clone)]
pub struct RuleBasedTaggingProvider {
    stopwords: HashSet<&'static str>,
}

impl Default for RuleBasedTaggingProvider {
    fn default() -> Self {
        let stopwords: HashSet<&'static str> = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of",
            "with", "by", "from", "up", "about", "into", "over", "after", "is", "are",
            "was", "were", "be", "been", "being", "have", "has", "had", "do", "does",
            "did", "will", "would", "shall", "should", "may", "might", "must", "can",
            "could", "that", "which", "who", "whom", "this", "these", "those", "it",
            "its", "they", "them", "their", "we", "us", "our", "you", "your", "he",
            "him", "his", "she", "her", "http", "https", "com", "org", "net", "www",
            "html", "index", "php", "asp",
        ]
        .iter()
        .copied()
        .collect();

        Self { stopwords }
    }
}

impl RuleBasedTaggingProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TaggingProvider for RuleBasedTaggingProvider {
    fn suggest_tags(&self, bookmark: &Bookmark) -> Vec<TagInfo> {
        let mut word_scores: HashMap<String, f64> = HashMap::new();

        // 1. Process title with higher weight (weight: 3.0)
        self.accumulate_words(&bookmark.title, 3.0, &mut word_scores);

        // 2. Process URL path components (weight: 2.0)
        if let Some(url) = &bookmark.source_url
            && let Ok(parsed) = url::Url::parse(url) {
                let path_segments: Vec<&str> = parsed.path_segments().map(|s| s.collect()).unwrap_or_default();
                for seg in path_segments {
                    self.accumulate_words(seg, 2.0, &mut word_scores);
                }
            }

        // 3. Process captured text (weight: 1.0)
        if let Some(text) = &bookmark.captured_text {
            self.accumulate_words(text, 1.0, &mut word_scores);
        }

        // 4. Process OCR text (weight: 1.5)
        if let Some(ocr) = &bookmark.ocr_text {
            self.accumulate_words(ocr, 1.5, &mut word_scores);
        }

        if word_scores.is_empty() {
            return Vec::new();
        }

        // Find maximum score to normalize confidence into [0.5, 0.99]
        let max_score = word_scores.values().cloned().fold(1.0, f64::max);

        let mut sorted_words: Vec<(String, f64)> = word_scores.into_iter().collect();
        sorted_words.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top 5 candidates
        sorted_words
            .into_iter()
            .take(5)
            .map(|(word, score)| {
                let confidence = (0.50 + 0.49 * (score / max_score)).min(0.99);
                TagInfo {
                    name: word,
                    source: TagSource::Ai,
                    confidence: Some((confidence * 100.0).round() / 100.0),
                }
            })
            .collect()
    }
}

impl RuleBasedTaggingProvider {
    fn accumulate_words(
        &self,
        text: &str,
        weight: f64,
        scores: &mut HashMap<String, f64>,
    ) {
        for raw_token in text.split(|c: char| !c.is_alphanumeric() && c != '-') {
            let token = raw_token.trim().to_lowercase();
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
    use axum::routing::post;
    use axum::{Json, Router};

    #[test]
    fn test_rule_based_tagging() {
        let provider = RuleBasedTaggingProvider::new();

        let mut bookmark = Bookmark::new(
            "b1".into(),
            "Academic".into(),
            "Rust Concurrency in Practice".into(),
            "web".into(),
        );
        bookmark.captured_text = Some("Fearless concurrency with threads and channels in Rust programming language.".into());

        let suggestions = provider.suggest_tags(&bookmark);
        assert!(!suggestions.is_empty());

        let tag_names: Vec<String> = suggestions.iter().map(|t| t.name.clone()).collect();
        assert!(tag_names.contains(&"rust".to_string()) || tag_names.contains(&"concurrency".to_string()));

        for tag in suggestions {
            assert_eq!(tag.source, TagSource::Ai);
            assert!(tag.confidence.unwrap() >= 0.50);
        }
    }

    #[test]
    fn test_ai_model_tagging_parse_json() {
        let provider = AiModelTaggingProvider::new(AiModelConfig::default());
        let json_input = r#"["rust", "async-await", "tokio", "concurrency"]"#;
        let tags = provider.parse_tags_response(json_input);

        assert_eq!(tags.len(), 4);
        assert_eq!(tags[0].name, "rust");
        assert_eq!(tags[1].name, "async-await");
        assert_eq!(tags[0].source, TagSource::Ai);
    }

    #[test]
    fn test_ai_model_tagging_parse_markdown_json() {
        let provider = AiModelTaggingProvider::new(AiModelConfig::default());
        let markdown_input = "```json\n[\"machine-learning\", \"deep-learning\", \"neural-nets\"]\n```";
        let tags = provider.parse_tags_response(markdown_input);

        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].name, "machine-learning");
        assert_eq!(tags[1].name, "deep-learning");
    }

    #[test]
    fn test_ai_model_tagging_parse_comma_separated() {
        let provider = AiModelTaggingProvider::new(AiModelConfig::default());
        let text_input = "Rust, Systems Programming, Multi-threading";
        let tags = provider.parse_tags_response(text_input);

        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].name, "rust");
        assert_eq!(tags[1].name, "systems-programming");
        assert_eq!(tags[2].name, "multi-threading");
    }

    #[test]
    fn test_ai_model_prompt_generation() {
        let provider = AiModelTaggingProvider::new(AiModelConfig::default());
        let mut bookmark = Bookmark::new("b-test".into(), "test.md".into(), "Distributed Systems".into(), "web".into());
        bookmark.source_url = Some("https://example.com/dist".into());
        bookmark.captured_text = Some("Raft and Paxos consensus algorithms.".into());

        let prompt = provider.generate_prompt(&bookmark);
        assert!(prompt.contains("Distributed Systems"));
        assert!(prompt.contains("https://example.com/dist"));
        assert!(prompt.contains("Raft and Paxos"));
    }

    #[test]
    fn test_ai_model_fallback_when_offline() {
        // Point to an invalid unreachable port
        let mut config = AiModelConfig::default();
        config.api_url = "http://127.0.0.1:59999/v1/chat/completions".to_string();
        config.timeout_secs = 1;

        let provider = AiModelTaggingProvider::new(config);

        let mut bookmark = Bookmark::new("b-fall".into(), "f.md".into(), "Rust Memory Safety".into(), "web".into());
        bookmark.captured_text = Some("Borrow checker and ownership in Rust systems.".into());

        // Should fall back to RuleBasedTaggingProvider without panicking
        let tags = provider.suggest_tags(&bookmark);
        assert!(!tags.is_empty());
        let names: Vec<String> = tags.iter().map(|t| t.name.clone()).collect();
        assert!(names.contains(&"rust".to_string()) || names.contains(&"safety".to_string()));
    }

    #[tokio::test]
    async fn test_ai_model_mock_http_server() {
        // Spawn a mock OpenAI/Ollama compatible server on a dynamic port
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                Json(serde_json::json!({
                    "choices": [
                        {
                            "message": {
                                "role": "assistant",
                                "content": "[\"rust-lang\", \"tokio-runtime\", \"high-performance\"]"
                            }
                        }
                    ]
                }))
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut config = AiModelConfig::default();
        config.api_url = format!("http://127.0.0.1:{port}/v1/chat/completions");

        let provider = AiModelTaggingProvider::new(config);
        let bookmark = Bookmark::new("test".into(), "t.md".into(), "High Performance Rust".into(), "web".into());

        let tags = provider.suggest_tags_async(&bookmark).await.unwrap();
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].name, "rust-lang");
        assert_eq!(tags[1].name, "tokio-runtime");
        assert_eq!(tags[2].name, "high-performance");
        assert_eq!(tags[0].source, TagSource::Ai);
    }
}

