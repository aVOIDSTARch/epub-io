// v0.0.1
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use id3::{Tag, TagLike, Version};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::borrow::Cow;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;


use crate::models::ChapterText;

/// Endpoint of the local text-to-voice (TTV) service.
const TTV_ENDPOINT: &str = "http://127.0.0.1:3310/ttv";

/// Static table of abbreviation expansions applied in order.
static ABBREVIATIONS: &[(&str, &str)] = &[
    ("e.g.,", "for example,"),
    ("e.g.", "for example"),
    ("i.e.,", "that is,"),
    ("i.e.", "that is"),
    ("etc.", "and so on"),
    ("vs.", "versus"),
    ("approx.", "approximately"),
    ("dept.", "department"),
    ("est.", "established"),
    ("fig.", "figure"),
    ("govt.", "government"),
    ("incl.", "including"),
    ("min.", "minutes"),
    ("misc.", "miscellaneous"),
    ("no.", "number"),
    ("ref.", "reference"),
    ("vol.", "volume"),
    ("Mr.", "Mister"),
    ("Mrs.", "Missus"),
    ("Ms.", "Miss"),
    ("Dr.", "Doctor"),
    ("Prof.", "Professor"),
    ("Sr.", "Senior"),
    ("Jr.", "Junior"),
    ("St.", "Saint"),
    ("Sgt.", "Sergeant"),
    ("Lt.", "Lieutenant"),
    ("Cpl.", "Corporal"),
    ("Pvt.", "Private"),
    ("Gen.", "General"),
    ("Capt.", "Captain"),
    ("Cmdr.", "Commander"),
    ("Col.", "Colonel"),
    ("Maj.", "Major"),
    ("Gov.", "Governor"),
    ("Sen.", "Senator"),
    ("Rep.", "Representative"),
    ("Pres.", "President"),
    ("Sec.", "Secretary"),
    ("Hon.", "Honorable"),
    ("Rev.", "Reverend"),
    ("B.C.", "Before Christ"),
    ("A.D.", "Anno Domini"),
    ("a.m.", "in the morning"),
    ("p.m.", "in the afternoon"),
    ("U.S.", "United States"),
    ("U.K.", "United Kingdom"),
    ("U.N.", "United Nations"),
    ("D.C.", "District of Columbia"),
];

/// Apply TTS cleanup to an HTML chapter string.
/// Returns well-formed XHTML suitable for epub-builder content.
pub fn clean_for_tts(html: &str) -> String {
    // 1. Decode HTML entities
    let decoded = html_escape::decode_html_entities(html);

    // 2. Replace typographic symbols in text nodes
    let decoded = replace_symbols(&decoded);

    // 3. Remove footnote markers: [1], [^1], <sup>1</sup>
    let decoded = remove_footnote_markers(&decoded);

    // 4. Expand abbreviations in text content
    let decoded = expand_abbreviations(&decoded);

    // 5. Normalize whitespace
    normalize_whitespace(&decoded)
}

fn replace_symbols(s: &str) -> String {
    s.replace('\u{2014}', ", ")   // em-dash —
        .replace('\u{2013}', " to ") // en-dash –
        .replace('\u{2026}', ".")    // ellipsis …
        .replace('\u{201C}', "\"")  // left double quote "
        .replace('\u{201D}', "\"")  // right double quote "
        .replace('\u{2018}', "'")   // left single quote '
        .replace('\u{2019}', "'")   // right single quote '
        .replace('\u{00B7}', " ")   // middle dot ·
        .replace('\u{2022}', "")    // bullet •
        .replace('\u{00A0}', " ")   // non-breaking space
}

fn remove_footnote_markers(s: &str) -> String {
    // Remove <sup>...</sup> tags (footnote markers in HTML)
    let s = remove_html_tag_and_content(s, "sup");
    // Remove [n] and [^n] style markers
    remove_bracket_markers(&s)
}

fn remove_html_tag_and_content(s: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(&open) {
        result.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find(&close) {
            rest = &rest[start + end + close.len()..];
        } else {
            break;
        }
    }
    result.push_str(rest);
    result
}

