use super::bookmark::Bookmark;
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

pub struct VaultStorage {
    root: PathBuf,
}

impl VaultStorage {
    pub fn new<P: AsRef<Path>>(root: P) -> io::Result<Self> {
        let root = root.as_ref().to_path_buf();

        fs::create_dir_all(&root)?;

        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save(&self, bookmark: &Bookmark) -> io::Result<()> {
        let path = self.safe_bookmark_path(&bookmark.id)?;

        let metadata =
            serde_yaml::to_string(bookmark)
                .map_err(io::Error::other)?;

        let content = format!(
            "---\n{}---\n",
            metadata
        );

        fs::write(path, content)
    }

    pub fn load(&self, id: &str) -> io::Result<Bookmark> {
        let path = self.safe_bookmark_path(id)?;

        let content = fs::read_to_string(path)?;
        let metadata = extract_frontmatter(&content)?;

        serde_yaml::from_str(metadata)
            .map_err(io::Error::other)
    }

    pub fn update(&self, bookmark: &Bookmark) -> io::Result<()> {
        self.save(bookmark)
    }

    pub fn delete(&self, id: &str) -> io::Result<()> {
        let path = self.safe_bookmark_path(id)?;

        if path.exists() {
            fs::remove_file(path)?;
        }

        Ok(())
    }

    pub fn list_ids(&self) -> io::Result<Vec<String>> {
        let mut ids = Vec::new();
        if !self.root.exists() {
            return Ok(ids);
        }

        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md")
                && let Some(file_stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    ids.push(file_stem.to_string());
                }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn list_all(&self) -> io::Result<Vec<Bookmark>> {
        let ids = self.list_ids()?;
        let mut bookmarks = Vec::new();
        for id in ids {
            match self.load(&id) {
                Ok(bm) => bookmarks.push(bm),
                Err(e) => eprintln!("Warning: failed to load bookmark '{id}': {e}"),
            }
        }
        Ok(bookmarks)
    }

    pub fn bookmark_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.md"))
    }

    pub fn safe_bookmark_path(&self, id: &str) -> io::Result<PathBuf> {
        // Prevent path traversal
        if id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Invalid bookmark ID: path traversal detected",
            ));
        }
        Ok(self.bookmark_path(id))
    }
}

pub fn extract_frontmatter(content: &str) -> io::Result<&str> {
    // Normalize leading prefix check for both \n and \r\n
    let content = if let Some(stripped) = content.strip_prefix("---\r\n") {
        stripped
    } else if let Some(stripped) = content.strip_prefix("---\n") {
        stripped
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing frontmatter",
        ));
    };

    // Find ending delimiter
    if let Some(end) = content.find("\n---") {
        let trimmed_end = if end > 0 && content.as_bytes()[end - 1] == b'\r' {
            end - 1
        } else {
            end
        };
        Ok(&content[..trimmed_end])
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid frontmatter: missing closing delimiter",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_bookmark_storage() {
        let vault_path = PathBuf::from("moneta-vault/bookmarks");

        println!(
            "Current directory: {}",
            std::env::current_dir().unwrap().display()
        );

        println!(
            "Vault path: {}",
            vault_path.canonicalize()
                .unwrap_or_else(|_| vault_path.clone())
                .display()
        );

        let storage = VaultStorage::new(&vault_path).unwrap();

        let mut bookmark = Bookmark::new(
            "test123".into(),
            r"C:\Images\rust.jpg".into(),
            "Rust OCR Tutorial".into(),
            "image".into(),
        );

        storage.save(&bookmark).unwrap();

        let bookmark_path = vault_path.join("test123.md");

        println!(
            "Bookmark created at: {}",
            bookmark_path.canonicalize().unwrap().display()
        );

        assert!(bookmark_path.exists());

        let loaded = storage.load("test123").unwrap();

        assert_eq!(loaded.id, bookmark.id);
        assert_eq!(loaded.path, bookmark.path);
        assert_eq!(loaded.title, bookmark.title);
        assert_eq!(loaded.source_type, bookmark.source_type);
        assert_eq!(loaded.tags, bookmark.tags);

        bookmark.title = "Updated Rust OCR Tutorial".into();
        bookmark.update_timestamp();

        storage.update(&bookmark).unwrap();

        let updated = storage.load("test123").unwrap();

        assert_eq!(
            updated.title,
            "Updated Rust OCR Tutorial"
        );
    }

    #[test]
    fn test_path_traversal_prevention() {
        let storage = VaultStorage::new("moneta-vault/bookmarks").unwrap();
        assert!(storage.safe_bookmark_path("../secret").is_err());
        assert!(storage.safe_bookmark_path("sub/folder").is_err());
        assert!(storage.safe_bookmark_path(r"sub\folder").is_err());
        assert!(storage.safe_bookmark_path("valid-id-123").is_ok());
    }

    #[test]
    fn test_list_all_bookmarks() {
        let storage = VaultStorage::new("moneta-vault/bookmarks").unwrap();
        let list = storage.list_all().unwrap();
        assert!(!list.is_empty());
    }
}