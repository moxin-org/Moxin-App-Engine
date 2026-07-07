use futures::{StreamExt, future};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use mofa_foundation::rag::{
    ChunkConfig, IdentityReranker, InMemoryVectorStore, PassthroughStreamingGenerator, TextChunker,
};
use mofa_foundation::rag::{
    DocumentChunk, GenerateInput, GeneratorChunk, RagPipeline, ScoredDocument,
};
use mofa_kernel::VectorStore;
use mofa_kernel::agent::AgentResult;
use mofa_kernel::agent::error::AgentError;

// helper retriever copied from example
struct SimpleRetriever {
    store: InMemoryVectorStore,
    dimensions: usize,
}

impl SimpleRetriever {
    fn new(store: InMemoryVectorStore, dimensions: usize) -> Self {
        Self { store, dimensions }
    }
}

#[async_trait]
impl mofa_kernel::rag::Retriever for SimpleRetriever {
    async fn retrieve(
        &self,
        query: &str,
        top_k: usize,
    ) -> mofa_kernel::agent::error::AgentResult<Vec<ScoredDocument>> {
        let query_embedding = simple_embedding(query, self.dimensions);
        let results: Vec<_> = self.store.search(&query_embedding, top_k, None).await?;
        Ok(results
            .into_iter()
            .map(|r| {
                ScoredDocument::new(
                    mofa_foundation::rag::Document::new(&r.id, &r.text),
                    r.score,
                    Some("vector_search".to_string()),
                )
            })
            .collect())
    }
}
use async_trait::async_trait;
use mofa_foundation::llm::{LLMAgent, LLMAgentBuilder, LLMResult, OpenAIProvider};
use mofa_kernel::rag::{Generator, GeneratorChunk as KernelGeneratorChunk};

/// Helper to build a simple deterministic embedding.
fn simple_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    let mut embedding = vec![0.0_f32; dimensions];
    for (i, byte) in text.bytes().enumerate() {
        embedding[i % dimensions] += byte as f32 / 255.0;
    }
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut embedding {
            *x /= norm;
        }
    }
    embedding
}

/// Wrap a live LLMAgent as a RAG generator by concatenating query and context
/// and forwarding streaming chunks.
struct AgentGenerator {
    agent: Arc<LLMAgent>,
}

#[async_trait]
impl Generator for AgentGenerator {
    async fn generate(&self, input: &GenerateInput) -> AgentResult<String> {
        let prompt = format!(
            "{}\n\nContext:\n{}",
            input.query,
            input
                .context
                .iter()
                .map(|d| d.text.clone())
                .collect::<Vec<_>>()
                .join("\n")
        );
        let mut stream = self
            .agent
            .chat_stream(prompt)
            .await
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        let mut full = String::new();
        while let Some(res) = stream.next().await {
            let chunk = res.map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
            full.push_str(&chunk);
        }
        Ok(full)
    }

    async fn stream(
        &self,
        input: GenerateInput,
    ) -> AgentResult<
        Pin<Box<dyn futures::stream::Stream<Item = AgentResult<KernelGeneratorChunk>> + Send>>,
    > {
        let prompt = format!(
            "{}\n\nContext:\n{}",
            input.query,
            input
                .context
                .iter()
                .map(|d| d.text.clone())
                .collect::<Vec<_>>()
                .join("\n")
        );
        let stream = self
            .agent
            .chat_stream(prompt)
            .await
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        let mapped = stream.map(|r| {
            r.map(KernelGeneratorChunk::Text)
                .map_err(|e| AgentError::ExecutionFailed(e.to_string()))
        });
        Ok(Box::pin(mapped))
    }
}

/// Attempt to build an OpenAI-backed agent from environment. Returns None if key
/// not present.
fn maybe_openai_agent() -> Option<Arc<LLMAgent>> {
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        let provider = Arc::new(OpenAIProvider::new(key));
        let agent = LLMAgentBuilder::new()
            .with_id("rag-test-agent")
            .with_provider(provider.clone())
            .build();
        Some(Arc::new(agent))
    } else {
        None
    }
}

