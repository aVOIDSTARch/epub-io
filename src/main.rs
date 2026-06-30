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
use pipeline::{enrich, reader, writer};

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
