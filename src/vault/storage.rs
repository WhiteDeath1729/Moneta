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

    pub fn save(&self, bookmark: &Bookmark) -> io::Result<()> {
        let path = self.bookmark_path(&bookmark.id);

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
        let path = self.bookmark_path(id);

        let content = fs::read_to_string(path)?;
        let metadata = extract_frontmatter(&content)?;

        serde_yaml::from_str(metadata)
            .map_err(io::Error::other)
    }

    pub fn update(&self, bookmark: &Bookmark) -> io::Result<()> {
        self.save(bookmark)
    }

    pub fn delete(&self, id: &str) -> io::Result<()> {
        let path = self.bookmark_path(id);

        if path.exists() {
            fs::remove_file(path)?;
        }

        Ok(())
    }

    fn bookmark_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.md"))
    }
}

fn extract_frontmatter(content: &str) -> io::Result<&str> {
    let content = content
        .strip_prefix("---\n")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Missing frontmatter",
            )
        })?;

    let end = content
        .find("\n---")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid frontmatter",
            )
        })?;

    Ok(&content[..end])
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

        /*
        bookmark.tags = vec![
            "rust".into(),
            "ocr".into(),
            "programming".into(),
        ];
        */

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
}