fn remove_bracket_markers(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // Check for [^digits] or [digits]
            let rest = &s[i..];
            let inner_start = 1;
            let mut j = inner_start;
            let inner = rest.as_bytes();
            if j < inner.len() && inner[j] == b'^' {
                j += 1;
            }
            let digit_start = j;
            while j < inner.len() && inner[j].is_ascii_digit() {
                j += 1;
            }
            if j > digit_start && j < inner.len() && inner[j] == b']' {
                // Skip the whole [^123] or [123]
                i += j + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn expand_abbreviations(s: &str) -> String {
    let mut result = Cow::Borrowed(s);
    for (abbrev, expansion) in ABBREVIATIONS {
        if result.contains(abbrev) {
            result = Cow::Owned(result.replace(abbrev, expansion));
        }
    }
    result.into_owned()
}

fn normalize_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_space = false;
    let mut in_tag = false;

    for ch in s.chars() {
        match ch {
            '<' => {
                in_tag = true;
                prev_space = false;
                result.push(ch);
            }
            '>' => {
                in_tag = false;
                prev_space = false;
                result.push(ch);
            }
            ' ' | '\t' if !in_tag => {
                if !prev_space {
                    result.push(' ');
                }
                prev_space = true;
            }
            '\n' | '\r' if !in_tag => {
                if !prev_space {
                    result.push('\n');
                }
                prev_space = true;
            }
            _ => {
                prev_space = false;
                result.push(ch);
            }
        }
    }
    result
}

/// Strip all HTML/XHTML markup from a chapter and return its readable text.
pub fn html_to_plain_text(html: &str) -> String {
    let decoded = html_escape::decode_html_entities(html);
    let stripped = strip_html_tags(&decoded);
    normalize_whitespace(&stripped).trim().to_string()
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut prev_space = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                prev_space = true;
            }
            _ if in_tag => continue,
            c if c.is_whitespace() => {
                if !prev_space {
                    result.push(' ');
                    prev_space = true;
                }
            }
            c => {
                prev_space = false;
                result.push(c);
            }
        }
    }

    result
}

/// Wrap cleaned chapter content in a full EPUB-compatible XHTML shell.
pub fn wrap_xhtml(title: &str, lang: &str, body_content: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"
      xmlns:epub="http://www.idpf.org/2007/ops"
      xml:lang="{lang}" lang="{lang}">
<head>
  <meta charset="UTF-8"/>
  <title>{title}</title>
</head>
<body epub:type="bodymatter chapter">
{body_content}
</body>
</html>"#,
        lang = escape_attr(lang),
        title = escape_attr(title),
        body_content = body_content,
    )
}

pub async fn synthesize_chapter_wav(
    epub_path: &Path,
    chapter_number: usize,
    _chapter_title: &str,
    chapter_text: &str,
    voice_identifier: Option<&str>,
) -> Result<PathBuf> {
    let epub_stem = epub_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("ebook");

    let output_file_name = format!("{epub_stem}-ch{:03}.wav", chapter_number);
    let output_path = epub_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&output_file_name);

    let plain_text = html_to_plain_text(chapter_text);
    let client = Client::new();
    let audio_bytes = ttv_synthesize(&client, &plain_text, voice_identifier).await?;

    let mut file = File::create(&output_path)
        .with_context(|| format!("failed to create output file {:?}", output_path))?;
    file.write_all(&audio_bytes)
        .context("failed to write wav output")?;
    file.flush().context("failed to flush wav output")?;

    Ok(output_path)
}

