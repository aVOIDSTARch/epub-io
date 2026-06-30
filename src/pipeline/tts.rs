// v0.0.1
use std::borrow::Cow;

/// Static table of abbreviation expansions applied in order.
static ABBREVIATIONS: &[(&str, &str)] = &[
    ("e.g.,", "for example,"),
    ("e.g.", "for example"),
    ("i.e.,", "that is,"),
    ("i.e.", "that is"),
    ("etc.", "and so on"),
    ("vs.", "versus"),
    ("approx.", "approximately"),
    ("dept.", "department"),
    ("est.", "established"),
    ("fig.", "figure"),
    ("govt.", "government"),
    ("incl.", "including"),
    ("min.", "minutes"),
    ("misc.", "miscellaneous"),
    ("no.", "number"),
    ("ref.", "reference"),
    ("vol.", "volume"),
    ("Mr.", "Mister"),
    ("Mrs.", "Missus"),
    ("Ms.", "Miss"),
    ("Dr.", "Doctor"),
    ("Prof.", "Professor"),
    ("Sr.", "Senior"),
    ("Jr.", "Junior"),
    ("St.", "Saint"),
    ("Sgt.", "Sergeant"),
    ("Lt.", "Lieutenant"),
    ("Cpl.", "Corporal"),
    ("Pvt.", "Private"),
    ("Gen.", "General"),
    ("Capt.", "Captain"),
    ("Cmdr.", "Commander"),
    ("Col.", "Colonel"),
    ("Maj.", "Major"),
    ("Gov.", "Governor"),
    ("Sen.", "Senator"),
    ("Rep.", "Representative"),
    ("Pres.", "President"),
    ("Sec.", "Secretary"),
    ("Hon.", "Honorable"),
    ("Rev.", "Reverend"),
    ("B.C.", "Before Christ"),
    ("A.D.", "Anno Domini"),
    ("a.m.", "in the morning"),
    ("p.m.", "in the afternoon"),
    ("U.S.", "United States"),
    ("U.K.", "United Kingdom"),
    ("U.N.", "United Nations"),
    ("D.C.", "District of Columbia"),
];

/// Apply TTS cleanup to an HTML chapter string.
/// Returns well-formed XHTML suitable for epub-builder content.
pub fn clean_for_tts(html: &str) -> String {
    // 1. Decode HTML entities
    let decoded = html_escape::decode_html_entities(html);

    // 2. Replace typographic symbols in text nodes
    let decoded = replace_symbols(&decoded);

    // 3. Remove footnote markers: [1], [^1], <sup>1</sup>
    let decoded = remove_footnote_markers(&decoded);

    // 4. Expand abbreviations in text content
    let decoded = expand_abbreviations(&decoded);

    // 5. Normalize whitespace
    normalize_whitespace(&decoded)
}

fn replace_symbols(s: &str) -> String {
    s.replace('\u{2014}', ", ")   // em-dash —
        .replace('\u{2013}', " to ") // en-dash –
        .replace('\u{2026}', ".")    // ellipsis …
        .replace('\u{201C}', "\"")  // left double quote "
        .replace('\u{201D}', "\"")  // right double quote "
        .replace('\u{2018}', "'")   // left single quote '
        .replace('\u{2019}', "'")   // right single quote '
        .replace('\u{00B7}', " ")   // middle dot ·
        .replace('\u{2022}', "")    // bullet •
        .replace('\u{00A0}', " ")   // non-breaking space
}

fn remove_footnote_markers(s: &str) -> String {
    // Remove <sup>...</sup> tags (footnote markers in HTML)
    let s = remove_html_tag_and_content(s, "sup");
    // Remove [n] and [^n] style markers
    remove_bracket_markers(&s)
}

fn remove_html_tag_and_content(s: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(&open) {
        result.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find(&close) {
            rest = &rest[start + end + close.len()..];
        } else {
            break;
        }
    }
    result.push_str(rest);
    result
}

fn remove_bracket_markers(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // Check for [^digits] or [digits]
            let rest = &s[i..];
            let inner_start = 1;
            let mut j = inner_start;
            let inner = rest.as_bytes();
            if j < inner.len() && inner[j] == b'^' {
                j += 1;
            }
            let digit_start = j;
            while j < inner.len() && inner[j].is_ascii_digit() {
                j += 1;
            }
            if j > digit_start && j < inner.len() && inner[j] == b']' {
                // Skip the whole [^123] or [123]
                i += j + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn expand_abbreviations(s: &str) -> String {
    let mut result = Cow::Borrowed(s);
    for (abbrev, expansion) in ABBREVIATIONS {
        if result.contains(abbrev) {
            result = Cow::Owned(result.replace(abbrev, expansion));
        }
    }
    result.into_owned()
}

fn normalize_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_space = false;
    let mut in_tag = false;

    for ch in s.chars() {
        match ch {
            '<' => {
                in_tag = true;
                prev_space = false;
                result.push(ch);
            }
            '>' => {
                in_tag = false;
                prev_space = false;
                result.push(ch);
            }
            ' ' | '\t' if !in_tag => {
                if !prev_space {
                    result.push(' ');
                }
                prev_space = true;
            }
            '\n' | '\r' if !in_tag => {
                if !prev_space {
                    result.push('\n');
                }
                prev_space = true;
            }
            _ => {
                prev_space = false;
                result.push(ch);
            }
        }
    }
    result
}

/// Wrap cleaned chapter content in a full EPUB-compatible XHTML shell.
pub fn wrap_xhtml(title: &str, lang: &str, body_content: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"
      xmlns:epub="http://www.idpf.org/2007/ops"
      xml:lang="{lang}" lang="{lang}">
<head>
  <meta charset="UTF-8"/>
  <title>{title}</title>
</head>
<body epub:type="bodymatter chapter">
{body_content}
</body>
</html>"#,
        lang = escape_attr(lang),
        title = escape_attr(title),
        body_content = body_content,
    )
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_eg() {
        let out = clean_for_tts("<p>e.g. cats</p>");
        assert!(out.contains("for example"), "got: {out}");
    }

    #[test]
    fn expands_dr() {
        let out = clean_for_tts("<p>Dr. Smith</p>");
        assert!(out.contains("Doctor Smith"), "got: {out}");
    }

    #[test]
    fn replaces_em_dash() {
        let out = clean_for_tts("<p>one\u{2014}two</p>");
        assert!(out.contains("one, two"), "got: {out}");
    }

    #[test]
    fn removes_sup_footnotes() {
        let out = clean_for_tts("<p>text<sup>1</sup> more</p>");
        assert!(!out.contains("<sup>"), "got: {out}");
    }

    #[test]
    fn removes_bracket_markers() {
        let out = clean_for_tts("<p>hello[1] world[^2]</p>");
        assert!(!out.contains("[1]"), "got: {out}");
        assert!(!out.contains("[^2]"), "got: {out}");
    }
}
