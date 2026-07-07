//! BM25 Index implementation
//!
//! Provides an in-memory BM25 index for sparse document retrieval.

use std::collections::{HashMap, HashSet};

/// BM25 index for sparse retrieval.
///
/// Uses the BM25 ranking function to score documents against queries.
///
/// BM25 formula:
/// score = idf * ((tf * (k1 + 1)) / (tf + k1 * (1 - b + b * dl / avgdl)))
///
/// Parameters:
/// - k1: term frequency saturation parameter (default: 1.5)
/// - b: document length normalization parameter (default: 0.75)
#[derive(Debug, Clone)]
pub struct Bm25Index {
    /// Document storage: id -> text
    documents: HashMap<String, String>,
    /// Term frequency per document: doc_id -> {term -> frequency}
    term_freqs: HashMap<String, HashMap<String, usize>>,
    /// Document lengths
    doc_lengths: HashMap<String, usize>,
    /// Average document length
    avg_doc_length: f64,
    /// Number of documents
    num_docs: usize,
    /// IDF scores: term -> idf
    idf: HashMap<String, f64>,
    /// BM25 parameter k1
    k1: f64,
    /// BM25 parameter b
    b: f64,
    /// Stop words to ignore
    stop_words: HashSet<String>,
}

impl Bm25Index {
    /// Create a new BM25 index with default parameters.
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            term_freqs: HashMap::new(),
            doc_lengths: HashMap::new(),
            avg_doc_length: 0.0,
            num_docs: 0,
            idf: HashMap::new(),
            k1: 1.5,
            b: 0.75,
            stop_words: default_stop_words(),
        }
    }

    /// Create a new BM25 index with custom parameters.
    pub fn with_params(k1: f64, b: f64) -> Self {
        Self {
            documents: HashMap::new(),
            term_freqs: HashMap::new(),
            doc_lengths: HashMap::new(),
            avg_doc_length: 0.0,
            num_docs: 0,
            idf: HashMap::new(),
            k1,
            b,
            stop_words: default_stop_words(),
        }
    }

    /// Index a single document.
    pub fn index_document(&mut self, id: impl Into<String>, text: impl Into<String>) {
        let id = id.into();
        let text = text.into();

        // Tokenize the document
        let tokens = self.tokenize(&text);
        let doc_length = tokens.len();

        // Calculate term frequencies
        let mut term_freq: HashMap<String, usize> = HashMap::new();
        for token in &tokens {
            *term_freq.entry(token.clone()).or_insert(0) += 1;
        }

        // Store document and term frequencies
        self.documents.insert(id.clone(), text);
        self.term_freqs.insert(id.clone(), term_freq);
        self.doc_lengths.insert(id.clone(), doc_length);

        // Update document count and average length
        self.num_docs += 1;
        let total_length: usize = self.doc_lengths.values().sum();
        self.avg_doc_length = total_length as f64 / self.num_docs as f64;

        // Recalculate IDF for all terms
        self.recalculate_idf();
    }

    /// Index multiple documents at once.
    pub fn index_documents(
        &mut self,
        documents: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) {
        for (id, text) in documents {
            let id = id.into();
            let text = text.into();

            let tokens = self.tokenize(&text);
            let doc_length = tokens.len();

            let mut term_freq: HashMap<String, usize> = HashMap::new();
            for token in &tokens {
                *term_freq.entry(token.clone()).or_insert(0) += 1;
            }

            self.documents.insert(id.clone(), text);
            self.term_freqs.insert(id.clone(), term_freq);
            self.doc_lengths.insert(id.clone(), doc_length);
        }

        // Update statistics
        self.num_docs = self.documents.len();
        let total_length: usize = self.doc_lengths.values().sum();
        self.avg_doc_length = if self.num_docs > 0 {
            total_length as f64 / self.num_docs as f64
        } else {
            0.0
        };

        self.recalculate_idf();
    }

    /// Tokenize text into terms.
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .filter(|s| !self.stop_words.contains(s))
            .collect()
    }

    /// Recalculate IDF scores for all terms in the index.
    fn recalculate_idf(&mut self) {
        // Collect all unique terms
        let mut all_terms: HashSet<String> = HashSet::new();
        for term_freqs in self.term_freqs.values() {
            for term in term_freqs.keys() {
                all_terms.insert(term.clone());
            }
        }

        // Calculate IDF for each term
        for term in all_terms {
            let doc_count = self
                .term_freqs
                .values()
                .filter(|tf| tf.contains_key(&term))
                .count();

            // IDF formula: log((N - n + 0.5) / (n + 0.5) + 1)
            // Using a smoothed version to avoid division by zero
            let idf = if self.num_docs > doc_count {
                let n = doc_count as f64;
                let n_docs = self.num_docs as f64;
                ((n_docs - n + 0.5) / (n + 0.5) + 1.0).ln()
            } else {
                0.0
            };

            self.idf.insert(term, idf);
        }
    }

    /// Get the BM25 score for a document against a query.
    pub fn score(&self, doc_id: &str, query: &str) -> f64 {
        let query_terms = self.tokenize(query);
        let term_freqs = match self.term_freqs.get(doc_id) {
            Some(tf) => tf,
            None => return 0.0,
        };

        let doc_length = *self.doc_lengths.get(doc_id).unwrap_or(&1);

        let mut score = 0.0;
        for term in query_terms {
            let tf = *term_freqs.get(&term).unwrap_or(&0);
            let idf = *self.idf.get(&term).unwrap_or(&0.0);

            if tf > 0 {
                // BM25 scoring formula
                let numerator = tf as f64 * (self.k1 + 1.0);
                let denominator = tf as f64
                    + self.k1
                        * (1.0 - self.b
                            + self.b * doc_length as f64 / self.avg_doc_length.max(1.0));
                score += idf * (numerator / denominator);
            }
        }

        score
    }

    /// Get the number of indexed documents.
    pub fn num_docs(&self) -> usize {
        self.num_docs
    }

    /// Get a document by ID.
    pub fn get_document(&self, id: &str) -> Option<&String> {
        self.documents.get(id)
    }

    /// Get all document IDs.
    pub fn document_ids(&self) -> Vec<&String> {
        self.documents.keys().collect()
    }

    /// Clear all indexed documents.
    pub fn clear(&mut self) {
        self.documents.clear();
        self.term_freqs.clear();
        self.doc_lengths.clear();
        self.idf.clear();
        self.num_docs = 0;
        self.avg_doc_length = 0.0;
    }
}

