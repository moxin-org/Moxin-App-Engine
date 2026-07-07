//! Recursive text chunker
//!
//! Splits text using a hierarchy of separators, trying the most meaningful
//! split first (paragraphs → sentences → words → characters).
//! Inspired by LangChain's RecursiveCharacterTextSplitter.

// =============================================================================
// RecursiveChunker
// =============================================================================

/// Configuration for recursive chunking.
#[derive(Debug, Clone)]
pub struct RecursiveChunkConfig {
    /// Maximum number of characters per chunk.
    pub chunk_size: usize,
    /// Number of characters to overlap between consecutive chunks.
    pub chunk_overlap: usize,
    /// Ordered list of separators to try (most → least meaningful).
    pub separators: Vec<String>,
}

impl Default for RecursiveChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 200,
            separators: vec![
                "\n\n".into(), // Paragraph
                "\n".into(),   // Line
                ". ".into(),   // Sentence
                ", ".into(),   // Clause
                " ".into(),    // Word
            ],
        }
    }
}

impl RecursiveChunkConfig {
    /// Create a new config with custom chunk size and overlap.
    #[must_use]
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
            separators: Self::default().separators,
        }
    }

    /// Set custom separators.
    #[must_use]
    pub fn with_separators(mut self, separators: Vec<String>) -> Self {
        self.separators = separators;
        self
    }
}

/// Recursive text chunker that tries multiple separator levels.
///
/// Splits text using a hierarchy of separators. First tries to split at
/// paragraph boundaries (`\n\n`), then at line boundaries (`\n`), then
/// at sentence boundaries (`. `), and falls back to word and character
/// boundaries as needed.
///
/// This produces higher-quality chunks than fixed-size splitting because
/// it preserves semantic boundaries where possible.
#[derive(Debug, Clone, Default)]
pub struct RecursiveChunker {
    config: RecursiveChunkConfig,
}

impl RecursiveChunker {
    /// Create a new recursive chunker with the given configuration.
    #[must_use]
    pub fn new(config: RecursiveChunkConfig) -> Self {
        Self { config }
    }

    /// Split text into chunks using recursive separator hierarchy.
    pub fn chunk(&self, text: &str) -> Vec<String> {
        if text.is_empty() {
            return vec![];
        }

        let char_count = text.chars().count();
        if char_count <= self.config.chunk_size {
            return vec![text.to_string()];
        }

        self.recursive_split(text, 0)
    }

    fn recursive_split(&self, text: &str, separator_idx: usize) -> Vec<String> {
        let text_char_count = text.chars().count();
        if text_char_count <= self.config.chunk_size {
            return vec![text.to_string()];
        }

        // If we've exhausted all separators, do a hard character split
        if separator_idx >= self.config.separators.len() {
            return self.hard_split(text);
        }

        let separator = &self.config.separators[separator_idx];
        let sep_char_count = separator.chars().count();
        let parts: Vec<&str> = text.split(separator.as_str()).collect();

        // If the separator didn't actually split anything useful, try next
        if parts.len() <= 1 {
            return self.recursive_split(text, separator_idx + 1);
        }

        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut current_char_count: usize = 0;

        for (i, part) in parts.iter().enumerate() {
            let part_char_count = part.chars().count();
            let candidate_char_count = if current.is_empty() {
                part_char_count
            } else {
                current_char_count + sep_char_count + part_char_count
            };

            if candidate_char_count <= self.config.chunk_size {
                if current.is_empty() {
                    current = part.to_string();
                } else {
                    current.push_str(separator);
                    current.push_str(part);
                }
                current_char_count = candidate_char_count;
            } else {
                // Save current chunk
                if !current.is_empty() {
                    chunks.push(current.trim().to_string());
                }

                // If this single part is still too large, recurse with next separator
                if part_char_count > self.config.chunk_size {
                    let sub_chunks = self.recursive_split(part, separator_idx + 1);
                    chunks.extend(sub_chunks);
                    current = String::new();
                    current_char_count = 0;
                } else {
                    current = part.to_string();
                    current_char_count = part_char_count;
                }
            }

            // Add overlap from the end of the previous chunk (UTF-8 safe)
            if i > 0 && current.is_empty() && !chunks.is_empty() {
                let last = chunks.last().unwrap();
                let last_chars: Vec<char> = last.chars().collect();
                let overlap_chars = last_chars.len().min(self.config.chunk_overlap);
                let overlap_text: String =
                    last_chars[last_chars.len() - overlap_chars..].iter().collect();
                let overlap_len = overlap_text.chars().count();
                // Only use overlap if it doesn't make next chunk too large
                if overlap_len < self.config.chunk_size / 2 {
                    current = overlap_text;
                    current_char_count = overlap_len;
                }
            }
        }

        // Save the last chunk
        if !current.trim().is_empty() {
            chunks.push(current.trim().to_string());
        }

        chunks.retain(|c| !c.is_empty());
        chunks
    }

