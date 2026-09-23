//! The `<defs>` registry: passes 6 and 7 of `research/06 §4.5.1`.
//!
//! Auxiliary elements — gradients, masks, clip paths, patterns — get short
//! ids derived from a hash of their content, so two identical definitions
//! collapse into one and an unchanged definition keeps its id from save to
//! save.

use std::collections::HashMap;

/// Collects definitions, deduplicated by content.
#[derive(Debug, Default)]
pub struct Defs {
    /// Element text, in first-use order: what goes inside `<defs>`.
    items: Vec<String>,
    /// Content key → id.
    by_content: HashMap<String, String>,
    /// Ids already handed out, to extend a hash prefix on collision.
    taken: HashMap<String, ()>,
    /// Requests that found an identical definition.
    pub hits: usize,
}

impl Defs {
    /// Registers a definition and returns its id.
    ///
    /// `element` is the tag name; `body` is everything after the tag name
    /// and before the end of the element, **without** an `id`: its
    /// attributes, `>`, children and the end tag (or `/>`). `prefix` is the
    /// one-letter id prefix (`g` for gradients, `m` for masks, …).
    pub fn add(&mut self, prefix: char, element: &str, body: &str) -> String {
        let key = format!("{element}{body}");
        if let Some(id) = self.by_content.get(&key) {
            self.hits += 1;
            return id.clone();
        }
        let hex = blake3::hash(key.as_bytes()).to_hex();
        let mut len = 4usize;
        let id = loop {
            let candidate = format!("{prefix}{}", hex.get(..len).unwrap_or(hex.as_str()));
            if !self.taken.contains_key(&candidate) || len >= hex.len() {
                break candidate;
            }
            len += 2;
        };
        self.taken.insert(id.clone(), ());
        self.items.push(format!("<{element} id=\"{id}\"{body}"));
        self.by_content.insert(key, id.clone());
        id
    }

    /// Whether nothing was registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many distinct definitions there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// The definitions, in first-use order.
    pub fn items(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_definitions_share_one_id() {
        let mut d = Defs::default();
        let a = d.add('g', "linearGradient", " x1=\"0\"/>");
        let b = d.add('g', "linearGradient", " x1=\"0\"/>");
        let c = d.add('g', "linearGradient", " x1=\"1\"/>");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(d.len(), 2);
        assert_eq!(d.hits, 1);
        assert!(a.starts_with('g') && a.len() == 5);
    }
}