#[tokio::test]
async fn integration_real_llm_streaming() {
    if maybe_openai_agent().is_none() {
        eprintln!("SKIPPING integration_real_llm_streaming: OPENAI_API_KEY not set");
        return;
    }

    let agent = maybe_openai_agent().unwrap();

    // prepare a tiny in-memory store
    let mut store = InMemoryVectorStore::cosine();
    let dims = 16;
    let doc = DocumentChunk::new(
        "d1",
        "The sky is blue",
        simple_embedding("The sky is blue", dims),
    );
    store.upsert(doc).await.unwrap();

    let retriever = Arc::new(SimpleRetriever::new(store, dims));
    let reranker = Arc::new(IdentityReranker);
    let generator = Arc::new(AgentGenerator {
        agent: agent.clone(),
    });
    let pipeline = RagPipeline::new(retriever, reranker, generator);

    let start = Instant::now();

    let (docs, mut stream) = pipeline
        .run_streaming("What color is the sky?", 1)
        .await
        .unwrap();
    assert!(!docs.is_empty());

    let mut resp = String::new();
    while let Some(chunk) = stream.next().await {
        if let Ok(KernelGeneratorChunk::Text(text)) = chunk {
            resp.push_str(&text);
        }
    }

    let elapsed = start.elapsed();
    println!(
        "Real LLM streaming response ({} bytes) in {:?}: {}",
        resp.len(),
        elapsed,
        resp
    );
    assert!(resp.len() > 0);
}

#[tokio::test]
async fn integration_concurrent_streaming_load() {
    if maybe_openai_agent().is_none() {
        eprintln!("SKIPPING integration_concurrent_streaming_load: OPENAI_API_KEY not set");
        return;
    }
    let agent = maybe_openai_agent().unwrap();

    let mut store = InMemoryVectorStore::cosine();
    let dims = 16;
    for i in 0..100 {
        let text = format!("Doc #{} content", i);
        let emb = simple_embedding(&text, dims);
        store
            .upsert(DocumentChunk::new(&format!("d{}", i), &text, emb))
            .await
            .unwrap();
    }

    let retriever = Arc::new(SimpleRetriever::new(store, dims));
    let reranker = Arc::new(IdentityReranker);
    let generator = Arc::new(AgentGenerator {
        agent: agent.clone(),
    });
    let pipeline = RagPipeline::new(retriever, reranker, generator);

    let queries: Vec<_> = (0..5).map(|i| format!("Query {}?", i)).collect();

    let handles: Vec<_> = queries
        .into_iter()
        .map(|q| {
            let p = pipeline.clone();
            tokio::spawn(async move {
                let (_docs, mut s) = p.run_streaming(&q, 3).await.unwrap();
                let mut buf = String::new();
                while let Some(chunk) = s.next().await {
                    if let Ok(KernelGeneratorChunk::Text(text)) = chunk {
                        buf.push_str(&text);
                    }
                }
                buf
            })
        })
        .collect();

    let results = future::join_all(handles).await;
    for r in results {
        let txt = r.unwrap();
        println!("concurrent response len {}", txt.len());
        assert!(!txt.is_empty());
    }
}

#[tokio::test]
async fn integration_large_document_processing_performance() {
    // no API requirement for this one, just exercising retrieval
    let mut store = InMemoryVectorStore::cosine();
    let dims = 64;
    for i in 0..1000 {
        let text = "x".repeat(100);
        let emb = simple_embedding(&text, dims);
        store
            .upsert(DocumentChunk::new(&format!("d{}", i), &text, emb))
            .await
            .unwrap();
    }
    let retriever = Arc::new(SimpleRetriever::new(store, dims));
    let reranker = Arc::new(IdentityReranker);
    let generator = Arc::new(PassthroughStreamingGenerator::new(SimpleGenerator));
    let pipeline = RagPipeline::new(retriever, reranker, generator);

    let start = Instant::now();
    let (_docs, mut s) = pipeline.run_streaming("hello", 5).await.unwrap();
    while let Some(_) = s.next().await {}
    let elapsed = start.elapsed();
    println!("processed 1000 docs retrieval+stream in {:?}", elapsed);
    assert!(elapsed.as_secs() < 10);
}

