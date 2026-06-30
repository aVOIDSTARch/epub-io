// v0.0.1
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EpubVersion {
    V2,
    #[default]
    V3,
}

impl EpubVersion {
    pub fn to_builder_version(self) -> epub_builder::EpubVersion {
        match self {
            EpubVersion::V2 => epub_builder::EpubVersion::V20,
            EpubVersion::V3 => epub_builder::EpubVersion::V30,
        }
    }
}

impl std::str::FromStr for EpubVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "2" | "v2" | "V2" => Ok(EpubVersion::V2),
            "3" | "v3" | "V3" => Ok(EpubVersion::V3),
            other => Err(anyhow::anyhow!("unknown epub version: {other}; use 2 or 3")),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BookMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub publication_date: Option<String>,
    pub cover_image: Option<Vec<u8>>,
    pub cover_mime: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub content: String,
    pub filename: String,
}

/// A single chapter reduced to plain text (no markup) plus the book-level
/// metadata needed to tag downstream artifacts such as audio files.
///
/// This is the post-pipeline representation fed to the TTV synthesis stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterText {
    pub chapter_number: usize,
    pub title: String,
    pub filename: String,
    pub page_range: Option<String>,
    /// The actual readable text of the chapter, with all HTML markup stripped.
    pub text: String,
    // — Book-level metadata copied in for convenience/tagging —
    pub book_title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub epub_version: EpubVersion,
    pub enrich: bool,
    pub tts_optimize: bool,
    pub isbn_override: Option<String>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            epub_version: EpubVersion::V3,
            enrich: true,
            tts_optimize: true,
            isbn_override: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}
