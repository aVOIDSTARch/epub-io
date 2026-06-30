// v0.0.1
use anyhow::{Context, Result, bail};
use ebook::formats::{AzwHandler, CbzHandler, EpubHandler, Fb2Handler, MobiHandler, PdfHandler, TxtHandler};
use ebook::traits::{EbookReader, ImageData, TocEntry};
use ebook::utils::detect_format;
use std::path::Path;
use tracing::debug;

use crate::models::{BookMetadata, Chapter, ChapterText};
use crate::pipeline::tts;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct ReadResult {
    pub metadata: BookMetadata,
    pub chapters: Vec<Chapter>,
    pub images: Vec<ImageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterExtraction {
    pub chapter: String,
    pub chapter_number: usize,
    pub title: String,
    pub page_range: Option<String>,
    pub text: String,
}

pub fn extract_chapters(read_result: &ReadResult) -> serde_json::Value {
    let chapters: Vec<ChapterExtraction> = read_result
        .chapters
        .iter()
        .enumerate()
        .map(|(i, chapter)| ChapterExtraction {
            chapter: chapter.filename.clone(),
            chapter_number: i + 1,
            title: chapter.title.clone(),
            page_range: extract_page_range(&chapter.content),
            text: tts::html_to_plain_text(&chapter.content),
        })
        .collect();

    json!({ "chapters": chapters })
}

/// Extract each chapter into a standalone [`ChapterText`] object containing the
/// plain text (markup stripped) plus the book-level metadata. This is the
/// post-pipeline representation handed to the TTV synthesis stage.
pub fn extract_chapter_texts(read_result: &ReadResult) -> Vec<ChapterText> {
    let meta = &read_result.metadata;
    read_result
        .chapters
        .iter()
        .enumerate()
        .map(|(i, chapter)| ChapterText {
            chapter_number: i + 1,
            title: chapter.title.clone(),
            filename: chapter.filename.clone(),
            page_range: extract_page_range(&chapter.content),
            text: tts::html_to_plain_text(&chapter.content),
            book_title: meta.title.clone(),
            author: meta.author.clone(),
            language: meta.language.clone(),
            isbn: meta.isbn.clone(),
            publisher: meta.publisher.clone(),
            publication_date: meta.publication_date.clone(),
        })
        .collect()
}

fn extract_page_range(text: &str) -> Option<String> {
    let marker = "--- Page ";
    let mut pages = Vec::new();
    let mut remainder = text;

    while let Some(idx) = remainder.find(marker) {
        let start = idx + marker.len();
        remainder = &remainder[start..];
        if let Some(end_idx) = remainder.find(" ---") {
            let raw_num = remainder[..end_idx].trim();
            if let Ok(page) = raw_num.parse::<u32>() {
                pages.push(page);
            }
            remainder = &remainder[end_idx + 4..];
        } else {
            break;
        }
    }

    if pages.is_empty() {
        None
    } else if pages.len() == 1 {
        Some(pages[0].to_string())
    } else {
        Some(format!("{}-{}", pages.first().unwrap(), pages.last().unwrap()))
    }
}

pub fn read_ebook(path: &Path) -> Result<ReadResult> {
    let fmt = detect_format(path).with_context(|| format!("cannot detect format of {path:?}"))?;
    debug!("detected format: {fmt}");

    match fmt.as_str() {
        "epub" => {
            // The `ebook` crate's EPUB handler misses self-closing manifest/spine
            // tags, so it returns empty content for most EPUBs. Use it for
            // metadata and images (which it extracts independently of the spine),
            // but read real per-chapter content directly from the archive.
            let mut result = read_with_handler(EpubHandler::new(), path)?;
            match crate::pipeline::epub_reader::read_epub_chapters(path) {
                Ok(chapters) if !chapters.is_empty() => result.chapters = chapters,
                Ok(_) => {}
                Err(e) => debug!("direct epub chapter read failed, keeping handler chapters: {e}"),
            }
            Ok(result)
        }
        "mobi" => read_with_handler(MobiHandler::new(), path),
        "fb2" => read_with_handler(Fb2Handler::new(), path),
        "cbz" => read_with_handler(CbzHandler::new(), path),
        "txt" => read_with_handler(TxtHandler::new(), path),
        "pdf" => read_with_handler(PdfHandler::new(), path),
        "azw" => read_with_handler(AzwHandler::new(), path),
        other => bail!("unsupported format: {other}"),
    }
}

fn read_with_handler<H: EbookReader>(mut handler: H, path: &Path) -> Result<ReadResult> {
    handler.read_from_file(path).with_context(|| format!("failed to read {path:?}"))?;

    let meta = handler.get_metadata().context("failed to extract metadata")?;
    let toc = handler.get_toc().unwrap_or_default();
    let raw_content = handler.get_content().unwrap_or_default();
    let images = handler.extract_images().unwrap_or_default();

    let metadata = BookMetadata {
        title: meta.title,
        author: meta.author,
        publisher: meta.publisher,
        description: meta.description,
        language: meta.language,
        isbn: meta.isbn,
        publication_date: meta.publication_date,
        cover_image: meta.cover_image,
        cover_mime: None,
        tags: meta.tags.unwrap_or_default(),
    };

    println!("ISBN: {:?}", metadata.isbn);

    let chapters = if toc.is_empty() {
        // No TOC: treat the whole content as a single chapter
        vec![Chapter {
            title: metadata.title.clone().unwrap_or_else(|| "Content".to_string()),
            content: raw_content,
            filename: "chapter_001.xhtml".to_string(),
        }]
    } else {
        build_chapters_from_toc(toc, &raw_content)
    };

    Ok(ReadResult { metadata, chapters, images })
}

fn build_chapters_from_toc(toc: Vec<TocEntry>, full_content: &str) -> Vec<Chapter> {
    // The `ebook` crate exposes only the whole-book content (all spine
    // documents concatenated), not per-chapter content. Each spine document is
    // a standalone XHTML file beginning with an `<html` root element, so we
    // split the concatenated content back into per-document segments. The TOC
    // and the concatenated content are produced in the same spine order, so
    // segment `i` corresponds to TOC entry `i`.
    let segments = split_content_by_documents(full_content);

    toc.into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let filename = entry
                .href
                .clone()
                .map(|h| sanitize_href(&h))
                .unwrap_or_else(|| format!("chapter_{:03}.xhtml", i + 1));

            // Fall back to the full content only if we could not split it
            // (e.g. formats whose content is not `<html>`-delimited).
            let content = segments
                .get(i)
                .cloned()
                .unwrap_or_else(|| if segments.is_empty() { full_content.to_string() } else { String::new() });

            Chapter {
                content,
                title: entry.title,
                filename,
            }
        })
        .collect()
}

