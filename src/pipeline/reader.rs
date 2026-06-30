// v0.0.1
use anyhow::{Context, Result, bail};
use ebook::formats::{AzwHandler, CbzHandler, EpubHandler, Fb2Handler, MobiHandler, PdfHandler, TxtHandler};
use ebook::traits::{EbookReader, ImageData, TocEntry};
use ebook::utils::detect_format;
use std::path::Path;
use tracing::debug;

use crate::models::{BookMetadata, Chapter};

pub struct ReadResult {
    pub metadata: BookMetadata,
    pub chapters: Vec<Chapter>,
    pub images: Vec<ImageData>,
}

pub fn read_ebook(path: &Path) -> Result<ReadResult> {
    let fmt = detect_format(path).with_context(|| format!("cannot detect format of {path:?}"))?;
    debug!("detected format: {fmt}");

    match fmt.as_str() {
        "epub" => read_with_handler(EpubHandler::new(), path),
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
    toc.into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let filename = entry
                .href
                .clone()
                .map(|h| sanitize_href(&h))
                .unwrap_or_else(|| format!("chapter_{:03}.xhtml", i + 1));

            Chapter {
                content: full_content.to_string(),
                title: entry.title,
                filename,
            }
        })
        .collect()
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
