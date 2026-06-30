// v0.0.1
//! Chapter-role classification.
//!
//! Tags each chapter as front matter, body, or back matter so the audio path can
//! skip material that is miserable to listen to (indexes, bibliographies, notes,
//! copyright pages, etc.). The EPUB output keeps every chapter regardless.
//!
//! Classification is heuristic, based on the chapter title and source filename.
//! Spine `linear="no"` and EPUB `landmarks` could refine this later, but titles
//! and filenames are reliable enough for the common case.

use crate::models::ChapterRole;

/// Strong body signals — checked first so an "Introduction"/"Epilogue" is never
/// misfiled as front/back matter. The user wants these narrated.
const BODY: &[&str] = &[
    "chapter",
    "introduction",
    "prologue",
    "epilogue",
    "conclusion",
    "afterword",
    "part ",
];

/// Back-matter signals — content typically not worth listening to.
const BACK: &[&str] = &[
    "index",
    "bibliograph",
    "glossary",
    "appendix",
    "references",
    "endnote",
    "colophon",
    "about the author",
    "further reading",
    "permissions",
    "credits",
    "notes",
    "_bm",
    "backmatter",
];

/// Front-matter signals — everything before the real content begins.
const FRONT: &[&str] = &[
    "cover",
    "half title",
    "halftitle",
    "half-title",
    "title page",
    "titlepage",
    "frontispiece",
    "copyright",
    "dedication",
    "contents",
    "table of contents",
    "toc",
    "acknowledg",
    "foreword",
    "preface",
    "epigraph",
    "also by",
    "praise for",
    "imprint",
    "list of illustrations",
    "list of figures",
    "list of tables",
    "_fm",
    "frontmatter",
];

/// Classify a chapter from its title and source filename.
///
/// Precedence: body signals win first (so "Introduction" stays body), then back
/// matter, then front matter. Anything unrecognized defaults to [`ChapterRole::Body`]
/// so genuine content with a descriptive title (e.g. "The Birth of Liberalism")
/// is never silently dropped from the audio.
pub fn classify_role(title: &str, filename: &str) -> ChapterRole {
    let hay = format!("{} {}", title.to_lowercase(), filename.to_lowercase());

    if BODY.iter().any(|kw| hay.contains(kw)) {
        ChapterRole::Body
    } else if BACK.iter().any(|kw| hay.contains(kw)) {
        ChapterRole::BackMatter
    } else if FRONT.iter().any(|kw| hay.contains(kw)) {
        ChapterRole::FrontMatter
    } else {
        ChapterRole::Body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_chapters() {
        assert_eq!(classify_role("Chapter 1", "10_Chapter01.xhtml"), ChapterRole::Body);
        assert_eq!(classify_role("Introduction", "09_Introduction.xhtml"), ChapterRole::Body);
        assert_eq!(classify_role("Epilogue", "17_Epilogue.xhtml"), ChapterRole::Body);
        // Unknown but descriptive title -> body (don't drop real content).
        assert_eq!(classify_role("The Birth of Liberalism", "x.xhtml"), ChapterRole::Body);
    }

    #[test]
    fn back_matter() {
        assert_eq!(classify_role("Index", "20_Index01.xhtml"), ChapterRole::BackMatter);
        assert_eq!(classify_role("Bibliography", "19_Bibliography.xhtml"), ChapterRole::BackMatter);
        assert_eq!(classify_role("Notes", "18_Notes.xhtml"), ChapterRole::BackMatter);
        assert_eq!(classify_role("", "21_Bm01.xhtml"), ChapterRole::BackMatter);
    }

    #[test]
    fn front_matter() {
        assert_eq!(classify_role("Cover Page", "01_Cover.xhtml"), ChapterRole::FrontMatter);
        assert_eq!(classify_role("Copyright", "04_Copyright01.xhtml"), ChapterRole::FrontMatter);
        assert_eq!(classify_role("Dedication", "05_Dedication.xhtml"), ChapterRole::FrontMatter);
        assert_eq!(classify_role("Contents", "06_Contents.xhtml"), ChapterRole::FrontMatter);
        assert_eq!(classify_role("Acknowledgments", "07_Acknowledgments.xhtml"), ChapterRole::FrontMatter);
        assert_eq!(classify_role("", "08_Fm01.xhtml"), ChapterRole::FrontMatter);
    }
}