impl Default for Bm25Index {
    fn default() -> Self {
        Self::new()
    }
}

/// Get default English stop words.
fn default_stop_words() -> HashSet<String> {
    vec![
        "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "he", "in", "is",
        "it", "its", "of", "on", "that", "the", "to", "was", "were", "will", "with", "the", "this",
        "but", "they", "have", "had", "what", "when", "where", "who", "which", "why", "how",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_documents() {
        let mut index = Bm25Index::new();
        index.index_documents(vec![
            ("doc1", "Rust is a systems programming language"),
            ("doc2", "Python is great for machine learning"),
        ]);

        assert_eq!(index.num_docs(), 2);
        assert!(index.get_document("doc1").is_some());
        assert!(index.get_document("doc2").is_some());
    }

    #[test]
    fn test_tokenize() {
        let index = Bm25Index::new();
        let tokens = index.tokenize("Hello, World! This is a TEST.");

        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // Stop words should be removed
        assert!(!tokens.contains(&"is".to_string()));
    }

    #[test]
    fn test_bm25_scoring() {
        let mut index = Bm25Index::new();
        index.index_documents(vec![
            ("doc1", "Rust is a systems programming language"),
            ("doc2", "Python is great for machine learning"),
        ]);

        // Query for "systems programming" - should match doc1 better
        let score1 = index.score("doc1", "systems programming");
        let score2 = index.score("doc2", "systems programming");

        assert!(
            score1 > score2,
            "doc1 should score higher for systems programming query"
        );
    }

    #[test]
    fn test_empty_index() {
        let index = Bm25Index::new();
        assert_eq!(index.num_docs(), 0);
        assert!(index.document_ids().is_empty());
    }
}
