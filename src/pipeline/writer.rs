// v0.0.1
use anyhow::{Context, Result};
use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};
use tracing::debug;

use crate::models::{BookMetadata, Chapter, EpubVersion};
use ebook::traits::ImageData;
use crate::pipeline::tts;

static DEFAULT_CSS: &str = r#"
body {
  font-family: serif;
  line-height: 1.6;
  margin: 1em 2em;
}
h1, h2, h3, h4, h5, h6 {
  font-family: sans-serif;
  line-height: 1.2;
  margin: 1em 0 0.5em;
}
p {
  margin: 0 0 0.8em;
  text-align: left;
}
"#;

pub fn build_epub(
    meta: &BookMetadata,
    chapters: &[Chapter],
    images: &[ImageData],
    epub_version: EpubVersion,
    tts_optimize: bool,
) -> Result<Vec<u8>> {
    let mut builder = EpubBuilder::new(ZipLibrary::new().context("zip library init failed")?)
        .context("epub builder init failed")?;

    builder.epub_version(epub_version.to_builder_version());

    // — Metadata —
    if let Some(title) = &meta.title {
        builder.metadata("title", title).context("set title")?;
    }
    if let Some(author) = &meta.author {
        builder.metadata("author", author).context("set author")?;
    }
    if let Some(lang) = &meta.language {
        builder.metadata("lang", lang).context("set lang")?;
    }
    if let Some(desc) = &meta.description {
        builder.metadata("description", desc).context("set description")?;
    }
    if !meta.tags.is_empty() {
        builder
            .metadata("subject", &meta.tags.join(", "))
            .context("set subject")?;
    }
    if let Some(publisher) = &meta.publisher {
        builder.metadata("generator", publisher).context("set generator")?;
    }

    // — Stylesheet —
    builder.stylesheet(DEFAULT_CSS.as_bytes()).context("set stylesheet")?;

    // — Cover image —
    if let Some(cover_bytes) = &meta.cover_image {
        let mime = meta
            .cover_mime
            .as_deref()
            .unwrap_or("image/jpeg");
        let ext = if mime.contains("png") { "png" } else { "jpg" };
        builder
            .add_cover_image(format!("cover.{ext}"), cover_bytes.as_slice(), mime)
            .context("add cover image")?;
        debug!("added cover image ({} bytes, {mime})", cover_bytes.len());
    }

    // — Inline images from source document —
    for img in images {
        // Skip cover if we already have one from enrichment
        if img.name.contains("cover") && meta.cover_image.is_some() {
            continue;
        }
        let _ = builder.add_resource(&img.name, img.data.as_slice(), &img.mime_type);
    }

    // — Chapters —
    let lang = meta.language.as_deref().unwrap_or("en");
    for (i, chapter) in chapters.iter().enumerate() {
        let content = if tts_optimize {
            tts::clean_for_tts(&chapter.content)
        } else {
            chapter.content.clone()
        };

        let xhtml = tts::wrap_xhtml(&chapter.title, lang, &content);

        let epub_content = EpubContent::new(&chapter.filename, xhtml.as_bytes())
            .title(&chapter.title)
            .reftype(if i == 0 {
                ReferenceType::Text
            } else {
                ReferenceType::Text
            });

        builder.add_content(epub_content).context("add chapter")?;
        debug!("added chapter {i}: {}", chapter.title);
    }

    builder.inline_toc();

    let mut output = Vec::new();
    builder.generate(&mut output).context("generate epub")?;
    Ok(output)
}
