use crate::ai::embeddings::{cosine_similarity, EmbeddingProvider, LocalFeatureEmbeddingProvider};
use crate::vault::bookmark::Bookmark;

pub trait InferenceProvider: Send + Sync {
    fn summarize(&self, text: &str, max_sentences: usize) -> String;
    fn cluster_bookmarks<'a>(&self, bookmarks: &'a [Bookmark], min_similarity: f32) -> Vec<Vec<&'a Bookmark>>;
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
    /// Extractive text summarization selecting the most representative sentences.
    fn summarize(&self, text: &str, max_sentences: usize) -> String {
        let sentences: Vec<&str> = text
            .split(['.', '!', '?'])
            .map(|s| s.trim())
            .filter(|s| s.len() > 15)
            .collect();

        if sentences.is_empty() {
            return text.chars().take(200).collect();
        }

        if sentences.len() <= max_sentences {
            return sentences.join(". ") + ".";
        }

        // Compute overall document embedding
        let doc_emb = self.embedding_provider.generate_embedding(text);

        // Score sentences by cosine similarity to overall document
        let mut scored_sentences: Vec<(&str, f32, usize)> = sentences
            .iter()
            .enumerate()
            .map(|(idx, &s)| {
                let sent_emb = self.embedding_provider.generate_embedding(s);
                let score = cosine_similarity(&doc_emb, &sent_emb);
                (s, score, idx)
            })
            .collect();

        scored_sentences.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Pick top sentences and restore original narrative order
        let mut top_sentences: Vec<(&str, usize)> = scored_sentences
            .into_iter()
            .take(max_sentences)
            .map(|(s, _, idx)| (s, idx))
            .collect();
        top_sentences.sort_by_key(|item| item.1);

        top_sentences
            .into_iter()
            .map(|(s, _)| s)
            .collect::<Vec<&str>>()
            .join(". ")
            + "."
    }

    /// Clusters bookmarks based on cosine similarity of their content.
    fn cluster_bookmarks<'a>(
        &self,
        bookmarks: &'a [Bookmark],
        min_similarity: f32,
    ) -> Vec<Vec<&'a Bookmark>> {
        let mut clusters: Vec<Vec<&'a Bookmark>> = Vec::new();
        let mut embeddings: Vec<Vec<f32>> = Vec::new();

        for bookmark in bookmarks {
            let text = format!(
                "{} {} {}",
                bookmark.title,
                bookmark.captured_text.as_deref().unwrap_or(""),
                bookmark.ocr_text.as_deref().unwrap_or("")
            );
            let emb = self.embedding_provider.generate_embedding(&text);

            let mut matched_cluster = None;
            for (idx, cluster_emb) in embeddings.iter().enumerate() {
                if cosine_similarity(&emb, cluster_emb) >= min_similarity {
                    matched_cluster = Some(idx);
                    break;
                }
            }

            if let Some(idx) = matched_cluster {
                clusters[idx].push(bookmark);
            } else {
                embeddings.push(emb);
                clusters.push(vec![bookmark]);
            }
        }

        clusters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extractive_summarize() {
        let engine = LocalInferenceEngine::new();
        let doc = "Moneta is an offline bookmarking tool. It automatically tags captured records. Cloud APIs are avoided to protect privacy. Rust powers the high performance core. Users can search locally with full text and vector embeddings.";
        let summary = engine.summarize(doc, 2);
        assert!(!summary.is_empty());
        assert!(summary.contains('.'));
    }

    #[test]
    fn test_cluster_bookmarks() {
        let engine = LocalInferenceEngine::new();
        let b1 = Bookmark::new("1".into(), "".into(), "Rust Programming".into(), "web".into());
        let b2 = Bookmark::new("2".into(), "".into(), "Rust Language Compiler".into(), "web".into());
        let b3 = Bookmark::new("3".into(), "".into(), "Italian Cooking Pasta Recipe".into(), "web".into());

        let list = vec![b1, b2, b3];
        let clusters = engine.cluster_bookmarks(&list, 0.4);
        assert!(clusters.len() >= 2);
    }
}