/// Split concatenated spine content into one segment per document, using each
/// document's `<html` root element as the boundary. Returns a single segment
/// containing the whole input when no `<html` boundary is found.
fn split_content_by_documents(full_content: &str) -> Vec<String> {
    let starts: Vec<usize> = full_content.match_indices("<html").map(|(i, _)| i).collect();

    if starts.is_empty() {
        return vec![full_content.to_string()];
    }

    starts
        .iter()
        .enumerate()
        .map(|(k, &start)| {
            let end = starts.get(k + 1).copied().unwrap_or(full_content.len());
            full_content[start..end].to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn splits_concatenated_documents() {
        let content = "<html>a</html>\n<html>b</html>\n<html>c</html>";
        let segments = split_content_by_documents(content);
        assert_eq!(segments.len(), 3);
        assert!(segments[0].contains("a"));
        assert!(segments[1].contains("b"));
        assert!(segments[2].contains("c"));
    }

    #[test]
    fn no_boundary_returns_single_segment() {
        let segments = split_content_by_documents("plain text only");
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn chapters_have_distinct_content_and_plain_text() {
        let epub_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "TheLostHistoryofLiberalism(Rosenblatt,Helena)(z-library.sk,1lib.sk,z-lib.sk).epub",
        );
        if !epub_path.exists() {
            eprintln!("sample EPUB missing; skipping");
            return;
        }

        let read_result = read_ebook(&epub_path).expect("read epub");
        assert!(read_result.chapters.len() > 2, "expected multiple chapters");

        // The splitting bug assigned the whole book to every chapter; ensure
        // adjacent chapters now differ.
        let first = &read_result.chapters[1].content;
        let second = &read_result.chapters[2].content;
        assert_ne!(first, second, "chapters should hold distinct content");

        let texts = extract_chapter_texts(&read_result);
        assert_eq!(texts.len(), read_result.chapters.len());
        let with_text = texts.iter().filter(|c| !c.text.trim().is_empty()).count();
        assert!(with_text > 2, "expected several non-empty plain-text chapters");
        // Plain text must not contain markup.
        assert!(
            texts.iter().all(|c| !c.text.contains('<')),
            "plain text should not contain markup"
        );
    }
}

fn sanitize_href(href: &str) -> String {
    let base = href.split('#').next().unwrap_or(href);
    let stem = std::path::Path::new(base)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(base);
    if stem.ends_with(".xhtml") || stem.ends_with(".html") || stem.ends_with(".htm") {
        stem.replace(".html", ".xhtml").replace(".htm", ".xhtml")
    } else {
        format!("{stem}.xhtml")
    }
}