pub async fn synthesize_chapter_mp3(
    epub_path: &Path,
    chapter_number: usize,
    chapter_title: &str,
    chapter_text: &str,
    voice_identifier: Option<&str>,
) -> Result<PathBuf> {
    let wav_path = synthesize_chapter_wav(
        epub_path,
        chapter_number,
        chapter_title,
        chapter_text,
        voice_identifier,
    )
    .await?;

    let output_file_name = format!(
        "{}-ch{:03}.mp3",
        epub_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("ebook"),
        chapter_number
    );
    let output_path = epub_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&output_file_name);

    let status = Command::new("ffmpeg")
        .args(&[
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            wav_path.to_str().unwrap(),
            "-codec:a",
            "libmp3lame",
            "-qscale:a",
            "2",
            output_path.to_str().unwrap(),
        ])
        .status()
        .context("failed to spawn ffmpeg")?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed to transcode WAV to MP3");
    }

    let epub_stem = epub_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("ebook");

    let mut tag = Tag::new();
    tag.set_title(chapter_title);
    tag.set_album(epub_stem);
    tag.set_artist("epub-io");
    tag.set_track(chapter_number as u32);
    tag.add_frame(id3::frame::Comment {
        lang: "eng".to_string(),
        description: "Generated by".to_string(),
        text: "epub-io".to_string(),
    });
    tag.write_to_path(&output_path, Version::Id3v24)
        .context("failed to write ID3 metadata")?;

    Ok(output_path)
}

/// POST plain text to the local TTV service and return the decoded WAV bytes.
async fn ttv_synthesize(client: &Client, text: &str, voice: Option<&str>) -> Result<Vec<u8>> {
    let request_body = json!({
        "text": text,
        "format": "wav",
        "sample_rate_hz": 24000,
        "voice_identifier": voice,
    });

    let response = client
        .post(TTV_ENDPOINT)
        .json(&request_body)
        .send()
        .await
        .context("failed to send TTV request")?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .context("failed to read TTV response body")?;
    if !status.is_success() {
        anyhow::bail!("TTV request failed: {} - {}", status, body_text);
    }

    let parsed: TtvResponse =
        serde_json::from_str(&body_text).context("failed to parse TTV response JSON")?;

    general_purpose::STANDARD
        .decode(parsed.audio_base64.trim())
        .context("failed to decode base64 audio")
}

/// Append a RIFF `LIST`/`INFO` metadata chunk to a WAV byte buffer.
///
/// `info` is a list of (4-character RIFF INFO tag, value) pairs; empty values
/// and malformed tags are skipped. Returns the input unchanged if it is not a
/// valid `RIFF`/`WAVE` container.
pub fn embed_wav_metadata(wav: &[u8], info: &[(&str, &str)]) -> Vec<u8> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return wav.to_vec();
    }

    // Build the INFO chunk body: the "INFO" form id followed by sub-chunks.
    let mut info_body: Vec<u8> = Vec::new();
    info_body.extend_from_slice(b"INFO");
    for (id, value) in info {
        if value.is_empty() || id.len() != 4 {
            continue;
        }
        let mut data = value.as_bytes().to_vec();
        data.push(0); // null terminator
        let size = data.len() as u32; // size excludes the alignment pad byte
        if data.len() % 2 == 1 {
            data.push(0); // word-align
        }
        info_body.extend_from_slice(id.as_bytes());
        info_body.extend_from_slice(&size.to_le_bytes());
        info_body.extend_from_slice(&data);
    }

    // No usable metadata — leave the file untouched.
    if info_body.len() == 4 {
        return wav.to_vec();
    }

    let mut out = wav.to_vec();
    out.extend_from_slice(b"LIST");
    out.extend_from_slice(&(info_body.len() as u32).to_le_bytes());
    out.extend_from_slice(&info_body);

    // Fix up the top-level RIFF chunk size (= total file size minus 8).
    let riff_size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    out
}

/// Synthesize a single [`ChapterText`] to a WAV file, embedding the chapter and
/// book metadata into the file's RIFF INFO chunk.
pub async fn synthesize_chapter_text_to_wav(
    client: &Client,
    chapter: &ChapterText,
    output_path: &Path,
    voice: Option<&str>,
) -> Result<()> {
    let audio = ttv_synthesize(client, &chapter.text, voice).await?;

    let track = chapter.chapter_number.to_string();
    let info: Vec<(&str, &str)> = vec![
        ("INAM", chapter.title.as_str()),                          // chapter title
        ("IART", chapter.author.as_deref().unwrap_or("")),         // author
        ("IPRD", chapter.book_title.as_deref().unwrap_or("")),     // book (album)
        ("IPRT", track.as_str()),                                  // chapter number
        ("ICRD", chapter.publication_date.as_deref().unwrap_or("")),
        ("ICMT", chapter.isbn.as_deref().unwrap_or("")),           // ISBN as comment
        ("IGNR", "Audiobook"),
        ("ISFT", "epub-io"),
    ];
    let wav = embed_wav_metadata(&audio, &info);

    let mut file = File::create(output_path)
        .with_context(|| format!("failed to create output file {:?}", output_path))?;
    file.write_all(&wav).context("failed to write wav output")?;
    file.flush().context("failed to flush wav output")?;
    Ok(())
}

