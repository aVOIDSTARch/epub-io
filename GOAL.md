# epub-io — Goal, Architecture & Roadmap

> **This is the single source of truth for the project.** Read it before planning new
> work. Update it as you go. Do **not** re-assess the whole project from scratch each
> session — trust the "Locked decisions" section and the checkboxes, and only revisit a
> decision if you have a concrete reason (write that reason in the Progress Log).

**How to use this file**

- Work top-to-bottom through the **Roadmap**. Each phase has a goal, tasks, and acceptance criteria.
- Mark progress by editing checkboxes: `- [ ]` → `- [x]`. Use `- [~]` for in-progress.
- Keep the **Status at a glance** table in sync (it's the fast overview).
- Add a dated line to the **Progress Log** whenever you complete a task or make a decision.
- Status legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` blocked (say why in the log)

**Last updated:** 2026-06-30

---

## 1. Mission (the why)

The user struggles with reading due to **ADHD**. epub-io turns books into **high-quality,
engaging audio they can actually consume** — plus a clean re-readable ebook. Every design
tradeoff should optimize for *"would someone with ADHD stay engaged listening to this?"*,
not just correctness. That means: great voices, dynamic narration, and **ruthlessly
skipping content that's miserable to listen to** (indexes, bibliographies, footnote dumps,
page numbers).

## 2. What "done" looks like

A website where the user drops **any** major book file and gets back, served via a fast,
secure API (to that site and other consumers):

1. A **pristine, consistent EPUB 3** — well-structured, sliceable into chapters. This is a
   **first-class, retained artifact**, not a throwaway intermediate: it is **kept and
   shareable with others**, and is the user's **read mode** for when they can't or don't
   want to listen. Audio and EPUB are co-equal outputs.
2. A **collection of high-quality, streamable, portable audio** (per-chapter files +
   a single resumable **M4B** audiobook with chapter markers), generated via the user's
   voice API (TTV) + ffmpeg, with **engaging SSML-driven narration**.

## 3. Architecture

The **keystone is a normalized intermediate model (the Book IR)**. Every input format
normalizes into it; both outputs flow out of it.

```text
                                                                                            │
                                                            [API: drop-in → store → stream back] ◄─ website
```

**Book IR** (target shape): `Book { metadata, chapters: [{ title, html, plain_text, role }] }`

- `html` — structured markup, consumed by the EPUB writer.
- `plain_text` — markup stripped, consumed by the audio path.
- `role` — `FrontMatter | Body | BackMatter` (drives audio inclusion + EPUB landmarks).

**Module map** (`src/`)

- `pipeline/reader.rs` — format dispatch + IR construction (`read_ebook`, `extract_chapter_texts`).
- `pipeline/epub_reader.rs` — direct EPUB spine reader (real per-chapter content).
- `pipeline/enrich.rs` — Open Library metadata enrichment.
- `pipeline/writer.rs` — EPUB building (`build_epub`).
- `pipeline/tts.rs` — readability cleanup, TTV synthesis, WAV metadata, MP3/M4B.
- `server/` — Axum HTTP API. `models.rs` — IR + DTOs. `cli.rs` / `main.rs` — CLI.

## 4. Status at a glance

| Phase | Goal | Status |
|------|------|--------|
| 0 | Foundation: EPUB read → chapters → plain text → per-chapter WAV | ✅ done |
| 1 | Chapter-role classification (skip back-matter for audio) | ✅ done |
| 2 | M4B audiobook assembly (chapters + cover, via ffmpeg) | ⬜ next |
| 3 | SSML readability layer + voice library | ⬜ todo |
| 4 | Pristine EPUB 3 output (landmarks, per-chapter spine, valid) | ⬜ todo |
| 5 | Multi-format ingestion (mobi/azw, fb2, txt, pdf best-effort) | ⬜ todo |
| 6 | Web service: drop-in → store → stream back via API | 🟨 partial |
| 7 | Security, performance, fidelity hardening | ⬜ todo |

## 5. Locked decisions (don't re-litigate without a concrete reason)

- **The bundled `ebook` crate (0.1.2) is broken for reading.** It only handles
  `Event::Start` when parsing the OPF manifest/spine, so EPUBs using self-closing
  `<item/>`/`<itemref/>` (almost all) yield **empty content**. We read EPUB spines
  directly in `epub_reader.rs` instead. `ebook` is still used only for metadata + images
  (which it extracts independently). Treat its non-EPUB readers as **unverified**.
- **Keep HTML in the IR.** The EPUB writer needs structure; audio strips to plain text on
  demand via `tts::html_to_plain_text`.
- **`epub-parser` crate is NOT a fit for the writer path** — it yields plain text only (no
  HTML) and `Page` has no href/filename, so titles can't be mapped precisely. It's fine
  only if we ever want an audio-only path; not worth the swap now.
- **Audio target = per-chapter files + one M4B** (AAC/MP4 with chapter markers + cover).
  Per-chapter files are the streaming unit; M4B is the portable, resumable audiobook.
- **Streaming v1 = HTTP range requests** over the API; HLS only if/when needed.
- **Voices:** Apple voice (via TTV API at `localhost:3310/ttv`) for now, but its SSML
  support is weak. For expressive narration + a collectible voice library + cost control at
  book scale, plan toward local neural TTS (Piper / Kokoro / XTTS) behind the same TTV API.
- **Back-matter is skipped for audio** (see Phase 1) — proven necessary: synthesizing the
  185 KB index produced a multi-hundred-MB useless WAV. **Note:** back-matter is skipped for
  *audio only* — the **EPUB keeps the full book** (index, notes, etc.) since it's the read mode.
- **The EPUB is a retained, shareable artifact**, co-equal with audio — not a throwaway
  intermediate. Storage must persist the EPUB per book and allow sharing/serving it to others
  and back to the user as a **read mode**. This raises the priority of pristine EPUB output
  (Phase 4) and durable storage (Phase 6).

## 6. Current foundation (works & verified)

- ✅ EPUB → distinct per-chapter content (`epub_reader.rs`), titles from NCX navMap.
- ✅ Metadata + images via `ebook`; Open Library enrichment (`enrich.rs`).
- ✅ `ChapterText` IR object: plain text (no markup) + book metadata (`models.rs`).
- ✅ `reader::extract_chapter_texts()` — post-pipeline chapter objects.
- ✅ TTV per-chapter WAV with metadata embedded in RIFF `LIST/INFO` chunk (`tts.rs`).
- ✅ CLI `audio` command; verified end-to-end against the running TTV API (ffprobe reads
  back title/artist/album/track/date/ISBN/genre tags).
- ✅ Network-free regression test asserting chapters are distinct & markup-free.
- 🟨 HTTP API: `/health`, `/api/v1/convert` (EPUB build only — no audio, no storage yet).

**Known tech debt:** `writer.rs` sets `ReferenceType::Text` for every chapter (no
landmarks); non-EPUB formats untested; `synthesize_chapter_mp3` exists but isn't wired into
a batch/CLI flow.

---

## 7. Roadmap

### Phase 0 — Foundation ✅ (done)

- [x] Direct EPUB spine reader with real per-chapter content

- [x] Fix chapter-splitting bug (every chapter held whole book)
- [x] `ChapterText` (plain text + metadata) + `extract_chapter_texts`
- [x] Per-chapter WAV via TTV with embedded RIFF metadata
- [x] CLI `audio` command + end-to-end verification + regression test

### Phase 1 — Chapter-role classification ✅ (done)

**Goal:** Never narrate junk. Tag each chapter `FrontMatter | Body | BackMatter`; audio
synthesizes Body (+ Introduction/Epilogue) only.

- [x] Add `role` to the IR — `ChapterRole` enum + field on `Chapter`/`ChapterText` (`models.rs`)
- [x] Classifier (`pipeline/classify.rs`): title + filename heuristics, body-first precedence,
      unknown→Body so real content is never dropped. Wired into both `epub_reader.rs` and the
      TOC/fallback paths in `reader.rs`. (Spine `linear`/landmarks left as a future refinement.)
- [x] `synthesize_chapters` skips non-body roles by default; `--include-all` CLI override
- [x] Unit tests: `classify.rs` cases + sample-book test asserting index/biblio/notes =
      BackMatter, cover = FrontMatter, introduction = Body, ≥8 body chapters
- [x] **Acceptance met:** `audio` on the sample book reports "10 body to narrate", skips all
      front/back matter (incl. the 185 KB index); first synthesized file is the Introduction.

### Phase 2 — M4B audiobook assembly ⬜ (next)

**Goal:** One resumable, portable audiobook file with chapter navigation.

- [ ] Batch synth to a chosen codec (reuse/extend `synthesize_chapter_mp3`; prefer AAC)
- [ ] Generate ffmpeg chapter-metadata file (timestamps from each chapter's duration)
- [ ] ffmpeg concat → `.m4b` with chapter markers, embedded cover art + title/author tags
- [ ] CLI flag: `--format wav|mp3|m4b` (default m4b); keep per-chapter files too
- **Acceptance:** produced `.m4b` plays with working chapter skip + cover in a standard
  audiobook player; `ffprobe` shows chapters.

### Phase 3 — SSML readability + voices ⬜

**Goal:** Narration that's engaging, not robotic.

- [ ] Extend `clean_for_tts` → emit SSML: paragraph/section pauses, sentence pacing,
      emphasis; strip page numbers + footnote refs
- [ ] Probe TTV API for SSML support; gate SSML behind capability (fallback to plain text)
- [ ] Voice selection plumbing end-to-end (`--voice`) + a place to register/collect voices
- [ ] Investigate local neural TTS (Piper/Kokoro/XTTS) behind the TTV API for quality/cost
- **Acceptance:** A/B a chapter plain vs SSML; SSML version has natural pauses & pacing.

### Phase 4 — Pristine EPUB 3 output ⬜

**Goal:** The rebuilt EPUB is genuinely clean and standards-valid.

- [ ] Real per-chapter spine documents (not one blob); stable filenames
- [ ] `nav.xhtml` with landmarks (cover, toc, bodymatter) driven by `role`
- [ ] Correct `ReferenceType` per chapter (fix `writer.rs` always-`Text`)
- [ ] Consistent typography CSS; valid EPUB 3 (run EPUBCheck)
- **Acceptance:** output passes EPUBCheck with no errors; opens cleanly with working nav.

### Phase 5 — Multi-format ingestion ⬜

**Goal:** "ANY major format" in tiers (input quality varies; output EPUB normalizes it).

- [ ] MOBI / AZW3 reader into the IR (Kindle) — real structure
- [ ] FB2 + TXT readers (easy)
- [ ] PDF best-effort (text extraction; chapter boundaries are hard — set expectations)
- [ ] Per-format integration tests with sample files
- **Acceptance:** each tiered format produces a valid EPUB + audio; PDF documented as best-effort.

### Phase 6 — Web service ⬜ (partial today)

**Goal:** Drop a file in the browser → get EPUB + audio collection back via API.

- [ ] `POST /api/v1/audiobook`: upload → IR → EPUB + audio collection → job result
- [ ] Storage layer for artifacts (EPUB, per-chapter audio, M4B) keyed by book — **durable**,
      since the EPUB is a kept/shareable artifact (read mode), not a throwaway
- [ ] Library + sharing: serve the retained EPUB to the user (read mode) and to others
- [ ] Streaming endpoints with HTTP range support (per-chapter + M4B)
- [ ] Async job model + progress for long conversions (a book = hours of audio)
- [ ] Minimal website: drop zone, progress, library, in-browser player
- **Acceptance:** drop a book in the browser, watch progress, stream the result back.

### Phase 7 — Hardening ⬜

**Goal:** Fast, secure, high fidelity.

- [ ] Input validation, size limits, sandboxing of ffmpeg/format parsers
- [ ] AuthN/Z for the API; safe artifact access
- [ ] Concurrency/throughput for synthesis; caching; resumable jobs
- [ ] Audio fidelity pass (sample rate, codec settings, loudness normalization)

---

## 8. Progress Log

Append newest entries at the top. One line per completion or decision.

- **2026-06-30** — Phase 1 complete. Added `ChapterRole` (FrontMatter/Body/BackMatter) to the
  IR + `pipeline/classify.rs` (title+filename heuristics, body-first, unknown→Body). Wired into
  both EPUB read paths; `synthesize_chapters` now narrates body only with `--include-all`
  override. Unit + sample-book tests pass; verified end-to-end: "10 body to narrate", front/back
  matter (incl. 185 KB index) skipped, synthesis starts at the Introduction.
- **2026-06-30** — Decision: the EPUB is a **retained, shareable artifact** and the user's
  **read mode** (for others / when they can't listen), co-equal with audio. Back-matter is
  skipped for audio only; the EPUB keeps the full book. Updated mission, decisions, Phase 6.
- **2026-06-30** — Phase 0 complete. Added direct EPUB reader (`epub_reader.rs`) after
  discovering the `ebook` crate is broken for self-closing OPF tags; fixed chapter-splitting
  bug; added `ChapterText` IR + `extract_chapter_texts`; added TTV per-chapter WAV with RIFF
  metadata + CLI `audio`; verified end-to-end against the live TTV API. Authored this GOAL.md.
