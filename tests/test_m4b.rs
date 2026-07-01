// v0.0.1
//! Offline integration test for M4B assembly (no TTV server needed; requires
//! ffmpeg + ffprobe on PATH).

use epub_io::models::BookMetadata;
use epub_io::pipeline::tts::{build_m4b, build_m4b_tiers, ChapterAudio};
use std::path::Path;
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn gen_sine_wav(path: &Path, secs: u32) {
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={secs}"))
        .args(["-ar", "24000", "-ac", "1"])
        .arg(path)
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success(), "failed to generate test wav");
}

#[test]
fn assembles_m4b_with_chapters_and_cover() {
    if !have("ffmpeg") || !have("ffprobe") {
        eprintln!("ffmpeg/ffprobe not available; skipping");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path();

    let durations = [1u32, 2, 1];
    let mut chapters = Vec::new();
    for (i, secs) in durations.iter().enumerate() {
        let p = out.join(format!("testbook-ch{:03}.wav", i + 1));
        gen_sine_wav(&p, *secs);
        chapters.push(ChapterAudio {
            chapter_number: i + 1,
            title: format!("Chapter {}", i + 1),
            path: p,
        });
    }

    // Generate a small cover image.
    let cover_path = out.join("cover.jpg");
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
            "color=c=blue:s=120x120", "-frames:v", "1"])
        .arg(&cover_path)
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success());
    let cover_bytes = std::fs::read(&cover_path).unwrap();

    let meta = BookMetadata {
        title: Some("Test Book".to_string()),
        author: Some("Tester".to_string()),
        cover_image: Some(cover_bytes),
        cover_mime: Some("image/jpeg".to_string()),
        ..Default::default()
    };

    let m4b = build_m4b(&meta, &chapters, out, "testbook").expect("build m4b");
    assert!(m4b.exists(), "m4b not created");

    // Probe chapter count.
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-show_chapters", "-of", "json"])
        .arg(&m4b)
        .output()
        .expect("spawn ffprobe");
    let json = String::from_utf8_lossy(&probe.stdout);
    let chapter_count = json.matches("\"id\"").count();
    assert_eq!(chapter_count, 3, "expected 3 chapters, ffprobe said: {json}");

    // Probe global tags + an attached cover stream.
    let fmt = Command::new("ffprobe")
        .args(["-v", "error", "-show_format", "-show_streams", "-of", "json"])
        .arg(&m4b)
        .output()
        .expect("spawn ffprobe");
    let fmt_json = String::from_utf8_lossy(&fmt.stdout);
    assert!(fmt_json.contains("Test Book"), "missing book title tag");
    assert!(fmt_json.contains("attached_pic"), "missing embedded cover art");
}

#[test]
fn assembles_m4b_tiers_with_increasing_size() {
    if !have("ffmpeg") || !have("ffprobe") {
        eprintln!("ffmpeg/ffprobe not available; skipping");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path();

    let durations = [2u32, 2, 2];
    let mut chapters = Vec::new();
    for (i, secs) in durations.iter().enumerate() {
        let p = out.join(format!("tiers-ch{:03}.wav", i + 1));
        gen_sine_wav(&p, *secs);
        chapters.push(ChapterAudio {
            chapter_number: i + 1,
            title: format!("Chapter {}", i + 1),
            path: p,
        });
    }

    let meta = BookMetadata {
        title: Some("Tier Book".to_string()),
        author: Some("Tester".to_string()),
        ..Default::default()
    };

    let paths = build_m4b_tiers(&meta, &chapters, out, "tiers", &["32k", "64k", "128k"])
        .expect("build m4b tiers");
    assert_eq!(paths.len(), 3, "expected 3 tiers");

    let mut sizes = Vec::new();
    for p in &paths {
        assert!(p.exists(), "tier not created: {:?}", p);
        // Every tier keeps all three chapter markers.
        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-show_chapters", "-of", "json"])
            .arg(p)
            .output()
            .expect("spawn ffprobe");
        let json = String::from_utf8_lossy(&probe.stdout);
        assert_eq!(json.matches("\"id\"").count(), 3, "expected 3 chapters in {:?}", p);
        sizes.push(std::fs::metadata(p).unwrap().len());
    }

    // Higher bitrate ⇒ larger file: 32k < 64k < 128k.
    assert!(sizes[0] < sizes[1], "32k should be smaller than 64k: {sizes:?}");
    assert!(sizes[1] < sizes[2], "64k should be smaller than 128k: {sizes:?}");
}