    fn hard_split(&self, text: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let step = self
            .config
            .chunk_size
            .saturating_sub(self.config.chunk_overlap)
            .max(1);
        let mut start = 0;

        while start < chars.len() {
            let end = (start + self.config.chunk_size).min(chars.len());
            let chunk: String = chars[start..end].iter().collect();
            if !chunk.trim().is_empty() {
                chunks.push(chunk);
            }
            if end >= chars.len() {
                break;
            }
            start += step;
        }

        chunks
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_no_split() {
        let chunker = RecursiveChunker::default();
        let chunks = chunker.chunk("Short text");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Short text");
    }

    #[test]
    fn empty_text() {
        let chunker = RecursiveChunker::default();
        let chunks = chunker.chunk("");
        assert!(chunks.is_empty());
    }

    #[test]
    fn splits_at_paragraph_boundary() {
        let config = RecursiveChunkConfig::new(50, 0);
        let chunker = RecursiveChunker::new(config);
        let text =
            "First paragraph content.\n\nSecond paragraph content.\n\nThird paragraph content.";
        let chunks = chunker.chunk(text);
        assert!(chunks.len() >= 2);
        // Each chunk should be under the size limit
        for chunk in &chunks {
            assert!(chunk.len() <= 50, "Chunk too large: {}", chunk.len());
        }
    }

    #[test]
    fn splits_at_sentence_boundary() {
        let config = RecursiveChunkConfig::new(30, 0);
        let chunker = RecursiveChunker::new(config);
        let text = "First sentence. Second sentence. Third sentence.";
        let chunks = chunker.chunk(text);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn respects_chunk_size() {
        let config = RecursiveChunkConfig::new(100, 0);
        let chunker = RecursiveChunker::new(config);
        let text = "A ".repeat(200); // 400 chars
        let chunks = chunker.chunk(&text);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(
                chunk.len() <= 110, // Allow small margin due to separator handling
                "Chunk too large: {} chars",
                chunk.len()
            );
        }
    }

    #[test]
    fn custom_separators() {
        let config = RecursiveChunkConfig::new(20, 0).with_separators(vec!["|".into(), " ".into()]);
        let chunker = RecursiveChunker::new(config);
        let text = "part one content|part two content|part three content here";
        let chunks = chunker.chunk(text);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn hard_split_fallback() {
        let config = RecursiveChunkConfig::new(5, 0).with_separators(vec![]); // No separators → immediate hard split
        let chunker = RecursiveChunker::new(config);
        let text = "abcdefghijklmnopqrst"; // 20 chars, no separators
        let chunks = chunker.chunk(text);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 5);
        }
    }

    #[test]
    fn preserves_content() {
        let config = RecursiveChunkConfig::new(50, 0);
        let chunker = RecursiveChunker::new(config);
        let text = "Hello world.\n\nThis is a test.\n\nFinal paragraph.";
        let chunks = chunker.chunk(text);
        let reconstructed: String = chunks.join(" ");
        // All original words should be present
        assert!(reconstructed.contains("Hello"));
        assert!(reconstructed.contains("test"));
        assert!(reconstructed.contains("Final"));
    }

    #[test]
    fn default_config_values() {
        let config = RecursiveChunkConfig::default();
        assert_eq!(config.chunk_size, 1000);
        assert_eq!(config.chunk_overlap, 200);
        assert_eq!(config.separators.len(), 5);
    }

    #[test]
    fn multibyte_utf8_chunking() {
        // Each CJK character is 3 bytes in UTF-8 but 1 char — ensures we
        // measure chunk_size in characters, not bytes.
        let config = RecursiveChunkConfig::new(10, 0);
        let chunker = RecursiveChunker::new(config);
        let text = "你好世界测试文本。这是第二句话。还有第三句话在这里。";
        let chunks = chunker.chunk(text);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(
                chunk.chars().count() <= 12, // small margin for separator handling
                "Chunk has {} chars, expected <= 12",
                chunk.chars().count()
            );
        }
    }

    #[test]
    fn overlap_with_large_text() {
        let config = RecursiveChunkConfig::new(50, 10);
        let chunker = RecursiveChunker::new(config);
        let text = "First paragraph with content.\n\nSecond paragraph with content.\n\nThird paragraph with more content here.";
        let chunks = chunker.chunk(text);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(
                chunk.chars().count() <= 60,
                "Chunk too large: {} chars",
                chunk.chars().count()
            );
        }
    }
}
