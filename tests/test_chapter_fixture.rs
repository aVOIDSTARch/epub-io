// v0.0.1
//! Offline test that a serialized `ChapterText` fixture (a real chapter, no
//! ebook or TTV service needed) round-trips through serde and exposes clean,
//! narration-ready text. Regenerate with:
//!   cargo run --example dump_chapter -- <ebook> > tests/fixtures/chapter.json

use epub_io::models::{ChapterRole, ChapterText};

const FIXTURE: &str = include_str!("fixtures/chapter.json");

#[test]
fn chapter_fixture_deserializes_clean() {
    let ch: ChapterText = serde_json::from_str(FIXTURE).expect("deserialize chapter fixture");

    assert_eq!(ch.role, ChapterRole::Body, "fixture should be a body chapter");
    assert!(ch.chapter_number > 0, "chapter_number should be set");
    assert!(!ch.title.trim().is_empty(), "title should be set");

    // Narration-ready plain text: non-trivial and free of raw HTML markup.
    let text = ch.text.trim();
    assert!(text.len() > 200, "expected substantial chapter text, got {}", text.len());
    assert!(!text.contains('<'), "text should carry no HTML tags");
    assert!(!text.contains("&nbsp;"), "text should carry no HTML entities");
}
