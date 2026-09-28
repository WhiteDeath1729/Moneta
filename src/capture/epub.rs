use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use zip::ZipArchive;

pub struct EpubChapter {
    pub name: String,
    pub html: String,
}

pub fn extract_chapters<P: AsRef<Path>>(
    path: P,
) -> io::Result<Vec<EpubChapter>> {

    let file = File::open(path)?;

    let mut archive =
        ZipArchive::new(file)
            .map_err(io::Error::other)?;

    let mut chapters = Vec::new();

    for i in 0..archive.len() {

        let mut file =
            archive.by_index(i)
                .map_err(io::Error::other)?;

        let name = file.name().to_string();

        let is_xhtml =
            name.ends_with(".xhtml") ||
            name.ends_with(".html") ||
            name.ends_with(".htm");

        if !is_xhtml {
            continue;
        }

        let mut html = String::new();

        file.read_to_string(&mut html)
            .map_err(io::Error::other)?;

        chapters.push(EpubChapter {
            name,
            html,
        });
    }

    Ok(chapters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epub_extraction() {

        let chapters =
            extract_chapters("test.epub")
                .unwrap();

        println!(
            "Found {} chapters",
            chapters.len()
        );

        for chapter in chapters {
            println!(
                "{} -> {} characters",
                chapter.name,
                chapter.html.len()
            );
        }
    }
}