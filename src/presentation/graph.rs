use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::MetadataService;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub source_type: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub unresolved_links: Vec<GraphEdge>,
}

#[derive(Default)]
pub struct KnowledgeGraph {
    nodes: HashMap<String, GraphNode>,
    title_to_id: HashMap<String, String>,
    outgoing_edges: HashMap<String, Vec<String>>,
    incoming_edges: HashMap<String, Vec<String>>,
    raw_links: Vec<(String, String)>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a bookmark into the graph and extracts wiki-links.
    pub fn add_bookmark(&mut self, bookmark: &Bookmark) {
        let node = GraphNode {
            id: bookmark.id.clone(),
            title: bookmark.title.clone(),
            source_type: bookmark.source_type.clone(),
            tags: bookmark.tags.clone(),
        };

        self.title_to_id
            .insert(bookmark.title.to_lowercase(), bookmark.id.clone());
        self.nodes.insert(bookmark.id.clone(), node);

        // Extract wiki links [[...]]
        let content = format!(
            "{} {}",
            bookmark.captured_text.as_deref().unwrap_or(""),
            bookmark.ocr_text.as_deref().unwrap_or("")
        );
        let targets = MetadataService::extract_wiki_links(&content);

        for target in targets {
            self.raw_links.push((bookmark.id.clone(), target));
        }
    }

    /// Resolves links between bookmarks, determining resolved vs unresolved links.
    pub fn build_graph(&mut self) -> GraphData {
        self.outgoing_edges.clear();
        self.incoming_edges.clear();

        let mut resolved_edges = Vec::new();
        let mut unresolved_edges = Vec::new();

        for (source_id, target_ref) in &self.raw_links {
            let target_lower = target_ref.to_lowercase();

            // Match by ID or by Title
            let resolved_target_id = if self.nodes.contains_key(target_ref) {
                Some(target_ref.clone())
            } else {
                self.title_to_id.get(&target_lower).cloned()
            };

            if let Some(target_id) = resolved_target_id {
                self.outgoing_edges
                    .entry(source_id.clone())
                    .or_default()
                    .push(target_id.clone());
                self.incoming_edges
                    .entry(target_id.clone())
                    .or_default()
                    .push(source_id.clone());

                resolved_edges.push(GraphEdge {
                    source: source_id.clone(),
                    target: target_id,
                });
            } else {
                unresolved_edges.push(GraphEdge {
                    source: source_id.clone(),
                    target: target_ref.clone(),
                });
            }
        }

        GraphData {
            nodes: self.nodes.values().cloned().collect(),
            edges: resolved_edges,
            unresolved_links: unresolved_edges,
        }
    }

    /// Gets backlinks (incoming links) for a bookmark ID or title.
    pub fn get_backlinks(&self, id_or_title: &str) -> Vec<String> {
        let id = self
            .title_to_id
            .get(&id_or_title.to_lowercase())
            .map(|s| s.as_str())
            .unwrap_or(id_or_title);

        self.incoming_edges
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    /// Gets immediate neighbors (connected nodes) for a bookmark.
    pub fn get_neighbors(&self, id: &str) -> Vec<String> {
        let mut neighbors = HashSet::new();

        if let Some(outgoing) = self.outgoing_edges.get(id) {
            for target in outgoing {
                neighbors.insert(target.clone());
            }
        }

        if let Some(incoming) = self.incoming_edges.get(id) {
            for source in incoming {
                neighbors.insert(source.clone());
            }
        }

        neighbors.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_knowledge_graph_resolution_and_backlinks() {
        let mut graph = KnowledgeGraph::new();

        let mut b1 = Bookmark::new("bm1".into(), "".into(), "Rust Intro".into(), "web".into());
        b1.captured_text = Some("Mentions [[Rust Advanced]] and [[Future Topic]].".into());

        let b2 = Bookmark::new("bm2".into(), "".into(), "Rust Advanced".into(), "web".into());

        graph.add_bookmark(&b1);
        graph.add_bookmark(&b2);

        let data = graph.build_graph();
        assert_eq!(data.nodes.len(), 2);
        assert_eq!(data.edges.len(), 1);
        assert_eq!(data.edges[0].source, "bm1");
        assert_eq!(data.edges[0].target, "bm2");

        // Unresolved link to [[Future Topic]]
        assert_eq!(data.unresolved_links.len(), 1);
        assert_eq!(data.unresolved_links[0].target, "Future Topic");

        // Backlinks to Rust Advanced
        let backlinks = graph.get_backlinks("Rust Advanced");
        assert_eq!(backlinks, vec!["bm1"]);

        // Neighbors of bm1
        let neighbors = graph.get_neighbors("bm1");
        assert!(neighbors.contains(&"bm2".to_string()));
    }
}
