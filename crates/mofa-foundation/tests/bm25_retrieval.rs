//! BM25 Retrieval Integration Tests

use mofa_foundation::rag::{Bm25Retriever, Retriever};

/// Test indexing documents.
#[tokio::test]
async fn test_indexing_documents() {
    let mut retriever = Bm25Retriever::new();

    retriever.index_documents(vec![
        ("doc1", "Rust is a systems programming language"),
        ("doc2", "Python is great for machine learning"),
    ]);

    assert_eq!(retriever.num_docs(), 2);
}

/// Test query retrieval.
#[tokio::test]
async fn test_query_retrieval() {
    let mut retriever = Bm25Retriever::new();

    retriever.index_documents(vec![
        ("doc1", "Rust is a systems programming language"),
        ("doc2", "Python is great for machine learning"),
    ]);

    let results = retriever.retrieve("systems programming", 2).await.unwrap();

    assert!(!results.is_empty());
    assert_eq!(results[0].document.id, "doc1");
}

/// Test ranking correctness.
#[tokio::test]
async fn test_ranking_correctness() {
    let mut retriever = Bm25Retriever::new();

    retriever.index_documents(vec![
        (
            "rust_doc",
            "Rust is a systems programming language that focuses on performance",
        ),
        (
            "python_doc",
            "Python is great for machine learning and data science",
        ),
        ("web_doc", "JavaScript is a web programming language"),
    ]);

    // Query for "systems programming" - should rank rust_doc highest
    let results = retriever.retrieve("systems programming", 3).await.unwrap();

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].document.id, "rust_doc");
    assert!(results[0].score >= results[1].score);
}

/// Test top_k filtering.
#[tokio::test]
async fn test_top_k_filtering() {
    let mut retriever = Bm25Retriever::new();

    retriever.index_documents(vec![
        ("doc1", "Rust programming language"),
        ("doc2", "Python programming language"),
        ("doc3", "JavaScript programming language"),
        ("doc4", "Go programming language"),
        ("doc5", "Java programming language"),
    ]);

    let results = retriever.retrieve("programming language", 2).await.unwrap();

    assert_eq!(results.len(), 2);
}
