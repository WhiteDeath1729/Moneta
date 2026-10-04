use moneta::ai::{DocumentContext, LocalAIEngine};
use moneta::vault::bookmark::Bookmark;
use moneta::vault::storage::VaultStorage;

#[test]
fn test_ai_enriched_bookmark_creation_and_persistence() {
    let engine = LocalAIEngine::default();

    // 1. Simulate user highlighting a passage
    let selected_text = "Josiah Claremont, widowed many years before, lived alone, with three colored servants nearly as old as himself, in one of the large pre-Civil War homes that had once been common around Columbia.";
    let full_doc = format!(
        "Historical Records of Richland County.\n\n{}\n\nThese estates were fast disappearing before the coming of the small, independent farmer and the real-estate developer.",
        selected_text
    );

    let mut ctx = DocumentContext::new(selected_text, "file");
    ctx.full_document_text = Some(full_doc);
    ctx.source_path = Some(r"C:\History\Richland.txt".into());
    ctx.window_title = Some("Richland County History".into());

    // 2. Perform AI context-aware analysis
    let analysis = engine.analyze_context(&ctx, "Richland History").expect("Analysis failed");

    assert!(!analysis.title.trim().is_empty());
    assert!(!analysis.summary.trim().is_empty());
    assert!(!analysis.tags.is_empty(), "AI tags should not be empty");
    assert!(!analysis.embedding.is_empty(), "AI embedding should not be empty");

    // 3. Create enriched bookmark
    let temp_vault_dir = std::env::temp_dir().join("moneta_test_ai_vault").join("bookmarks");
    let storage = VaultStorage::new(&temp_vault_dir).expect("Failed to create vault storage");

    let bookmark_id = format!("test-ai-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let mut bookmark = Bookmark::new(
        bookmark_id.clone(),
        r"C:\History\Richland.txt".into(),
        analysis.title.clone(),
        "file".into(),
    );
    bookmark.captured_text = Some(selected_text.to_string());
    bookmark.tags = analysis.tags.clone();
    bookmark.summary = Some(analysis.summary.clone());

    // 4. Save bookmark to Markdown vault
    storage.save(&bookmark).expect("Failed to save bookmark");
    let bookmark_file = storage.bookmark_path(&bookmark_id);
    assert!(bookmark_file.exists(), "Bookmark markdown file should exist on disk");

    // 5. Verify loaded bookmark matches
    let loaded = storage.load(&bookmark_id).expect("Failed to load bookmark");
    assert_eq!(loaded.title, analysis.title);
    assert_eq!(loaded.tags, analysis.tags);
    assert_eq!(loaded.summary, Some(analysis.summary));
    assert_eq!(loaded.captured_text, Some(selected_text.to_string()));

    // 6. Save binary embedding
    let emb_path = storage.save_embedding(&bookmark_id, &analysis.embedding).expect("Failed to save embedding");
    assert!(emb_path.exists(), "Embedding binary file should exist");

    // 7. Verify loaded embedding
    let loaded_emb = storage.load_embedding(&bookmark_id).expect("Failed to load embedding");
    assert_eq!(loaded_emb, analysis.embedding);

    // Clean up
    let _ = std::fs::remove_dir_all(temp_vault_dir.parent().unwrap());
}