// A simple generator used in the large-doc performance test
struct SimpleGenerator;

#[async_trait]
impl Generator for SimpleGenerator {
    async fn generate(&self, input: &GenerateInput) -> AgentResult<String> {
        Ok(format!("Q:{} ctx={}", input.query, input.context.len()))
    }
}

#[tokio::test]
async fn integration_error_recovery_stream() {
    // use a trivial in-memory store wrapper for retrieval
    let dims = 8;
    let retriever = Arc::new(SimpleRetriever::new(InMemoryVectorStore::cosine(), dims));
    let reranker = Arc::new(IdentityReranker);

    struct BrokenGen;
    #[async_trait]
    impl Generator for BrokenGen {
        async fn generate(&self, _input: &GenerateInput) -> AgentResult<String> {
            Ok("".to_string())
        }
        async fn stream(
            &self,
            _input: GenerateInput,
        ) -> AgentResult<
            Pin<Box<dyn futures::stream::Stream<Item = AgentResult<KernelGeneratorChunk>> + Send>>,
        > {
            let stream =
                futures::stream::once(async { Err(AgentError::ExecutionFailed("broken".into())) });
            Ok(Box::pin(stream))
        }
    }

    let pipeline = RagPipeline::new(retriever, reranker, Arc::new(BrokenGen));
    let (_docs, mut s) = pipeline.run_streaming("x", 1).await.unwrap();
    let first = s.next().await.unwrap();
    assert!(first.is_err());
}


#[tokio::test]
async fn test_stategraph_multi_turn() {
    use mofa_foundation::workflow::StateGraphImpl;
    use mofa_kernel::workflow::{
        Command, CompiledGraph, JsonState, NodeFunc, RuntimeContext, StateGraph, StreamEvent, END,
        START,
    };
    use serde_json::json;

    struct TurnNode {
        id: &'static str,
        turn: i64,
    }

    #[async_trait]
    impl NodeFunc<JsonState> for TurnNode {
        async fn call(
            &self,
            _state: &mut JsonState,
            _ctx: &RuntimeContext,
        ) -> AgentResult<Command> {
            Ok(Command::new()
                .update("turn", json!(self.turn))
                .continue_())
        }

        fn name(&self) -> &str {
            self.id
        }
    }

    let graph_id = "rag_integration_stategraph_multi_turn";
    let mut graph = StateGraphImpl::<JsonState>::new(graph_id);

    graph
        .add_node(
            "round1",
            Box::new(TurnNode {
                id: "round1",
                turn: 1,
            }),
        )
        .add_node(
            "round2",
            Box::new(TurnNode {
                id: "round2",
                turn: 2,
            }),
        )
        .add_node(
            "round3",
            Box::new(TurnNode {
                id: "round3",
                turn: 3,
            }),
        )
        .add_edge(START, "round1")
        .add_edge("round1", "round2")
        .add_edge("round2", "round3")
        .add_edge("round3", END);

    let compiled = graph.compile().expect("compile StateGraph");
    let config = Some(RuntimeContext::new(graph_id));
    let mut stream = compiled.stream(JsonState::new(), config);

    let mut chunks: Vec<StreamEvent<JsonState>> = vec![];
    while let Some(item) = stream.next().await {
        let event = item.expect("stream transport error");
        chunks.push(event);
        if chunks
            .iter()
            .filter(|e| matches!(e, StreamEvent::NodeEnd { .. }))
            .count()
            >= 3
        {
            break;
        }
    }

    let node_ends = chunks
        .iter()
        .filter(|e| matches!(e, StreamEvent::NodeEnd { .. }))
        .count();
    assert_eq!(
        node_ends, 3,
        "expected three NodeEnd events (three turns); stream halted early"
    );
}
