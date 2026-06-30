// v0.0.1
pub mod handlers;

use anyhow::Result;
use axum::{routing::get, routing::post, Router};
use open_library_api_rs::OpenLibraryClient;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::models::{ErrorResponse, HealthResponse};
use handlers::{convert, health};

pub struct AppState {
    pub ol_client: OpenLibraryClient,
}

#[derive(OpenApi)]
#[openapi(
    paths(handlers::health, handlers::convert),
    components(schemas(HealthResponse, ErrorResponse)),
    info(
        title = "epub-io",
        version = "0.1.0",
        description = "Convert ebooks to TTS-optimized EPUB with Open Library metadata enrichment"
    ),
    tags(
        (name = "System", description = "Health and status endpoints"),
        (name = "Conversion", description = "Ebook conversion endpoints")
    )
)]
struct ApiDoc;

pub async fn serve(host: &str, port: u16) -> Result<()> {
    let ol_client = OpenLibraryClient::builder()
        .build()
        .map_err(|e| anyhow::anyhow!("open library client: {e}"))?;

    let state = Arc::new(AppState { ol_client });

    let app = Router::new()
        .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()))
        .route("/health", get(health))
        .route("/api/v1/convert", post(convert))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid address {host}:{port}: {e}"))?;

    info!("listening on http://{addr}");
    info!("swagger UI at http://{addr}/docs");
    info!("openapi spec at http://{addr}/openapi.json");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
