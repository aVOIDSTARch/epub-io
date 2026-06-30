// v0.0.1
use axum::{
    extract::{Multipart, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::task;
use tracing::{error, info};
use crate::models::{ConvertOptions, ErrorResponse, EpubVersion, HealthResponse};
use crate::pipeline::{enrich, reader, writer};
use crate::server::AppState;

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse)
    ),
    tag = "System"
)]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok".to_string() })
}

/// Convert an ebook to TTS-optimized EPUB.
///
/// Upload an ebook file (epub, mobi, pdf, fb2, txt, azw, cbz) as `multipart/form-data`.
/// Fields: `file` (required), `epub_version` (2|3, default 3), `enrich` (bool, default true),
/// `tts_optimize` (bool, default true), `isbn` (optional override).
/// The response is a binary EPUB file download (`application/epub+zip`).
#[utoipa::path(
    post,
    path = "/api/v1/convert",
    responses(
        (status = 200, description = "Converted EPUB file (application/epub+zip)"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 422, description = "Conversion failed", body = ErrorResponse),
    ),
    tag = "Conversion"
)]
pub async fn convert(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Response {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut original_filename = String::from("upload");
    let mut opts = ConvertOptions::default();

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("file") => {
                if let Some(name) = field.file_name() {
                    original_filename = name.to_string();
                }
                match field.bytes().await {
                    Ok(b) => file_bytes = Some(b.to_vec()),
                    Err(e) => {
                        return error_response(StatusCode::BAD_REQUEST, &format!("failed to read file field: {e}"));
                    }
                }
            }
            Some("epub_version") => {
                if let Ok(text) = field.text().await {
                    opts.epub_version = text.trim().parse().unwrap_or(EpubVersion::V3);
                }
            }
            Some("enrich") => {
                if let Ok(text) = field.text().await {
                    opts.enrich = !matches!(text.trim().to_lowercase().as_str(), "false" | "0" | "no");
                }
            }
            Some("tts_optimize") => {
                if let Ok(text) = field.text().await {
                    opts.tts_optimize = !matches!(text.trim().to_lowercase().as_str(), "false" | "0" | "no");
                }
            }
            Some("isbn") => {
                if let Ok(text) = field.text().await {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        opts.isbn_override = Some(trimmed);
                    }
                }
            }
            _ => {}
        }
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => return error_response(StatusCode::BAD_REQUEST, "no file field in multipart body"),
    };

    info!("converting {original_filename} ({} bytes)", bytes.len());

    let ol_client = state.ol_client.clone();
    let filename_for_closure = original_filename.clone();
    let result = task::spawn_blocking(move || {
        // Write to a temp file so the ebook crate can detect format via extension
        let suffix = std::path::Path::new(&filename_for_closure)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();

        let mut tmp = NamedTempFile::with_suffix(&suffix)
            .map_err(|e| anyhow::anyhow!("temp file: {e}"))?;

        use std::io::Write;
        tmp.write_all(&bytes).map_err(|e| anyhow::anyhow!("write temp: {e}"))?;

        let path = tmp.path().to_path_buf();
        reader::read_ebook(&path)
    })
    .await;

    let read_result = match result {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return error_response(StatusCode::UNPROCESSABLE_ENTITY, &format!("read error: {e}")),
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("task panic: {e}")),
    };

    // Enrich metadata asynchronously
    let enriched_meta = if opts.enrich {
        enrich::enrich_metadata(
            &ol_client,
            read_result.metadata,
            opts.isbn_override.as_deref(),
        )
        .await
    } else {
        read_result.metadata
    };

    let chapters = read_result.chapters;
    let images = read_result.images;
    let epub_version = opts.epub_version;
    let tts_optimize = opts.tts_optimize;

    let epub_bytes = task::spawn_blocking(move || {
        writer::build_epub(&enriched_meta, &chapters, &images, epub_version, tts_optimize)
    })
    .await;

    match epub_bytes {
        Ok(Ok(bytes)) => {
            let stem = std::path::Path::new(&original_filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");
            let disposition = format!("attachment; filename=\"{stem}.epub\"");

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/epub+zip"),
                    (header::CONTENT_DISPOSITION, &disposition),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(Err(e)) => error_response(StatusCode::UNPROCESSABLE_ENTITY, &format!("build error: {e}")),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("task panic: {e}")),
    }
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    error!("{msg}");
    (status, Json(ErrorResponse { error: msg.to_string() })).into_response()
}
