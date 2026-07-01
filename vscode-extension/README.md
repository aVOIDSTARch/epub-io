# EPUB Studio

A VS Code extension for **viewing, editing, parsing, repairing, and correcting
metadata** in EPUB files — powered by the [`epub-io`](../) engine in this repo.

## Features

- **Custom EPUB editor** — open any `.epub` and get a rich view: cover, an
  editable Dublin Core metadata form, and a spine browser with an in-place
  reader. Metadata and chapter edits use VS Code's native dirty/save/undo, so
  `Ctrl/Cmd+S` writes a valid `.epub` back to disk.
- **Metadata correction** — edit title, author(s), language, identifier/ISBN,
  publisher, date, subjects and description. Edits are applied surgically to the
  OPF `<metadata>` block (the manifest/spine are left untouched), and the EPUB 3
  `dcterms:modified` timestamp is refreshed automatically.
- **Structure outline** — an activity-bar tree view of the active EPUB's
  metadata, spine and manifest. Click a spine entry to edit its XHTML source.
- **Chapter source editing** — extract a chapter to a normal editor tab; saving
  writes it straight back into the zip.
- **Extract plain text** — pull a chapter's readable text into a scratch buffer
  (handy for narration/TTS review).
- **Repair & Clean** — runs `epub-io convert` (offline) to produce a normalized
  `*.cleaned.epub`.
- **Enrich Metadata** — runs `epub-io convert` with Open Library enrichment to
  produce a `*.enriched.epub`.
- **Validate (EPUBCheck)** — runs [EPUBCheck](https://github.com/w3c/epubcheck)
  and shows the report. Java is resolved robustly (`epubStudio.javaPath` →
  `JAVA_HOME` → `java_home` → `PATH`), and it will **never** try to execute a
  `.jdk` bundle directory — the common cause of `spawn …/openjdk.jdk EACCES`.

## Requirements

- The `epub-io` binary (used for Repair/Enrich). By default the extension looks
  for `target/debug/epub-io` and `target/release/epub-io` in the workspace, then
  `epub-io` on `PATH`. Override with `epubStudio.epubIoPath`.
- For validation: `epubcheck.jar` (set `epubStudio.epubcheckJar`) and a JRE/JDK.

## Settings

| Setting | Purpose |
| ------- | ------- |
| `epubStudio.epubIoPath` | Path to the `epub-io` binary. |
| `epubStudio.epubcheckJar` | Path to `epubcheck.jar` for validation. |
| `epubStudio.javaPath` | Path to the `java` **binary** (…/Contents/Home/bin/java), not a `.jdk` directory. |

## Development

```bash
cd vscode-extension
npm install
npm run compile      # or: npm run watch
# then press F5 in VS Code to launch an Extension Development Host
```

The extension is written in TypeScript; EPUB parsing/repackaging uses `jszip`
and `fast-xml-parser`.
