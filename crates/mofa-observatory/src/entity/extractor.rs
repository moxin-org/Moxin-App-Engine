use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// An entity extracted from text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Entity type: "URL", "DATE", "NUMBER", "QUOTED", "PROPER_NOUN".
    pub kind: String,
    /// The matched text.
    pub value: String,
    /// Byte offset where the match starts.
    pub start: usize,
    /// Byte offset where the match ends.
    pub end: usize,
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://[^\s<>]+").unwrap())
}
fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(\d{4}-\d{2}-\d{2}|\d{1,2}/\d{1,2}/\d{2,4})\b").unwrap()
    })
}
fn number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d+(?:[.,]\d+)*\b").unwrap())
}
fn quoted_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#""[^"]{1,200}""#).unwrap())
}
fn proper_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b[A-Z][a-z]{2,}\b").unwrap())
}

/// Extract named entities from `text` using regex patterns.
pub fn extract_entities(text: &str) -> Vec<Entity> {
    let mut entities = Vec::new();

    for m in url_re().find_iter(text) {
        entities.push(Entity {
            kind: "URL".to_string(),
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
        });
    }
    for m in date_re().find_iter(text) {
        entities.push(Entity {
            kind: "DATE".to_string(),
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
        });
    }
    for m in number_re().find_iter(text) {
        entities.push(Entity {
            kind: "NUMBER".to_string(),
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
        });
    }
    for m in quoted_re().find_iter(text) {
        entities.push(Entity {
            kind: "QUOTED".to_string(),
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
        });
    }
    for m in proper_re().find_iter(text) {
        // Skip if already captured as part of a URL
        let already = entities.iter().any(|e| e.start <= m.start() && m.end() <= e.end);
        if !already {
            entities.push(Entity {
                kind: "PROPER_NOUN".to_string(),
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
    }

    // Sort by start position
    entities.sort_by_key(|e| e.start);
    entities
}
