

// v0.0.1
use open_library_api_rs::models::common::{CoverKey, ImageSize};
use open_library_api_rs::models::search::{SearchParams};
use open_library_api_rs::OpenLibraryClient;
use tracing::{debug, warn};

use crate::models::BookMetadata;

pub async fn enrich_metadata(
    client: &OpenLibraryClient,
    mut meta: BookMetadata,
    isbn_override: Option<&str>,
) -> BookMetadata {
    let effective_isbn = isbn_override
        .map(ToOwned::to_owned)
        .or_else(|| meta.isbn.clone());

    let edition = if let Some(ref isbn) = effective_isbn {
        match client.get_edition_by_isbn(isbn).await {
            Ok(ed) => {
                debug!("ISBN lookup succeeded for {isbn}");
                Some(ed)
            }
            Err(e) => {
                warn!("ISBN lookup failed for {isbn}: {e}");
                None
            }
        }
    } else {
        None
    };

    if let Some(ed) = edition {
        apply_edition(&mut meta, ed, client).await;
    } else {
        // Fallback: title+author search and edition fetch by title.
        let title = meta.title.clone();
        let author = meta.author.clone();

        if title.is_some() || author.is_some() {
            let params = SearchParams {
                title: title.clone(),
                author,
                limit: Some(1),
                ..Default::default()
            };

            match client.search(params).await {
                Ok(resp) if !resp.docs.is_empty() => {
                    let doc = &resp.docs[0];
                    debug!("search found: {:?}", doc.title);

                    if let Some(edition_key) = doc.cover_edition_key.clone() {
                        match client.get_edition(&edition_key).await {
                            Ok(ed) => {
                                debug!("edition lookup succeeded for {edition_key}");
                                apply_edition(&mut meta, ed, client).await;
                                return meta;
                            }
                            Err(e) => warn!("edition lookup failed for {edition_key}: {e}"),
                        }
                    }

                    if let Some(isbn) = doc.isbn.as_ref().and_then(|v| v.first()) {
                        match client.get_edition_by_isbn(isbn).await {
                            Ok(ed) => {
                                debug!("search-doc ISBN lookup succeeded for {isbn}");
                                apply_edition(&mut meta, ed, client).await;
                                return meta;
                            }
                            Err(e) => warn!("search-doc ISBN lookup failed for {isbn}: {e}"),
                        }
                    }

                    if meta.publisher.is_none() {
                        meta.publisher = doc.publisher.as_ref().and_then(|v| v.first().cloned());
                    }
                    if meta.language.is_none() {
                        meta.language = doc.language.as_ref().and_then(|v| v.first().cloned());
                    }
                    if meta.publication_date.is_none() {
                        meta.publication_date = doc.first_publish_year.map(|y| y.to_string());
                    }
                    if meta.tags.is_empty() {
                        if let Some(subjects) = &doc.subject {
                            meta.tags = subjects.iter().take(10).cloned().collect();
                        }
                    }
                    if meta.cover_image.is_none() {
                        if let Some(cover_id) = doc.cover_i {
                            meta.cover_image = fetch_cover(client, &cover_id.to_string()).await;
                            meta.cover_mime = meta.cover_image.as_ref().map(|_| "image/jpeg".to_string());
                        }
                    }
                }
                Ok(_) => debug!("search returned no results"),
                Err(e) => warn!("search failed: {e}"),
            }
        }
    }

    meta
}

async fn apply_edition(
    meta: &mut BookMetadata,
    ed: open_library_api_rs::models::edition::Edition,
    client: &OpenLibraryClient,
) {
    if meta.title.is_none() {
        meta.title = ed.full_title.or(ed.title);
    }
    if meta.publisher.is_none() {
        meta.publisher = ed.publishers.as_ref().and_then(|v| v.first()).cloned();
    }
    if meta.publication_date.is_none() {
        meta.publication_date = ed.publish_date.clone();
    }
    if meta.language.is_none() {
        meta.language = ed
            .languages
            .as_ref()
            .and_then(|v| v.first())
            .map(|k| k.key.trim_start_matches("/languages/").to_string());
    }
    if meta.description.is_none() {
        meta.description = ed
            .description
            .map(|d| d.into_text());
    }
    if meta.isbn.is_none() {
        meta.isbn = ed
            .isbn_13
            .as_ref()
            .and_then(|v| v.first())
            .or_else(|| ed.isbn_10.as_ref().and_then(|v| v.first()))
            .cloned();
    }
    if meta.tags.is_empty() {
        if let Some(subjects) = &ed.subjects {
            meta.tags = subjects.iter().take(10).cloned().collect();
        }
    }
    if meta.cover_image.is_none() {
        if let Some(cover_id) = ed.covers.as_ref().and_then(|v| v.first()) {
            meta.cover_image = fetch_cover(client, &cover_id.to_string()).await;
            meta.cover_mime = meta.cover_image.as_ref().map(|_| "image/jpeg".to_string());
        }
    }
}

async fn fetch_cover(client: &OpenLibraryClient, cover_id: &str) -> Option<Vec<u8>> {
    let url = client.cover_url(CoverKey::Id, cover_id, ImageSize::Large);
    match reqwest::get(url).await {
        Ok(resp) if resp.status().is_success() => match resp.bytes().await {
            Ok(bytes) => {
                debug!("fetched cover image ({} bytes)", bytes.len());
                Some(bytes.to_vec())
            }
            Err(e) => {
                warn!("failed to read cover bytes: {e}");
                None
            }
        },
        Ok(resp) => {
            warn!("cover fetch returned HTTP {}", resp.status());
            None
        }
        Err(e) => {
            warn!("cover fetch error: {e}");
            None
        }
    }
}
