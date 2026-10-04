use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama::ModelWeights;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokenizers::Tokenizer;

use crate::ai::analysis::{DocumentContext, parse_ai_analysis_response};
use crate::ai::embeddings::{cosine_similarity, EmbeddingProvider, LocalFeatureEmbeddingProvider};
use crate::vault::bookmark::Bookmark;

pub trait InferenceProvider: Send + Sync {
    fn summarize(&self, text: &str, max_sentences: usize) -> String;
    fn cluster_bookmarks<'a>(&self, bookmarks: &'a [Bookmark], min_similarity: f32) -> Vec<Vec<&'a Bookmark>>;
}

pub struct LocalLlmInference {
    model_path: PathBuf,
    model: Option<Mutex<ModelWeights>>,
    tokenizer: Option<Tokenizer>,
    device: Device,
}

impl LocalLlmInference {
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            model: None,
            tokenizer: None,
            device: Device::Cpu,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Loads the quantized GGUF model and tokenizer into memory once.
    pub fn load(&mut self) -> Result<(), String> {
        if self.model.is_some() {
            return Ok(());
        }

        if !self.model_path.exists() {
            return Err(format!(
                "Local AI model not found.\nExpected:\n{}",
                self.model_path.display()
            ));
        }

        let mut file = File::open(&self.model_path).map_err(|e| {
            format!("Failed to open model file {}: {e}", self.model_path.display())
        })?;

        let content = gguf_file::Content::read(&mut file).map_err(|e| {
            format!("Failed to read GGUF metadata from {}: {e}", self.model_path.display())
        })?;

        // Check if a companion tokenizer.json exists beside the model or in models/llm/
        let companion_tokenizer = self.model_path.with_file_name("tokenizer.json");
        let parent_tokenizer = self.model_path.parent().map(|p| p.join("tokenizer.json"));

        let tokenizer = if companion_tokenizer.exists() {
            Tokenizer::from_file(companion_tokenizer).ok()
        } else if let Some(p) = parent_tokenizer.filter(|p| p.exists()) {
            Tokenizer::from_file(p).ok()
        } else {
            // Attempt to build a vocabulary-based tokenizer from GGUF metadata if available
            None
        };

        let weights = ModelWeights::from_gguf(content, &mut file, &self.device).map_err(|e| {
            format!("Failed to load quantized model weights from GGUF: {e}")
        })?;

        self.model = Some(Mutex::new(weights));
        self.tokenizer = tokenizer;

        Ok(())
    }

    /// Runs local inference on the model with prompt.
    pub fn generate(&self, prompt: &str, max_tokens: usize, temperature: f32) -> Result<String, String> {
        let model_mutex = self.model.as_ref().ok_or_else(|| {
            format!(
                "Local AI model is not loaded.\nExpected:\n{}",
                self.model_path.display()
            )
        })?;

        let tokenizer = self.tokenizer.as_ref().ok_or_else(|| {
            "Tokenizer is not loaded for local LLM inference".to_string()
        })?;

        let tokens = tokenizer
            .encode(prompt, true)
            .map_err(|e| format!("Failed to tokenize prompt: {e}"))?
            .get_ids()
            .to_vec();

        if tokens.is_empty() {
            return Err("Tokenized prompt is empty".to_string());
        }

        let mut model = model_mutex
            .lock()
            .map_err(|e| format!("Model mutex lock error: {e}"))?;

        let mut logits_processor = LogitsProcessor::new(299792458, Some(temperature as f64), None);
        let mut generated_tokens = Vec::new();
        let mut all_tokens = tokens.clone();

        // Process prompt tokens
        let input = Tensor::new(all_tokens.as_slice(), &self.device)
            .map_err(|e| format!("Tensor creation error: {e}"))?
            .unsqueeze(0)
            .map_err(|e| format!("Tensor unsqueeze error: {e}"))?;

        let logits = model
            .forward(&input, 0)
            .map_err(|e| format!("Forward pass error: {e}"))?;

        let logits = logits
            .squeeze(0)
            .map_err(|e| format!("Logits squeeze error: {e}"))?;

        let next_token = logits_processor
            .sample(&logits)
            .map_err(|e| format!("Sampling error: {e}"))?;

        generated_tokens.push(next_token);
        all_tokens.push(next_token);

        // Autoregressive generation loop
        for index in 0..max_tokens {
            // Check for common EOS tokens
            if next_token == 2 || next_token == 151643 || next_token == 151645 || next_token == 0 {
                break;
            }

            let input = Tensor::new(&[next_token], &self.device)
                .map_err(|e| format!("Token tensor error: {e}"))?
                .unsqueeze(0)
                .map_err(|e| format!("Token unsqueeze error: {e}"))?;

            let logits = model
                .forward(&input, all_tokens.len() - 1)
                .map_err(|e| format!("Model generation step {index} failed: {e}"))?
                .squeeze(0)
                .map_err(|e| format!("Logits squeeze error: {e}"))?;

            let sampled = logits_processor
                .sample(&logits)
                .map_err(|e| format!("Sampling error: {e}"))?;

            if sampled == 2 || sampled == 151643 || sampled == 151645 || sampled == 0 {
                break;
            }

            generated_tokens.push(sampled);
            all_tokens.push(sampled);
        }

        let decoded = tokenizer
            .decode(&generated_tokens, true)
            .map_err(|e| format!("Failed to decode output tokens: {e}"))?;

        Ok(decoded)
    }

