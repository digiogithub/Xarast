//! The fonts an SVG embeds, for resvg.
//!
//! `usvg` does not read `@font-face`. Xarast's SVG (exported, or a
//! `.xarast` package's `document.svg`) embeds each face it draws with as a
//! WOFF2 subset behind an `@font-face` rule (`research/06 §6.7`), so before
//! rendering we do what a browser does: decode those files and let them
//! shadow any installed face of the same family. The WOFF2 files are ours
//! (null transforms), decoded by `xarast_text::embed::woff2::decode`.

use std::path::Path;
use std::sync::Arc;

use resvg::usvg::fontdb::Database;

/// The `src` of every `@font-face` rule in `svg`: a `data:` URI or a path.
fn sources(svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find("@font-face{") {
        rest = &rest[i..];
        let end = rest.find('}').unwrap_or(rest.len());
        let rule = &rest[..end];
        if let Some(s) = rule.find("url(") {
            let src = &rule[s + 4..];
            if let Some(e) = src.find(')') {
                out.push(src[..e].trim_matches(['\'', '"']).to_owned());
            }
        }
        rest = &rest[end..];
    }
    out
}

/// Standard base64, padding optional; `None` on anything else.
fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// `base` plus every face `svg` embeds, each shadowing installed faces of
/// its family; `base` itself when the SVG embeds nothing. `dir` resolves
/// relative sources (a `.xarast` package's `resources/fonts/…`).
pub fn with_embedded(base: &Arc<Database>, svg: &str, dir: Option<&Path>) -> Arc<Database> {
    let files: Vec<Vec<u8>> = sources(svg)
        .into_iter()
        .filter_map(|src| {
            let woff2 = match src.strip_prefix("data:font/woff2;base64,") {
                Some(b64) => base64(b64)?,
                None => std::fs::read(dir?.join(&src)).ok()?,
            };
            xarast_text::embed::woff2::decode(&woff2)
        })
        .collect();
    if files.is_empty() {
        return Arc::clone(base);
    }
    let mut embedded = Database::new();
    for f in &files {
        embedded.load_font_data(f.clone());
    }
    let families: Vec<String> = embedded
        .faces()
        .flat_map(|f| f.families.iter().map(|(n, _)| n.to_lowercase()))
        .collect();
    let mut db = (**base).clone();
    let shadowed: Vec<_> = db
        .faces()
        .filter(|f| {
            f.families
                .iter()
                .any(|(n, _)| families.contains(&n.to_lowercase()))
        })
        .map(|f| f.id)
        .collect();
    for id in shadowed {
        db.remove_face(id);
    }
    for f in files {
        db.load_font_data(f);
    }
    Arc::new(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_and_base64() {
        let svg = "<style>@font-face{font-family:'A';src:url(data:font/woff2;base64,AAEC) format('woff2');}\n\
                   @font-face{font-family:'B';src:url(resources/fonts/b3-x.woff2) format('woff2');}</style>";
        assert_eq!(
            sources(svg),
            ["data:font/woff2;base64,AAEC", "resources/fonts/b3-x.woff2"]
        );
        assert_eq!(base64("AAEC"), Some(vec![0, 1, 2]));
        assert_eq!(base64("aGk="), Some(b"hi".to_vec()));
        assert_eq!(base64("a!"), None);
    }
}
