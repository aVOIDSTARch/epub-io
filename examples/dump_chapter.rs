//! Extract a single clean chapter from an ebook and serialize it as JSON — a
//! deterministic fixture for tests that need real `ChapterText` input without a
//! live ebook or the TTV service.
//!
//! Usage: `cargo run --example dump_chapter -- <ebook> <out.json> [chapter_number]`
//! Writes the chosen chapter as pretty JSON to `out.json` (writing to a file
//! rather than stdout keeps stray library prints out of the fixture). With no
//! chapter number, picks the first non-empty Body chapter.

use epub_io::models::ChapterRole;
use epub_io::pipeline::reader;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: dump_chapter <ebook> <out.json> [chapter_number]");
    let out = args
        .next()
        .expect("usage: dump_chapter <ebook> <out.json> [chapter_number]");
    let want: Option<usize> = args.next().and_then(|s| s.parse().ok());

    let read_result = reader::read_ebook(std::path::Path::new(&path))?;
    let chapters = reader::extract_chapter_texts(&read_result);

    let chosen = match want {
        Some(n) => chapters.into_iter().find(|c| c.chapter_number == n),
        None => chapters
            .into_iter()
            .find(|c| c.role == ChapterRole::Body && !c.text.trim().is_empty()),
    }
    .ok_or_else(|| anyhow::anyhow!("no matching chapter found"))?;

    std::fs::write(&out, serde_json::to_string_pretty(&chosen)?)?;
    eprintln!("wrote chapter {} ({:?}) to {out}", chosen.chapter_number, chosen.title);
    Ok(())
}