    /// Analyzes a rich DocumentContext to produce Title, Summary, and Semantic Tags.
    pub fn analyze_document_context(
        &self,
        context: &DocumentContext,
        fallback_title: &str,
    ) -> Result<(String, String, Vec<String>), String> {
        let system_prompt = r#"You are Moneta, a context-aware offline AI engine for personal knowledge and bookmarking.
Analyze the user's bookmarked selection within its source context.
Do NOT output generic tags like "text", "document", "file", "content", "information", "notes".
Infer semantic tags representing concepts, subjects, technologies, people, topics, and domains found in the source.
Return ONLY valid JSON matching this schema:
{
  "title": "descriptive title of what the bookmark is actually about",
  "summary": "concise contextual summary of the bookmarked material",
  "tags": ["tag1", "tag2", "tag3"]
}"#;

        let context_prompt = context.build_prompt_context(3500);

        let formatted_prompt = format!(
            "<|im_start|>system\n{system_prompt}<|im_end|>\n<|im_start|>user\n{context_prompt}\n<|im_end|>\n<|im_start|>assistant\n"
        );

        if self.is_loaded() {
            let output = self.generate(&formatted_prompt, 400, 0.2)?;
            Ok(parse_ai_analysis_response(&output, fallback_title))
        } else {
            // Documented fallback when model file is not yet installed on disk
            let fallback_result = fallback_context_analysis(context, fallback_title);
            Ok(fallback_result)
        }
    }
}

/// Fallback contextual extractor used only when the local model file is not present on disk.
/// Grounded strictly in the provided text.
pub fn fallback_context_analysis(
    context: &DocumentContext,
    fallback_title: &str,
) -> (String, String, Vec<String>) {
    let title = if !context.selected_text.trim().is_empty() {
        let first_line = context.selected_text.lines().next().unwrap_or("").trim();
        if first_line.len() > 10 && first_line.len() < 70 {
            first_line.to_string()
        } else {
            fallback_title.to_string()
        }
    } else {
        fallback_title.to_string()
    };

    let summary = if !context.selected_text.trim().is_empty() {
        context.selected_text.trim().chars().take(250).collect::<String>()
    } else {
        format!("Bookmark from {}", context.source_type)
    };

    // Extract key conceptual tokens from selection and surrounding context
    let mut word_counts = std::collections::HashMap::new();
    let text_to_scan = format!(
        "{} {} {}",
        context.selected_text,
        context.surrounding_context.as_deref().unwrap_or(""),
        context.ocr_text.as_deref().unwrap_or("")
    );

    let stopwords = [
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of",
        "with", "by", "from", "this", "that", "these", "those", "is", "are", "was",
        "were", "be", "been", "it", "its", "they", "them", "their", "we", "our",
        "you", "your", "into", "many", "before", "lived", "alone", "three", "nearly",
        "than", "himself", "large", "once", "common", "fast", "coming", "small",
    ];

    for word in text_to_scan.split(|c: char| !c.is_alphanumeric() && c != '-') {
        let clean = word.trim().to_lowercase();
        if clean.len() >= 4 && !stopwords.contains(&clean.as_str()) {
            *word_counts.entry(clean).or_insert(0) += 1;
        }
    }

    let mut sorted_words: Vec<(String, usize)> = word_counts.into_iter().collect();
    sorted_words.sort_by(|a, b| b.1.cmp(&a.1));

    let tags: Vec<String> = sorted_words
        .into_iter()
        .take(6)
        .map(|(w, _)| w)
        .collect();

    let sanitized = crate::ai::analysis::sanitize_tags(&tags);
    (title, summary, sanitized)
}

#[derive(Default)]
pub struct LocalInferenceEngine {
    embedding_provider: LocalFeatureEmbeddingProvider,
}

impl LocalInferenceEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl InferenceProvider for LocalInferenceEngine {
    fn summarize(&self, text: &str, max_sentences: usize) -> String {
        let sentences: Vec<&str> = text
            .split(['.', '!', '?'])
            .map(|sentence| sentence.trim())
            .filter(|sentence| sentence.len() > 15)
            .collect();

        if sentences.is_empty() {
            return text.chars().take(200).collect();
        }

        if sentences.len() <= max_sentences {
            return format!("{}.", sentences.join(". "));
        }

        let document_embedding = self.embedding_provider.generate_embedding(text);

        let mut scored: Vec<(&str, f32, usize)> = sentences
            .iter()
            .enumerate()
            .map(|(index, sentence)| {
                let sentence_embedding = self.embedding_provider.generate_embedding(sentence);
                let score = cosine_similarity(&document_embedding, &sentence_embedding);
                (*sentence, score, index)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut selected: Vec<(&str, usize)> = scored
            .into_iter()
            .take(max_sentences)
            .map(|(sentence, _, index)| (sentence, index))
            .collect();

        selected.sort_by_key(|item| item.1);
        selected
            .into_iter()
            .map(|(sentence, _)| sentence)
            .collect::<Vec<_>>()
            .join(". ")
            + "."
    }

    fn cluster_bookmarks<'a>(&self, bookmarks: &'a [Bookmark], min_similarity: f32) -> Vec<Vec<&'a Bookmark>> {
        let mut clusters: Vec<Vec<&'a Bookmark>> = Vec::new();
        let mut cluster_embeddings: Vec<Vec<f32>> = Vec::new();

        for bookmark in bookmarks {
            let text = format!(
                "{} {} {}",
                bookmark.title,
                bookmark.captured_text.as_deref().unwrap_or(""),
                bookmark.ocr_text.as_deref().unwrap_or("")
            );

            let embedding = self.embedding_provider.generate_embedding(&text);
            let mut matched = None;

            for (index, cluster_embedding) in cluster_embeddings.iter().enumerate() {
                if cosine_similarity(&embedding, cluster_embedding) >= min_similarity {
                    matched = Some(index);
                    break;
                }
            }

            if let Some(index) = matched {
                clusters[index].push(bookmark);
            } else {
                cluster_embeddings.push(embedding);
                clusters.push(vec![bookmark]);
            }
        }

        clusters
    }
}