// v0.0.1
//! Direct EPUB spine reader.
//!
//! The `ebook` crate (0.1.2) only handles `Event::Start` when parsing the OPF
//! `<manifest>`/`<spine>`, so EPUBs that use self-closing `<item/>`/`<itemref/>`
//! tags (the vast majority) yield empty content. This module reads the spine
//! directly from the ZIP container so each chapter gets its real, distinct text.

use anyhow::{bail, Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::models::Chapter;

/// Read the EPUB at `path` and return one [`Chapter`] per spine document, in
/// reading order, with real per-document content.
pub fn read_epub_chapters(path: &Path) -> Result<Vec<Chapter>> {
    let file = std::fs::File::open(path).with_context(|| format!("failed to open {path:?}"))?;
    let mut archive = ZipArchive::new(file).context("failed to open EPUB as zip archive")?;

    let opf_path = find_opf_path(&mut archive)?;
    let opf = read_zip_text(&mut archive, &opf_path)?;
    let opf_dir = parent_dir(&opf_path);

    let (manifest, spine, ncx_id) = parse_opf(&opf)?;

    // Map chapter file -> human title from the NCX navMap, when available.
    let titles = ncx_id
        .and_then(|id| manifest.get(&id).cloned())
        .map(|href| join_path(&opf_dir, &href))
        .and_then(|ncx_path| read_zip_text(&mut archive, &ncx_path).ok())
        .map(|ncx| parse_ncx_titles(&ncx))
        .unwrap_or_default();

    let mut chapters = Vec::new();
    for (i, idref) in spine.iter().enumerate() {
        let Some(href) = manifest.get(idref) else { continue };
        let full_path = join_path(&opf_dir, href);
        let content = match read_zip_text(&mut archive, &full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let base = basename(href);
        let title = titles
            .get(&base)
            .cloned()
            .or_else(|| extract_title(&content))
            .unwrap_or_else(|| format!("Chapter {}", i + 1));

        let role = crate::pipeline::classify::classify_role(&title, &base);
        chapters.push(Chapter {
            title,
            content,
            filename: ensure_xhtml(&base),
            role,
        });
    }

    Ok(chapters)
}

fn read_zip_text<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>, name: &str) -> Result<String> {
    let mut file = archive
        .by_name(name)
        .with_context(|| format!("missing zip entry {name}"))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .with_context(|| format!("failed to read zip entry {name}"))?;
    Ok(buf)
}

/// Locate the OPF package document via `META-INF/container.xml`.
fn find_opf_path<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> Result<String> {
    let container = read_zip_text(archive, "META-INF/container.xml")
        .context("EPUB missing META-INF/container.xml")?;

    let mut reader = Reader::from_str(&container);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local_name(e.name().as_ref()) == "rootfile" {
                    if let Some(path) = attr(&e, "full-path") {
                        return Ok(path);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("container.xml parse error: {e}"),
            _ => {}
        }
        buf.clear();
    }
    bail!("no rootfile found in container.xml")
}

/// Parse the OPF: returns (manifest id->href, ordered spine idrefs, ncx item id).
fn parse_opf(opf: &str) -> Result<(HashMap<String, String>, Vec<String>, Option<String>)> {
    let mut reader = Reader::from_str(opf);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut manifest: HashMap<String, String> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    let mut ncx_id: Option<String> = None;
    let mut in_manifest = false;
    let mut in_spine = false;

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "manifest" => in_manifest = true,
                    "spine" => in_spine = true,
                    "item" if in_manifest => {
                        if let (Some(id), Some(href)) = (attr(&e, "id"), attr(&e, "href")) {
                            if attr(&e, "media-type").as_deref() == Some("application/x-dtbncx+xml") {
                                ncx_id = Some(id.clone());
                            }
                            manifest.insert(id, href);
                        }
                    }
                    "itemref" if in_spine => {
                        if let Some(idref) = attr(&e, "idref") {
                            spine.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "manifest" {
                    in_manifest = false;
                } else if name == "spine" {
                    in_spine = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("opf parse error: {e}"),
            _ => {}
        }
        buf.clear();
    }

    if spine.is_empty() {
        bail!("OPF spine is empty");
    }
    Ok((manifest, spine, ncx_id))
}

/// Parse the NCX navMap into a map of chapter file basename -> title.
fn parse_ncx_titles(ncx: &str) -> HashMap<String, String> {
    let mut reader = Reader::from_str(ncx);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut map: HashMap<String, String> = HashMap::new();
    let mut in_text = false;
    let mut current_label = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local_name(e.name().as_ref()).as_str() {
                "text" => {
                    in_text = true;
                    current_label.clear();
                }
                "content" => {
                    if let Some(src) = attr(&e, "src") {
                        let base = basename(&src);
                        if !base.is_empty() && !current_label.trim().is_empty() {
                            map.entry(base).or_insert_with(|| current_label.trim().to_string());
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => {
                current_label.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::End(e)) => {
                if local_name(e.name().as_ref()) == "text" {
                    in_text = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

/// Best-effort chapter title from the document's `<title>` element.
fn extract_title(content: &str) -> Option<String> {
    let lower = content.to_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    let title = content[start..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

/// Strip a quick-xml qualified name down to its local part (drops any prefix).
fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

/// Read a named attribute off an element, returning its UTF-8 value.
fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if local_name(a.key.as_ref()) == key {
            Some(String::from_utf8_lossy(&a.value).to_string())
        } else {
            None
        }
    })
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(idx) => path[..idx].to_string(),
        None => String::new(),
    }
}

fn basename(path: &str) -> String {
    let no_frag = path.split('#').next().unwrap_or(path);
    no_frag.rsplit('/').next().unwrap_or(no_frag).to_string()
}

/// Join an OPF-relative href to the OPF directory, resolving `.`/`..` segments.
fn join_path(dir: &str, href: &str) -> String {
    let combined = if dir.is_empty() {
        href.to_string()
    } else {
        format!("{dir}/{href}")
    };

    let mut parts: Vec<&str> = Vec::new();
    for segment in combined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn ensure_xhtml(name: &str) -> String {
    if name.ends_with(".xhtml") {
        name.to_string()
    } else if name.ends_with(".html") || name.ends_with(".htm") {
        name.replace(".html", ".xhtml").replace(".htm", ".xhtml")
    } else {
        format!("{name}.xhtml")
    }
}