/// Synthesize every chapter to a WAV file in `out_dir`, named
/// `{file_stem}-ch{NNN}.wav`. Chapters whose text is empty are skipped.
/// Returns the paths of the WAV files written.
pub async fn synthesize_chapters(
    chapters: &[ChapterText],
    out_dir: &Path,
    file_stem: &str,
    voice: Option<&str>,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output directory {:?}", out_dir))?;

    let client = Client::new();
    let mut outputs = Vec::new();

    for chapter in chapters {
        if chapter.text.trim().is_empty() {
            tracing::debug!("skipping empty chapter {}", chapter.chapter_number);
            continue;
        }

        let file_name = format!("{file_stem}-ch{:03}.wav", chapter.chapter_number);
        let output_path = out_dir.join(&file_name);

        synthesize_chapter_text_to_wav(&client, chapter, &output_path, voice)
            .await
            .with_context(|| format!("failed to synthesize chapter {}", chapter.chapter_number))?;

        tracing::info!("wrote {:?}", output_path);
        outputs.push(output_path);
    }

    Ok(outputs)
}

#[derive(Deserialize)]
struct TtvResponse {
    audio_base64: String,
    format: String,
    sample_rate_hz: u32,
    channels: u16,
    byte_count: usize,
    frame_count: usize,
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EpubVersion;
    use std::path::Path;

    #[test]
    fn expands_eg() {
        let out = clean_for_tts("<p>e.g. cats</p>");
        assert!(out.contains("for example"), "got: {out}");
    }

    #[test]
    fn expands_dr() {
        let out = clean_for_tts("<p>Dr. Smith</p>");
        assert!(out.contains("Doctor Smith"), "got: {out}");
    }

    #[test]
    fn replaces_em_dash() {
        let out = clean_for_tts("<p>one\u{2014}two</p>");
        assert!(out.contains("one, two"), "got: {out}");
    }

    #[test]
    fn removes_sup_footnotes() {
        let out = clean_for_tts("<p>text<sup>1</sup> more</p>");
        assert!(!out.contains("<sup>"), "got: {out}");
    }

    #[test]
    fn removes_bracket_markers() {
        let out = clean_for_tts("<p>hello[1] world[^2]</p>");
        assert!(!out.contains("[1]"), "got: {out}");
        assert!(!out.contains("[^2]"), "got: {out}");
    }

    #[tokio::test]
    async fn synthesize_first_chapter_of_root_book() -> anyhow::Result<()> {
        let epub_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "TheLostHistoryofLiberalism(Rosenblatt,Helena)(z-library.sk,1lib.sk,z-lib.sk).epub",
        );
        assert!(epub_path.exists(), "root EPUB not found: {:?}", epub_path);

        let read_result = crate::pipeline::reader::read_ebook(&epub_path)?;
        let output_epub = Path::new(env!("CARGO_MANIFEST_DIR")).join("converted_test.epub");
        let epub_bytes = crate::pipeline::writer::build_epub(
            &read_result.metadata,
            &read_result.chapters,
            &read_result.images,
            EpubVersion::V3,
            true,
        )?;
        std::fs::write(&output_epub, &epub_bytes)?;

        let first_chapter = read_result
            .chapters
            .first()
            .expect("expected at least one chapter");

        let wav_path = synthesize_chapter_wav(
            &output_epub,
            1,
            &first_chapter.title,
            &first_chapter.content,
            None,
        )
        .await?;

        assert!(wav_path.exists(), "WAV file was not created");
        Ok(())
    }
}
