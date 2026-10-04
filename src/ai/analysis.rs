use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Result of local AI analysis on context-aware bookmark content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIAnalysis {
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub embedding: Vec<f32>,
}

/// Rich source context captured around the user's selection.
#[derive(Debug, Clone, Default)]
pub struct DocumentContext {
    pub selected_text: String,
    pub surrounding_context: Option<String>,
    pub full_document_text: Option<String>,
    pub ocr_text: Option<String>,
    pub source_path: Option<String>,
    pub source_type: String,
    pub window_title: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl DocumentContext {
    pub fn new(selected_text: impl Into<String>, source_type: impl Into<String>) -> Self {
        Self {
            selected_text: selected_text.into(),
            source_type: source_type.into(),
            ..Default::default()
        }
    }

    /// Formats the contextual representation to be embedded.
    /// Combines title, summary, selected text, and relevant context.
    pub fn format_text_for_embedding(&self, title: &str, summary: &str) -> String {
        let mut parts = Vec::new();
        if !title.trim().is_empty() {
            parts.push(format!("Title: {title}"));
        }
        if !summary.trim().is_empty() {
            parts.push(format!("Summary: {summary}"));
        }
        if !self.selected_text.trim().is_empty() {
            parts.push(format!("Selected: {}", self.selected_text.trim()));
        }
        if let Some(surrounding) = &self.surrounding_context {
            if !surrounding.trim().is_empty() {
                parts.push(format!("Context: {}", surrounding.trim()));
            }
        }
        if let Some(ocr) = &self.ocr_text {
            if !ocr.trim().is_empty() {
                parts.push(format!("OCR: {}", ocr.trim()));
            }
        }
        parts.join("\n\n")
    }

    /// Prepares aggregated context for large documents using chunking.
    pub fn build_prompt_context(&self, max_context_chars: usize) -> String {
        let mut sections = Vec::new();

        sections.push(format!(
            "=== BOOKMARKED SELECTION ===\n{}",
            self.selected_text.trim()
        ));

        if let Some(surrounding) = &self.surrounding_context {
            if !surrounding.trim().is_empty() {
                sections.push(format!(
                    "=== LOCAL SURROUNDING CONTEXT ===\n{}",
                    surrounding.trim()
                ));
            }
        }

        if let Some(ocr) = &self.ocr_text {
            if !ocr.trim().is_empty() {
                sections.push(format!(
                    "=== OCR TEXT ===\n{}",
                    ocr.trim()
                ));
            }
        }

        // Handle full document context using chunking & aggregation if needed
        if let Some(doc) = &self.full_document_text {
            let doc_trimmed = doc.trim();
            if !doc_trimmed.is_empty() {
                if doc_trimmed.len() <= max_context_chars {
                    sections.push(format!(
                        "=== COMPLETE SOURCE DOCUMENT ===\n{}",
                        doc_trimmed
                    ));
                } else {
                    let aggregated = aggregate_document_chunks(
                        doc_trimmed,
                        &self.selected_text,
                        max_context_chars,
                    );
                    sections.push(format!(
                        "=== AGGREGATED DOCUMENT CONTEXT ===\n{}",
                        aggregated
                    ));
                }
            }
        }

        sections.push(format!(
            "=== SOURCE METADATA ===\nSource Type: {}\nSource Path: {}\nWindow Title: {}",
            self.source_type,
            self.source_path.as_deref().unwrap_or("none"),
            self.window_title.as_deref().unwrap_or("none")
        ));

        sections.join("\n\n")
    }
}

/// Chunks a large document and aggregates key sections (introduction, focus chunk containing selection, conclusion).
pub fn aggregate_document_chunks(
    doc: &str,
    selected_text: &str,
    max_total_chars: usize,
) -> String {
    let lines: Vec<&str> = doc.lines().collect();
    if lines.is_empty() {
        return doc.chars().take(max_total_chars).collect();
    }

    // Chunk by paragraphs or word windows
    let chunk_size = 1500; // characters per chunk
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for line in lines {
        if current_chunk.len() + line.len() > chunk_size && !current_chunk.is_empty() {
            chunks.push(current_chunk);
            current_chunk = String::new();
        }
        current_chunk.push_str(line);
        current_chunk.push('\n');
    }
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    if chunks.len() <= 2 {
        return doc.chars().take(max_total_chars).collect();
    }

    // Find the chunk containing the selected text
    let mut focus_idx = None;
    let selected_sample: String = selected_text.chars().take(40).collect();

    for (i, chunk) in chunks.iter().enumerate() {
        if chunk.contains(&selected_sample) || (!selected_text.is_empty() && chunk.contains(selected_text)) {
            focus_idx = Some(i);
            break;
        }
    }

    let mut result = Vec::new();

    // 1. Beginning / Intro chunk
    result.push(format!("[Document Beginning]\n{}", chunks[0].trim()));

    // 2. Focus chunk (where selection is located)
    if let Some(idx) = focus_idx {
        if idx != 0 && idx != chunks.len() - 1 {
            result.push(format!("[Selection Focus Section]\n{}", chunks[idx].trim()));
        }
    } else if chunks.len() > 2 {
        let mid = chunks.len() / 2;
        result.push(format!("[Document Mid-section]\n{}", chunks[mid].trim()));
    }

    // 3. Conclusion chunk
    if chunks.len() > 1 {
        result.push(format!("[Document Conclusion]\n{}", chunks[chunks.len() - 1].trim()));
    }

    let combined = result.join("\n\n---\n\n");
    if combined.len() > max_total_chars {
        combined.chars().take(max_total_chars).collect()
    } else {
        combined
    }
}

/// Banned generic words that must not be used as tags.
const GENERIC_TAG_STOPWORDS: &[&str] = &[
    "text",
    "information",
    "document",
    "file",
    "content",
    "important",
    "notes",
    "bookmark",
    "moneta",
    "paragraph",
    "word",
    "sentence",
    "thing",
    "data",
];

/// Sanitizes, normalizes, and filters semantic tags.
pub fn sanitize_tags(raw_tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut clean_tags = Vec::new();

    for tag in raw_tags {
        let trimmed = tag.trim().trim_start_matches('#');
        let mut normalized = String::new();
        let mut last_was_dash = false;

        for ch in trimmed.chars() {
            if ch.is_alphanumeric() {
                normalized.push(ch.to_ascii_lowercase());
                last_was_dash = false;
            } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !normalized.is_empty() && !last_was_dash {
                normalized.push('-');
                last_was_dash = true;
            }
        }

        if normalized.ends_with('-') {
            normalized.pop();
        }

        if normalized.len() < 2 || normalized.len() > 40 {
            continue;
        }

        if GENERIC_TAG_STOPWORDS.contains(&normalized.as_str()) {
            continue;
        }

        if seen.insert(normalized.clone()) {
            clean_tags.push(normalized);
        }

        if clean_tags.len() >= 10 {
            break;
        }
    }

    clean_tags
}

#[derive(Deserialize)]
struct RawAnalysisJson {
    title: Option<String>,
    summary: Option<String>,
    tags: Option<Vec<String>>,
}

/// Parses structured AI output from raw LLM string.
pub fn parse_ai_analysis_response(raw_output: &str, fallback_title: &str) -> (String, String, Vec<String>) {
    let cleaned = raw_output
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // 1. Attempt direct JSON parsing
    if let Ok(parsed) = serde_json::from_str::<RawAnalysisJson>(cleaned) {
        let title = parsed.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| fallback_title.to_string());
        let summary = parsed.summary.unwrap_or_default();
        let tags = sanitize_tags(&parsed.tags.unwrap_or_default());
        return (title, summary, tags);
    }

