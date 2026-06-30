// v0.0.1
use anyhow::{Context, Result};
use base64::engine::general_purpose;
use base64::Engine as _;
use epub_io::pipeline::reader;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct TtvResponse {
    audio_base64: String,
    format: String,
    sample_rate_hz: u32,
    channels: u16,
    byte_count: usize,
    frame_count: usize,
}

#[tokio::test]
async fn export_test_wav() -> Result<()> {
    let epub_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "TheLostHistoryofLiberalism(Rosenblatt,Helena)(z-library.sk,1lib.sk,z-lib.sk).epub",
    );
    let read_result = reader::read_ebook(&epub_path)?;
    let chapter_json = reader::extract_chapters(&read_result);
    let chapter_text = chapter_json["chapters"][0]["text"]
        .as_str()
        .context("missing chapter text")?;

    println!("Extracted chapter text:\n{}", chapter_text);

    let client = Client::new();
    let request_body = serde_json::json!({
        "text": chapter_text,
        "format": "wav",
        "sample_rate_hz": 24000,
    });

    let response = client
        .post("http://127.0.0.1:3310/ttv")
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

    let response: TtvResponse = serde_json::from_str(&body_text)
        .context("failed to parse TTV response JSON")?;

    let audio_bytes = general_purpose::STANDARD
        .decode(response.audio_base64.trim())
        .context("failed to decode base64 audio")?;

    let output_path = PathBuf::from("test_wav.wav");
    fs::write(&output_path, &audio_bytes)
        .with_context(|| format!("failed to write {}", output_path.display()))?;

    println!(
        "Saved {} bytes to {}",
        audio_bytes.len(),
        output_path.display()
    );

    Ok(())
}
