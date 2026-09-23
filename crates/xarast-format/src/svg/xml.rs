//! The small amount of XML writing the SVG profile needs.
//!
//! Hand-written rather than through `quick_xml::Writer`: the output is
//! built attribute by attribute from the model, the escaping rules are four
//! lines, and a string builder is the cheapest way to produce 20 MB of it.
//! What this module guarantees is that **whatever the model holds, the
//! output is well-formed XML**: text and attribute values are escaped,
//! characters XML 1.0 forbids are dropped, and names are checked before
//! they are written.

/// Whether a character may appear in an XML 1.0 document.
#[must_use]
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// Appends `s` escaped for a double-quoted attribute value.
///
/// Tabs and line breaks are written as character references so that
/// attribute-value normalisation on reading gives them back.
pub fn push_attr_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c if is_xml_char(c) => out.push(c),
            _ => {}
        }
    }
}

/// Appends `s` escaped for character data.
pub fn push_text_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            c if is_xml_char(c) => out.push(c),
            _ => {}
        }
    }
}

/// Appends ` name="value"`, escaping the value.
pub fn attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    push_attr_escaped(out, value);
    out.push('"');
}

/// Whether `s` is an XML `NCName` (a name with no colon).
#[must_use]
pub fn is_ncname(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let start = |c: char| {
        c == '_'
            || c.is_ascii_alphabetic()
            || matches!(c, '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
                | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}'
                | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
                | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
    };
    start(first) && chars.all(|c| {
        start(c)
            || c.is_ascii_digit()
            || matches!(c, '-' | '.' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
    })
}

/// Standard base64, padded, for the few binary payloads written inline.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let sym = |i: u32| char::from(T.get((i & 63) as usize).copied().unwrap_or(b'A'));
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        out.push(sym(n >> 18));
        out.push(sym(n >> 12));
        out.push(if chunk.len() > 1 { sym(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(n) } else { '=' });
    }
    out
}

/// Whether a verbatim fragment can be re-emitted without breaking the
/// document: a comment, a processing instruction, or exactly one balanced
/// element (with any trailing whitespace), and never a DOCTYPE or CDATA
/// that would hide markup.
///
/// The reader only ever stores fragments that pass this; the check is
/// here too because the model's API is public and a fragment that did not
/// come from the reader must not be able to corrupt a save.
#[must_use]
pub fn fragment_is_well_formed(raw: &str, kind: crate::svg::FragmentKind) -> bool {
    use crate::svg::FragmentKind as K;
    if !raw.chars().all(is_xml_char) {
        return false;
    }
    match kind {
        K::Comment => {
            let Some(body) = raw.strip_prefix("<!--").and_then(|r| r.strip_suffix("-->")) else {
                return false;
            };
            !body.contains("--") && !body.ends_with('-')
        }
        K::ProcessingInstruction => {
            let Some(body) = raw.strip_prefix("<?").and_then(|r| r.strip_suffix("?>")) else {
                return false;
            };
            let target = body.split_whitespace().next().unwrap_or("");
            is_ncname(target) && !target.eq_ignore_ascii_case("xml") && !body.contains("?>")
        }
        K::Element => element_is_balanced(raw),
    }
}

fn element_is_balanced(raw: &str) -> bool {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(raw);
    let cfg = r.config_mut();
    cfg.check_end_names = true;
    cfg.trim_text(false);
    let mut depth = 0usize;
    let mut roots = 0usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
            }
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Ok(Event::Empty(_)) => {
                if depth == 0 {
                    roots += 1;
                }
            }
            Ok(Event::Text(t)) => {
                if depth == 0 && !t.iter().all(u8::is_ascii_whitespace) {
                    return false;
                }
            }
            Ok(Event::Comment(_) | Event::PI(_) | Event::CData(_) | Event::GeneralRef(_)) => {
                if depth == 0 {
                    return false;
                }
            }
            Ok(Event::DocType(_) | Event::Decl(_)) => return false,
            Ok(Event::Eof) => return depth == 0 && roots == 1,
            Err(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::FragmentKind as K;

    #[test]
    fn escaping_covers_markup_and_drops_forbidden_characters() {
        let mut s = String::new();
        push_attr_escaped(&mut s, "a&b<c>\"d\te\u{1}f");
        assert_eq!(s, "a&amp;b&lt;c&gt;&quot;d&#9;ef");
        let mut t = String::new();
        push_text_escaped(&mut t, "x<y & z\u{FFFF}");
        assert_eq!(t, "x&lt;y &amp; z");
    }

    #[test]
    fn ncnames() {
        for ok in ["a", "_x", "a-b.c", "état", "x1"] {
            assert!(is_ncname(ok), "{ok}");
        }
        for bad in ["", "1a", "a:b", "-a", "a b", ".a"] {
            assert!(!is_ncname(bad), "{bad}");
        }
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        let v = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (i, o) in v {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn fragments_must_be_one_balanced_element_or_a_comment_or_a_pi() {
        assert!(fragment_is_well_formed(
            "<a:x xmlns:a='u'><y/>t</a:x>",
            K::Element
        ));
        assert!(fragment_is_well_formed("<x/>\n  ", K::Element));
        assert!(!fragment_is_well_formed("<x>", K::Element));
        assert!(!fragment_is_well_formed("<x/><y/>", K::Element));
        assert!(!fragment_is_well_formed("</g><script/>", K::Element));
        assert!(!fragment_is_well_formed("text", K::Element));
        assert!(!fragment_is_well_formed("<!DOCTYPE x><x/>", K::Element));
        assert!(fragment_is_well_formed("<!-- note -->", K::Comment));
        assert!(!fragment_is_well_formed("<!-- a -- b -->", K::Comment));
        assert!(!fragment_is_well_formed("<!-- x --><g>", K::Comment));
        assert!(fragment_is_well_formed(
            "<?app data?>",
            K::ProcessingInstruction
        ));
        assert!(!fragment_is_well_formed(
            "<?xml version='1.0'?>",
            K::ProcessingInstruction
        ));
    }
}
