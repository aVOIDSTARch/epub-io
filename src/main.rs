// v0.0.1
mod cli;
mod models;
mod pipeline;
mod server;

use anyhow::{Context, Result};
use clap::Parser;
use open_library_api_rs::OpenLibraryClient;
use std::path::PathBuf;
use tracing::info;

use cli::{Cli, Command};
use models::{ConvertOptions, EpubVersion};
use pipeline::{enrich, reader, tts, writer};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Convert {
            input,
            output,
            epub_version,
            no_enrich,
            no_tts,
            isbn,
        } => run_convert(input, output, epub_version, !no_enrich, !no_tts, isbn).await,
        Command::Audio {
            input,
            out_dir,
            format,
            voice,
            include_all,
            no_enrich,
            isbn,
        } => run_audio(input, out_dir, format, voice, include_all, !no_enrich, isbn).await,
        Command::Serve { host, port } => server::serve(&host, port).await,
    }
}

async fn run_convert(
    input: PathBuf,
    output: Option<PathBuf>,
    epub_version_str: String,
    enrich_meta: bool,
    tts_optimize: bool,
    isbn_override: Option<String>,
) -> Result<()> {
    let epub_version: EpubVersion = epub_version_str
        .parse()
        .with_context(|| format!("invalid epub version: {epub_version_str}"))?;

    let output_path = output.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap_or_default();
        input.with_file_name(format!("{}.epub", stem.to_string_lossy()))
    });

    info!("reading {input:?}");
    let read_result = tokio::task::spawn_blocking({
        let input = input.clone();
        move || reader::read_ebook(&input)
    })
    .await
    .context("reader task panicked")?
    .context("failed to read ebook")?;

    let opts = ConvertOptions {
        epub_version,
        enrich: enrich_meta,
        tts_optimize,
        isbn_override,
    };

    let enriched_meta = if opts.enrich {
        info!("enriching metadata via Open Library");
        let ol_client = OpenLibraryClient::builder()
            .build()
            .context("open library client init")?;
        enrich::enrich_metadata(&ol_client, read_result.metadata, opts.isbn_override.as_deref()).await
    } else {
        read_result.metadata
    };

    info!("building EPUB {:?}", epub_version);
    let chapters = read_result.chapters;
    let images = read_result.images;
    let epub_bytes = tokio::task::spawn_blocking(move || {
        writer::build_epub(&enriched_meta, &chapters, &images, epub_version, tts_optimize)
    })
    .await
    .context("writer task panicked")?
    .context("failed to build epub")?;

    std::fs::write(&output_path, &epub_bytes)
        .with_context(|| format!("failed to write {output_path:?}"))?;

    info!("wrote {} bytes to {output_path:?}", epub_bytes.len());
    println!("Done: {}", output_path.display());
    Ok(())
}

async fn run_audio(
    input: PathBuf,
    out_dir: Option<PathBuf>,
    format: String,
    voice: Option<String>,
    include_all: bool,
    enrich_meta: bool,
    isbn_override: Option<String>,
) -> Result<()> {
    let format = format.trim().to_lowercase();
    if !matches!(format.as_str(), "wav" | "mp3" | "m4b") {
        anyhow::bail!("unknown audio format: {format}; use wav, mp3, or m4b");
    }
    info!("reading {input:?}");
    let mut read_result = tokio::task::spawn_blocking({
        let input = input.clone();
        move || reader::read_ebook(&input)
    })
    .await
    .context("reader task panicked")?
    .context("failed to read ebook")?;

    if enrich_meta {
        info!("enriching metadata via Open Library");
        let ol_client = OpenLibraryClient::builder()
            .build()
            .context("open library client init")?;
        read_result.metadata =
            enrich::enrich_metadata(&ol_client, read_result.metadata, isbn_override.as_deref()).await;
    }

    // Post-pipeline: extract each chapter into a plain-text object with metadata.
    let chapters = reader::extract_chapter_texts(&read_result);
    let body = chapters.iter().filter(|c| c.role == models::ChapterRole::Body).count();
    info!(
        "extracted {} chapters ({} body to narrate{})",
        chapters.len(),
        body,
        if include_all { ", but --include-all set" } else { "" }
    );

    let out_dir = out_dir.unwrap_or_else(|| {
        input
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    });
    let file_stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ebook");

    info!("synthesizing audio via TTV");
    let audios = tts::synthesize_chapters(&chapters, &out_dir, file_stem, voice.as_deref(), include_all)
        .await
        .context("failed to synthesize chapter audio")?;

    if audios.is_empty() {
        anyhow::bail!("no chapters were synthesized (all skipped or empty)");
    }

    match format.as_str() {
        "wav" => {
            println!("Wrote {} WAV chapter file(s) to {}", audios.len(), out_dir.display());
        }
        "m4b" => {
            info!("assembling {} chapters into M4B audiobook", audios.len());
            let meta = read_result.metadata.clone();
            let stem = file_stem.to_string();
            let dir = out_dir.clone();
            let m4b = tokio::task::spawn_blocking(move || {
                tts::build_m4b(&meta, &audios, &dir, &stem)
            })
            .await
            .context("m4b task panicked")?
            .context("failed to assemble m4b audiobook")?;
            println!("Done: {}", m4b.display());
        }
        "mp3" => {
            let dir = out_dir.clone();
            let book_title = read_result.metadata.title.clone();
            let author = read_result.metadata.author.clone();
            let paths = tokio::task::spawn_blocking(move || {
                tts::transcode_chapters_to_mp3(&audios, &dir, book_title.as_deref(), author.as_deref())
            })
            .await
            .context("mp3 task panicked")?
            .context("failed to transcode chapters to mp3")?;
            println!("Wrote {} MP3 chapter file(s) to {}", paths.len(), out_dir.display());
        }
        _ => unreachable!(),
    }

    Ok(())
}
