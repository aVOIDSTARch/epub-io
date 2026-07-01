// v0.0.1
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "epub-io",
    about = "Convert ebooks to TTS-optimized EPUB with Open Library metadata enrichment",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Convert an ebook file to a clean, TTS-optimized EPUB
    Convert {
        /// Input ebook (epub, mobi, pdf, fb2, txt, azw, cbz)
        input: PathBuf,

        /// Output file path (default: <input_stem>.epub)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// EPUB version to produce: 2 or 3 (default: 3)
        #[arg(long, default_value = "3")]
        epub_version: String,

        /// Skip Open Library metadata enrichment
        #[arg(long)]
        no_enrich: bool,

        /// Skip TTS cleanup and optimization
        #[arg(long)]
        no_tts: bool,

        /// Override ISBN used for Open Library lookup
        #[arg(long)]
        isbn: Option<String>,
    },

    /// Synthesize per-chapter WAV audio via the local TTV API (localhost:3310)
    Audio {
        /// Input ebook (epub, mobi, pdf, fb2, txt, azw, cbz)
        input: PathBuf,

        /// Directory to write WAV files into (default: alongside the input)
        #[arg(short, long)]
        out_dir: Option<PathBuf>,

        /// Output audio format: all (3 chaptered M4B tiers at 32/64/128 kbps
        /// plus a high-quality per-chapter MP3 fallback), m4b (single 64 kbps
        /// chaptered audiobook), mp3 (per-chapter), or wav (per-chapter).
        /// Default: all.
        #[arg(long, default_value = "all")]
        format: String,

        /// TTV voice identifier to use
        #[arg(long)]
        voice: Option<String>,

        /// Narrate every chapter, including front/back matter (cover, contents,
        /// index, bibliography, notes). By default only body chapters are read.
        #[arg(long)]
        include_all: bool,

        /// Skip Open Library metadata enrichment
        #[arg(long)]
        no_enrich: bool,

        /// Override ISBN used for Open Library lookup
        #[arg(long)]
        isbn: Option<String>,
    },

    /// Start the OpenAPI HTTP server
    Serve {
        /// Bind address
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// Port to listen on
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
}
