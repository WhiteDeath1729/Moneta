use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TagSource {
    User,
    Ai,
}

impl std::fmt::Display for TagSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagSource::User => write!(f, "USER"),
            TagSource::Ai => write!(f, "AI"),
        }
    }
}

impl From<&str> for TagSource {
    fn from(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "AI" => TagSource::Ai,
            _ => TagSource::User,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagInfo {
    pub name: String,
    pub source: TagSource,
    pub confidence: Option<f64>,
}

pub struct MetadataService;

impl MetadataService {
    /// Normalizes a tag string into a standard lowercase, hyphen-separated format.
    pub fn normalize_tag(tag: &str) -> String {
        let trimmed = tag.trim().trim_start_matches('#');
        let mut normalized = String::new();
        let mut last_was_dash = false;

        for ch in trimmed.chars() {
            if ch.is_alphanumeric() {
                normalized.push(ch.to_ascii_lowercase());
                last_was_dash = false;
            } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !normalized.is_empty()
                && !last_was_dash {
                    normalized.push('-');
                    last_was_dash = true;
                }
        }

        // Trim trailing dash if present
        if normalized.ends_with('-') {
            normalized.pop();
        }

        normalized
    }

    /// Extracts Obsidian-style wiki links `[[target]]` or `[[target|alias]]` from text.
    pub fn extract_wiki_links(content: &str) -> Vec<String> {
        let mut links = Vec::new();
        let mut cursor = 0;

        while let Some(start) = content[cursor..].find("[[") {
            let actual_start = cursor + start + 2;
            if let Some(end) = content[actual_start..].find("]]") {
                let link_content = &content[actual_start..actual_start + end];
                let target = link_content.split('|').next().unwrap_or("").trim();
                if !target.is_empty() && !links.contains(&target.to_string()) {
                    links.push(target.to_string());
                }
                cursor = actual_start + end + 2;
            } else {
                break;
            }
        }

        links
    }

    /// Computes a stable content hash (DJB2/Hex) for detecting changes to bookmark records.
    pub fn compute_content_hash(content: &str) -> String {
        let mut hash: u64 = 5381;
        for byte in content.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        format!("{:016x}", hash)
    }

    /// Merges user tags and AI tags without overwriting user tags.
    pub fn merge_tags(user_tags: &[String], ai_tags: &[TagInfo]) -> Vec<String> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // User tags have highest priority
        for tag in user_tags {
            let normalized = Self::normalize_tag(tag);
            if !normalized.is_empty() && seen.insert(normalized.clone()) {
                result.push(normalized);
            }
        }

        // AI suggested tags appended if not already present
        for tag_info in ai_tags {
            let normalized = Self::normalize_tag(&tag_info.name);
            if !normalized.is_empty() && seen.insert(normalized.clone()) {
                result.push(normalized);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tag() {
        assert_eq!(MetadataService::normalize_tag("#Rust-Lang"), "rust-lang");
        assert_eq!(MetadataService::normalize_tag("Deep Learning"), "deep-learning");
        assert_eq!(MetadataService::normalize_tag("  AI_Model  "), "ai-model");
        assert_eq!(MetadataService::normalize_tag("C++"), "c");
    }

    #[test]
    fn test_extract_wiki_links() {
        let text = "Refer to [[Rust Research]] and [[Parallel Computing|MPI]] for details. Also [[Rust Research]].";
        let links = MetadataService::extract_wiki_links(text);
        assert_eq!(links, vec!["Rust Research", "Parallel Computing"]);
    }

    #[test]
    fn test_compute_content_hash() {
        let hash1 = MetadataService::compute_content_hash("hello world");
        let hash2 = MetadataService::compute_content_hash("hello world");
        let hash3 = MetadataService::compute_content_hash("hello world!");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 16);
    }

    #[test]
    fn test_merge_tags() {
        let user_tags = vec!["Rust".into(), "Systems".into()];
        let ai_tags = vec![
            TagInfo {
                name: "rust".into(),
                source: TagSource::Ai,
                confidence: Some(0.95),
            },
            TagInfo {
                name: "performance".into(),
                source: TagSource::Ai,
                confidence: Some(0.85),
            },
        ];

        let merged = MetadataService::merge_tags(&user_tags, &ai_tags);
        assert_eq!(merged, vec!["rust", "systems", "performance"]);
    }
}
