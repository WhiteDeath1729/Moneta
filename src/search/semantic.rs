use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::ai::embeddings::cosine_similarity;
use crate::ai::OfflineAIService;
use crate::search::indexing::SqliteIndex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub bookmark_id: String,
    pub score: f32,
    pub match_type: String,
}

pub struct SearchEngine {
    index: Arc<SqliteIndex>,
    ai: Arc<OfflineAIService>,
}

impl SearchEngine {
    pub fn new(index: Arc<SqliteIndex>, ai: Arc<OfflineAIService>) -> Self {
        Self { index, ai }
    }

    /// Keyword / Full-Text search across title, content, OCR text, and tags.
    pub fn keyword_search(&self, query: &str) -> Vec<SearchResult> {
        let ids = self.index.query_keyword(query).unwrap_or_default();
        let total = ids.len() as f32;

        ids.into_iter()
            .enumerate()
            .map(|(rank, id)| {
                // Rank-based decay score
                let score = if total > 1.0 {
                    1.0 - (rank as f32 / total) * 0.5
                } else {
                    1.0
                };

                SearchResult {
                    bookmark_id: id,
                    score,
                    match_type: "keyword".to_string(),
                }
            })
            .collect()
    }

    /// Semantic search using local embedding cosine similarity.
    pub fn semantic_search(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let query_emb = self.ai.generate_embedding(query);
        let stored_embeddings = self.index.get_all_embeddings().unwrap_or_default();

        let mut scored: Vec<SearchResult> = stored_embeddings
            .into_iter()
            .map(|(id, emb)| {
                let sim = cosine_similarity(&query_emb, &emb);
                SearchResult {
                    bookmark_id: id,
                    score: sim,
                    match_type: "semantic".to_string(),
                }
            })
            .filter(|r| r.score > 0.05)
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    /// Hybrid search combining keyword and semantic candidates with Reciprocal Rank Fusion.
    pub fn hybrid_search(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let keyword_results = self.keyword_search(query);
        let semantic_results = self.semantic_search(query, top_k * 2);

        let k = 60.0f32; // Standard RRF smoothing constant
        let mut rrf_scores: HashMap<String, f32> = HashMap::new();

        for (rank, res) in keyword_results.iter().enumerate() {
            let score = 1.0 / (k + rank as f32 + 1.0);
            *rrf_scores.entry(res.bookmark_id.clone()).or_insert(0.0) += score * 1.2;
        }

        for (rank, res) in semantic_results.iter().enumerate() {
            let score = 1.0 / (k + rank as f32 + 1.0);
            *rrf_scores.entry(res.bookmark_id.clone()).or_insert(0.0) += score;
        }

        let mut combined: Vec<SearchResult> = rrf_scores
            .into_iter()
            .map(|(id, score)| SearchResult {
                bookmark_id: id,
                score,
                match_type: "hybrid".to_string(),
            })
            .collect();

        combined.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        combined.truncate(top_k);
        combined
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::bookmark::Bookmark;

    #[test]
    fn test_search_engine_keyword_and_semantic_and_hybrid() {
        let index = Arc::new(SqliteIndex::open_in_memory().unwrap());
        let ai = Arc::new(OfflineAIService::new());

        let mut bm1 = Bookmark::new("b1".into(), "b1.md".into(), "Rust Concurrency".into(), "web".into());
        bm1.captured_text = Some("Fearless multi-threading and asynchronous tokio runtime.".into());

        let mut bm2 = Bookmark::new("b2".into(), "b2.md".into(), "Python Data Analysis".into(), "web".into());
        bm2.captured_text = Some("Pandas and NumPy for scientific computing and matrix operations.".into());

        index.index_bookmark(&bm1, Some(&ai)).unwrap();
        index.index_bookmark(&bm2, Some(&ai)).unwrap();

        let engine = SearchEngine::new(index, ai);

        // Keyword search
        let kw_res = engine.keyword_search("asynchronous");
        assert_eq!(kw_res.len(), 1);
        assert_eq!(kw_res[0].bookmark_id, "b1");

        // Semantic search
        let sem_res = engine.semantic_search("Rust parallel threads", 2);
        for r in &sem_res {
            println!("SEM RESULT: id={}, score={}, type={}", r.bookmark_id, r.score, r.match_type);
        }
        assert!(!sem_res.is_empty());
        assert_eq!(sem_res[0].bookmark_id, "b1");

        // Hybrid search
        let hyb_res = engine.hybrid_search("Rust concurrency tokio", 2);
        assert!(!hyb_res.is_empty());
        assert_eq!(hyb_res[0].bookmark_id, "b1");
    }
}