    // 2. Attempt extracting substring between outer `{` and `}`
    if let (Some(start), Some(end)) = (cleaned.find('{'), cleaned.rfind('}')) {
        if end > start {
            if let Ok(parsed) = serde_json::from_str::<RawAnalysisJson>(&cleaned[start..=end]) {
                let title = parsed.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| fallback_title.to_string());
                let summary = parsed.summary.unwrap_or_default();
                let tags = sanitize_tags(&parsed.tags.unwrap_or_default());
                return (title, summary, tags);
            }
        }
    }

    // 3. Fallback heuristic parsing if JSON formatting failed
    let mut title = fallback_title.to_string();
    let mut summary = String::new();
    let mut tags = Vec::new();

    for line in cleaned.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("title:") {
            title = trimmed["title:".len()..].trim().trim_matches('"').to_string();
        } else if trimmed.to_lowercase().starts_with("summary:") {
            summary = trimmed["summary:".len()..].trim().trim_matches('"').to_string();
        } else if trimmed.to_lowercase().starts_with("tags:") {
            let tag_part = trimmed["tags:".len()..].trim();
            for item in tag_part.split([',', ';', '[', ']']) {
                let clean = item.trim().trim_matches('"');
                if !clean.is_empty() {
                    tags.push(clean.to_string());
                }
            }
        }
    }

    if summary.is_empty() {
        summary = cleaned.chars().take(200).collect();
    }

    (title, summary, sanitize_tags(&tags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_tags() {
        let raw = vec![
            "Text".into(),
            "#Rust-Lang".into(),
            "Julia Sets".into(),
            "complex dynamics".into(),
            "DOCUMENT".into(),
            "rust-lang".into(), // duplicate
            "content".into(),
        ];
        let cleaned = sanitize_tags(&raw);
        assert_eq!(cleaned, vec!["rust-lang", "julia-sets", "complex-dynamics"]);
    }

    #[test]
    fn test_parse_ai_analysis_response_clean_json() {
        let json = r#"
        {
          "title": "Fatou and Julia Sets in Complex Dynamics",
          "summary": "Historical development of complex dynamics through Fatou and Julia sets.",
          "tags": ["julia-sets", "complex-dynamics", "pierre-fatou", "gaston-julia", "mathematics"]
        }
        "#;
        let (title, summary, tags) = parse_ai_analysis_response(json, "Default");
        assert_eq!(title, "Fatou and Julia Sets in Complex Dynamics");
        assert!(summary.contains("Historical development"));
        assert_eq!(tags.len(), 5);
        assert!(tags.contains(&"julia-sets".to_string()));
        assert!(tags.contains(&"complex-dynamics".to_string()));
    }

    #[test]
    fn test_parse_ai_analysis_response_markdown_wrapped() {
        let json = "Here is the analysis:\n```json\n{\n  \"title\": \"Quantum Computing\",\n  \"summary\": \"Qubits and entanglement.\",\n  \"tags\": [\"physics\", \"qubits\", \"quantum-gates\"]\n}\n```";
        let (title, summary, tags) = parse_ai_analysis_response(json, "Fallback");
        assert_eq!(title, "Quantum Computing");
        assert_eq!(summary, "Qubits and entanglement.");
        assert_eq!(tags, vec!["physics", "qubits", "quantum-gates"]);
    }

    #[test]
    fn test_aggregate_document_chunks() {
        let mut large_doc = String::new();
        large_doc.push_str("Introduction to Dynamical Systems and Holomorphic Maps.\n");
        for i in 0..100 {
            large_doc.push_str(&format!("Discussion paragraph {i} covering iteration of rational functions in the Riemann sphere.\n"));
        }
        large_doc.push_str("Pierre Fatou and Gaston Julia independently arrive at Julia sets in the early 20th century.\n");
        for i in 100..200 {
            large_doc.push_str(&format!("Subsequent findings {i} in chaotic dynamics and fractal geometry.\n"));
        }
        large_doc.push_str("Conclusion: The modern theory of Julia and Fatou sets continues to inspire.\n");

        let selected = "Pierre Fatou and Gaston Julia independently arrive at Julia sets";
        let aggregated = aggregate_document_chunks(&large_doc, selected, 4000);

        assert!(aggregated.contains("Introduction"));
        assert!(aggregated.contains("Pierre Fatou and Gaston Julia"));
        assert!(aggregated.contains("Conclusion"));
    }
}
