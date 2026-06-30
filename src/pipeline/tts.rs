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
use tempfile::NamedTempFile;


use crate::models::{BookMetadata, ChapterRole, ChapterText};

/// A synthesized chapter audio file plus the info needed to build chapter markers.
#[derive(Debug, Clone)]
pub struct ChapterAudio {
    pub chapter_number: usize,
    pub title: String,
    pub path: PathBuf,
}

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

/// Synthesize chapters to WAV files in `out_dir`, named `{file_stem}-ch{NNN}.wav`.
///
/// By default only body chapters are narrated — front matter (cover, copyright,
/// contents, …) and back matter (index, bibliography, notes, …) are skipped, since
/// they are miserable to listen to and waste synthesis. Set `include_all` to
/// narrate every chapter regardless of role. Chapters with empty text are always
/// skipped. Returns the paths of the WAV files written.
pub async fn synthesize_chapters(
    chapters: &[ChapterText],
    out_dir: &Path,
    file_stem: &str,
    voice: Option<&str>,
    include_all: bool,
) -> Result<Vec<ChapterAudio>> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output directory {:?}", out_dir))?;

    let client = Client::new();
    let mut outputs = Vec::new();

    for chapter in chapters {
        if !include_all && chapter.role != ChapterRole::Body {
            tracing::info!(
                "skipping {:?} chapter {}: {}",
                chapter.role,
                chapter.chapter_number,
                chapter.title
            );
            continue;
        }

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
        outputs.push(ChapterAudio {
            chapter_number: chapter.chapter_number,
            title: chapter.title.clone(),
            path: output_path,
        });
    }

    Ok(outputs)
}

// ───────────────────────── M4B audiobook assembly ─────────────────────────

/// Assemble per-chapter audio into a single chaptered, resumable M4B audiobook
/// (AAC in an MP4 container) with chapter markers, cover art, and book metadata.
///
/// Uses ffmpeg: the per-chapter files are concatenated, an ffmetadata file
/// supplies the `[CHAPTER]` markers + global tags, and the cover (if any) is
/// embedded as an attached picture. Returns the path to the written `.m4b`.
pub fn build_m4b(
    meta: &BookMetadata,
    chapters: &[ChapterAudio],
    out_dir: &Path,
    file_stem: &str,
) -> Result<PathBuf> {
    if chapters.is_empty() {
        anyhow::bail!("no chapters to assemble into an audiobook");
    }
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output directory {:?}", out_dir))?;

    // Measure each chapter and compute cumulative start/end times in ms.
    let mut marks: Vec<(String, u64, u64)> = Vec::with_capacity(chapters.len());
    let mut cursor_ms: u64 = 0;
    for ch in chapters {
        let dur = probe_duration_ms(&ch.path)
            .with_context(|| format!("failed to probe duration of {:?}", ch.path))?;
        let start = cursor_ms;
        let end = cursor_ms + dur;
        marks.push((ch.title.clone(), start, end));
        cursor_ms = end;
    }

    // Write the concat list and ffmetadata files alongside the output.
    let concat_path = out_dir.join(format!("{file_stem}.concat.txt"));
    let paths: Vec<PathBuf> = chapters.iter().map(|c| c.path.clone()).collect();
    std::fs::write(&concat_path, build_concat_list(&paths)?)
        .with_context(|| format!("failed to write concat list {:?}", concat_path))?;

    let meta_path = out_dir.join(format!("{file_stem}.ffmeta.txt"));
    let ffmeta = build_ffmetadata(
        meta.title.as_deref(),
        meta.author.as_deref(),
        meta.publication_date.as_deref(),
        meta.isbn.as_deref(),
        &marks,
    );
    std::fs::write(&meta_path, ffmeta)
        .with_context(|| format!("failed to write ffmetadata {:?}", meta_path))?;

    // Cover art (optional): write to a temp file with the right extension.
    let cover_file: Option<NamedTempFile> = match &meta.cover_image {
        Some(bytes) if !bytes.is_empty() => {
            let ext = if meta.cover_mime.as_deref().map(|m| m.contains("png")).unwrap_or(false) {
                ".png"
            } else {
                ".jpg"
            };
            let mut tf = NamedTempFile::with_suffix(ext).context("cover temp file")?;
            tf.write_all(bytes).context("write cover temp")?;
            tf.flush().ok();
            Some(tf)
        }
        _ => None,
    };

    let output_path = out_dir.join(format!("{file_stem}.m4b"));

    // Build the ffmpeg invocation. Inputs: 0=concat audio, 1=ffmetadata,
    // and optionally 2=cover.
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-f", "concat", "-safe", "0", "-i"])
        .arg(&concat_path)
        .args(["-f", "ffmetadata", "-i"])
        .arg(&meta_path);

    if let Some(cover) = &cover_file {
        cmd.arg("-i").arg(cover.path());
    }

    // Map chapters + global metadata from the ffmetadata input (index 1).
    cmd.args(["-map_metadata", "1", "-map_chapters", "1", "-map", "0:a"]);
    if cover_file.is_some() {
        cmd.args(["-map", "2:v", "-c:v", "mjpeg", "-disposition:v", "attached_pic"]);
    }
    cmd.args(["-c:a", "aac", "-b:a", "64k"]).arg(&output_path);

    let status = cmd.status().context("failed to spawn ffmpeg for m4b assembly")?;
    if !status.success() {
        anyhow::bail!("ffmpeg failed to assemble m4b");
    }

    // Clean up the intermediate sidecar files.
    let _ = std::fs::remove_file(&concat_path);
    let _ = std::fs::remove_file(&meta_path);

    Ok(output_path)
}

/// Transcode each per-chapter WAV to MP3 (alongside it) and tag it with ID3
/// metadata. Returns the MP3 paths. The source WAVs are left in place.
pub fn transcode_chapters_to_mp3(
    chapters: &[ChapterAudio],
    out_dir: &Path,
    book_title: Option<&str>,
    author: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let total = chapters.len() as u32;
    let mut outputs = Vec::with_capacity(chapters.len());

    for ch in chapters {
        let mp3_path = out_dir.join(format!(
            "{}.mp3",
            ch.path.file_stem().and_then(|s| s.to_str()).unwrap_or("chapter")
        ));

        let status = Command::new("ffmpeg")
            .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&ch.path)
            .args(["-codec:a", "libmp3lame", "-qscale:a", "2"])
            .arg(&mp3_path)
            .status()
            .context("failed to spawn ffmpeg")?;
        if !status.success() {
            anyhow::bail!("ffmpeg failed to transcode {:?} to mp3", ch.path);
        }

        let mut tag = Tag::new();
        tag.set_title(ch.title.as_str());
        if let Some(t) = book_title {
            tag.set_album(t);
        }
        tag.set_artist(author.unwrap_or("epub-io"));
        tag.set_track(ch.chapter_number as u32);
        tag.set_total_tracks(total);
        tag.write_to_path(&mp3_path, Version::Id3v24)
            .context("failed to write ID3 metadata")?;

        outputs.push(mp3_path);
    }

    Ok(outputs)
}

/// Probe a media file's duration in milliseconds via ffprobe.
fn probe_duration_ms(path: &Path) -> Result<u64> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .context("failed to spawn ffprobe")?;
    if !output.status.success() {
        anyhow::bail!("ffprobe failed for {:?}", path);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let secs: f64 = text.trim().parse().with_context(|| format!("unparsable duration: {text:?}"))?;
    Ok((secs * 1000.0).round() as u64)
}

/// Build an ffmpeg concat-demuxer list file from absolute paths.
fn build_concat_list(paths: &[PathBuf]) -> Result<String> {
    let mut out = String::new();
    for p in paths {
        let abs = std::fs::canonicalize(p)
            .with_context(|| format!("cannot canonicalize {:?}", p))?;
        let s = abs.to_string_lossy().replace('\'', "'\\''");
        out.push_str(&format!("file '{s}'\n"));
    }
    Ok(out)
}

/// Build an ffmetadata document with global tags and `[CHAPTER]` markers.
/// `marks` is a list of (title, start_ms, end_ms) in reading order.
fn build_ffmetadata(
    title: Option<&str>,
    author: Option<&str>,
    date: Option<&str>,
    isbn: Option<&str>,
    marks: &[(String, u64, u64)],
) -> String {
    let mut out = String::from(";FFMETADATA1\n");
    if let Some(t) = title {
        out.push_str(&format!("title={}\n", ffmeta_escape(t)));
        out.push_str(&format!("album={}\n", ffmeta_escape(t)));
    }
    if let Some(a) = author {
        out.push_str(&format!("artist={}\n", ffmeta_escape(a)));
        out.push_str(&format!("album_artist={}\n", ffmeta_escape(a)));
    }
    if let Some(d) = date {
        out.push_str(&format!("date={}\n", ffmeta_escape(d)));
    }
    if let Some(i) = isbn {
        out.push_str(&format!("comment={}\n", ffmeta_escape(i)));
    }
    out.push_str("genre=Audiobook\n");
    out.push_str("encoder=epub-io\n");

    for (chapter_title, start, end) in marks {
        out.push_str("\n[CHAPTER]\nTIMEBASE=1/1000\n");
        out.push_str(&format!("START={start}\n"));
        out.push_str(&format!("END={end}\n"));
        out.push_str(&format!("title={}\n", ffmeta_escape(chapter_title)));
    }
    out
}

/// Escape a value for the ffmetadata format (`=`, `;`, `#`, `\`, and newlines).
fn ffmeta_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '=' | ';' | '#' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    out
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
    fn ffmetadata_has_global_tags_and_chapters() {
        let marks = vec![
            ("Introduction".to_string(), 0u64, 5000u64),
            ("Chapter 1".to_string(), 5000u64, 12000u64),
        ];
        let out = build_ffmetadata(Some("My Book"), Some("Jane Doe"), Some("2020"), Some("123"), &marks);
        assert!(out.starts_with(";FFMETADATA1"));
        assert!(out.contains("title=My Book"));
        assert!(out.contains("artist=Jane Doe"));
        assert!(out.contains("genre=Audiobook"));
        assert_eq!(out.matches("[CHAPTER]").count(), 2);
        assert!(out.contains("START=5000"));
        assert!(out.contains("END=12000"));
        assert!(out.contains("title=Chapter 1"));
    }

    #[test]
    fn ffmetadata_escapes_special_chars() {
        let marks = vec![("A=B; C#D".to_string(), 0u64, 1u64)];
        let out = build_ffmetadata(Some("x"), None, None, None, &marks);
        assert!(out.contains(r"title=A\=B\; C\#D"), "got: {out}");
    }

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
