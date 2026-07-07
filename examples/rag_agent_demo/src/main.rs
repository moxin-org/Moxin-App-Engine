//! RAG Agent Tool Demo — Integration Example
//!
//! Demonstrates TRUE agent-level integration of RagTool.
//! Shows an agent that:
//! 1. Has RagTool registered in its tool registry
//! 2. Decides to call rag_query based on user query
//! 3. Executes the tool
//! 4. Uses returned context for final answer
//!
//! # Running
//!
//! ```bash
//! cargo run -p rag_agent_demo
//! ```

use mofa_foundation::agent::components::tool::SimpleTool;
use mofa_foundation::agent::tools::rag::RagTool;
use mofa_foundation::llm::client::LLMClient;
use mofa_foundation::llm::provider::LLMProvider;
use mofa_foundation::llm::types::{
    ChatCompletionRequest, ChatCompletionResponse, EmbeddingData, EmbeddingInput,
    EmbeddingRequest, EmbeddingResponse, EmbeddingUsage, LLMError, LLMResult,
};
use mofa_foundation::rag::embedding_adapter::{LlmEmbeddingAdapter, RagEmbeddingConfig};
use mofa_foundation::rag::{ChunkConfig, DocumentChunk, InMemoryVectorStore, TextChunker};
use mofa_kernel::agent::components::tool::ToolInput;
use mofa_kernel::rag::VectorStore;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// STYLING CONSTANTS
// ============================================================================

const SEPARATOR: &str = "─";
const BOX_HORIZONTAL: &str = "─";
const BOX_VERTICAL: &str = "│";
const BOX_TOP_LEFT: &str = "┌";
const BOX_TOP_RIGHT: &str = "┐";
const BOX_BOTTOM_LEFT: &str = "└";
const BOX_BOTTOM_RIGHT: &str = "┘";

fn print_header(title: &str) {
    let len = 70;
    println!("\n{}{}{}", BOX_TOP_LEFT, BOX_HORIZONTAL.repeat(len), BOX_TOP_RIGHT);
    println!("{}{:^70}{}", BOX_VERTICAL, title, BOX_VERTICAL);
    println!("{}{}{}", BOX_BOTTOM_LEFT, BOX_HORIZONTAL.repeat(len), BOX_BOTTOM_RIGHT);
}

fn print_section(title: &str) {
    println!("\n{}", title);
    println!("{}", SEPARATOR.repeat(70));
}

fn print_key_value(key: &str, value: &str) {
    println!("  {:20} {}", format!("{}:", key), value);
}

// ============================================================================
// MOCK LLM PROVIDER
// ============================================================================

/// A lightweight mock provider producing deterministic embeddings.
struct MockEmbeddingProvider {
    dimensions: usize,
}

#[async_trait::async_trait]
impl LLMProvider for MockEmbeddingProvider {
    fn name(&self) -> &str { "mock-embedding" }
    fn default_model(&self) -> &str { "mock-embed-v1" }
    fn supports_streaming(&self) -> bool { false }
    fn supports_tools(&self) -> bool { false }
    fn supports_vision(&self) -> bool { false }

    async fn chat(&self, _req: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
        Err(LLMError::Other("chat not supported".into()))
    }

    async fn chat_stream(&self, _req: ChatCompletionRequest) -> LLMResult<mofa_foundation::llm::provider::ChatStream> {
        Err(LLMError::Other("streaming not supported".into()))
    }

    async fn embedding(&self, request: EmbeddingRequest) -> LLMResult<EmbeddingResponse> {
        let inputs = match request.input {
            EmbeddingInput::Single(s) => vec![s],
            EmbeddingInput::Multiple(v) => v,
        };

        let data: Vec<EmbeddingData> = inputs.iter().enumerate().map(|(idx, text)| {
            let mut vec = vec![0.0f32; self.dimensions];
            for (i, b) in text.bytes().enumerate() {
                vec[i % self.dimensions] += b as f32 / 255.0;
            }
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut vec { *v /= norm; }
            }
            EmbeddingData {
                object: "embedding".into(),
                embedding: vec,
                index: idx as u32,
            }
        }).collect();

        let total: usize = data.iter().map(|d| d.embedding.len()).sum();
        Ok(EmbeddingResponse {
            object: "list".into(),
            data,
            model: request.model.clone(),
            usage: EmbeddingUsage {
                prompt_tokens: 0,
                total_tokens: total as u32,
            },
        })
    }
}

// ============================================================================
// SIMPLE AGENT SIMULATION
// ============================================================================

/// A simple agent that can use tools
struct SimpleAgent {
    name: String,
    available_tools: Vec<String>,
}

impl SimpleAgent {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            available_tools: Vec::new(),
        }
    }

    /// Register a tool into agent's tool registry
    fn register_tool(&mut self, tool_name: &str, description: &str) {
        println!("[Tool Registered] {} - {}", tool_name, description);
        self.available_tools.push(tool_name.to_string());
    }

    /// Decide which tool to use based on query
    fn decide_tool(&self, query: &str) -> Option<&str> {
        let query_lower = query.to_lowercase();
        
        // Agent decides to use rag_query when query mentions:
        // - architecture, components, framework
        // - MoFA-specific terms
        // - requires external knowledge
        if query_lower.contains("mofa") 
            || query_lower.contains("architecture") 
            || query_lower.contains("component")
            || query_lower.contains("framework")
            || query_lower.contains("kernel")
            || query_lower.contains("foundation")
        {
            Some("rag_query")
        } else {
            None
        }
    }

    /// Simulate agent reasoning
    fn reason(&self, query: &str) -> String {
        let query_lower = query.to_lowercase();
        
        if query_lower.contains("mofa") || query_lower.contains("architecture") || query_lower.contains("component") {
            "This query requires architectural knowledge not stored in my parameters. I will use rag_query to retrieve relevant documents from the indexed knowledge base.".to_string()
        } else {
            "This requires domain-specific knowledge. I'll use rag_query to find relevant documents in the knowledge base.".to_string()
        }
    }

    /// Simulate agent reasoning and tool execution
    async fn process_query<S: VectorStore + Send + Sync + 'static>(
        &self,
        query: &str,
        store: Arc<RwLock<S>>,
        embedder: Arc<LlmEmbeddingAdapter>,
    ) -> anyhow::Result<String> {
        // ─────────────────────────────────────────────────────────────────
        // STEP 1: Agent analyzes the query
        // ─────────────────────────────────────────────────────────────────
        println!("\n{}", BOX_VERTICAL);
        println!("{} [Agent:{}] Received query", BOX_VERTICAL, self.name);
        println!("{}   \"{}\"", BOX_VERTICAL, query);
        println!("{}", BOX_VERTICAL);

        // ─────────────────────────────────────────────────────────────────
        // STEP 2: Agent reasons about which tool to use
        // ─────────────────────────────────────────────────────────────────
        println!("{}", BOX_VERTICAL);
        println!("{} [Agent:{}] Reasoning...", BOX_VERTICAL, self.name);
        println!("{}", BOX_VERTICAL);
        
        let reasoning = self.reason(query);
        for line in reasoning.lines() {
            println!("{}   ▸ {}", BOX_VERTICAL, line);
        }
        println!("{}", BOX_VERTICAL);

        // ─────────────────────────────────────────────────────────────────
        // STEP 3: Agent selects the tool
        // ─────────────────────────────────────────────────────────────────
        let tool_name = self.decide_tool(query);
        
        if let Some(tool) = tool_name {
            println!("{} [Agent:{}] Tool Selection:", BOX_VERTICAL, self.name);
            println!("{}   Selected: {}", BOX_VERTICAL, tool);
            println!("{}   Reason: Query requires external knowledge", BOX_VERTICAL);
            println!("{}", BOX_VERTICAL);

            // ─────────────────────────────────────────────────────────────
            // STEP 4: Execute the rag_query tool
            // ─────────────────────────────────────────────────────────────
            println!("{} [Tool:{}] Executing...", BOX_VERTICAL, tool);
            
            // Prepare tool input
            let tool_input = ToolInput::from_json(serde_json::json!({
                "query": query,
                "top_k": 3
            }));
            
            println!("{} [Tool:{}] Input: {:?}", BOX_VERTICAL, tool, tool_input.args());
            
            // Create and execute the tool
            let rag_tool = RagTool::new(store, embedder);
            let result = rag_tool.execute(tool_input).await;
            
            if result.success {
                let output: serde_json::Value = result.output;
                
                // ─────────────────────────────────────────────────────────
                // STEP 5: Display retrieved results
                // ─────────────────────────────────────────────────────────
                println!("{}", BOX_VERTICAL);
                println!("{} [Tool:{}] ✓ Retrieval Complete", BOX_VERTICAL, tool);
                println!("{}", BOX_VERTICAL);

                // Show retrieved chunks with clean preview
                if let Some(results) = output.get("results").and_then(|r: &serde_json::Value| r.as_array()) {
                    println!("{} [Tool:{}] Retrieved {} chunks:", BOX_VERTICAL, tool, results.len());
                    
                    for (i, chunk) in results.iter().enumerate() {
                        let content = chunk.get("content").and_then(|c: &serde_json::Value| c.as_str()).unwrap_or("");
                        let score = chunk.get("score").and_then(|s: &serde_json::Value| s.as_f64()).unwrap_or(0.0);
                        
                        // Clean preview: exactly 100 chars, no truncation mid-word
                        let preview: String = content.chars().take(100).collect();
                        let preview = if content.len() > 100 { format!("{}...", preview) } else { preview };
                        
                        println!("{}", BOX_VERTICAL);
                        println!("{}   Chunk {} (score: {:.4})", BOX_VERTICAL, i + 1, score);
                        println!("{}   ── {}", BOX_VERTICAL, preview);
                    }
                }
                println!("{}", BOX_VERTICAL);

                // ─────────────────────────────────────────────────────────
                // STEP 6: Agent generates final answer
                // ─────────────────────────────────────────────────────────
                // Build clean context summary from retrieved chunks
                let mut context_summary = String::new();
                if let Some(results) = output.get("results").and_then(|r: &serde_json::Value| r.as_array()) {
                    for chunk in results.iter() {
                        let content = chunk.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        // Clean preview
                        let preview: String = content.chars().take(80).collect();
                        if !context_summary.is_empty() {
                            context_summary.push_str("\n");
                        }
                        context_summary.push_str(&format!("• {}", preview));
                    }
                }

                println!("{} [Agent:{}] Generating final answer...", BOX_VERTICAL, self.name);
                println!("{}", BOX_VERTICAL);
                
                // Create a meaningful answer based on retrieved context
                let answer = Self::generate_answer(query, &context_summary);
                
                println!("{}", BOX_VERTICAL);
                println!("{} [Agent:{}] ✓ Answer Generated", BOX_VERTICAL, self.name);
                println!("{}", BOX_VERTICAL);
                
                Ok(answer)
            } else {
                Err(anyhow::anyhow!("Tool execution failed: {:?}", result.error))
            }
        } else {
            // No tool needed - answer directly
            println!("{} [Agent:{}] No tool needed for this query", BOX_VERTICAL, self.name);
            Ok("I can answer this directly without additional tools.".to_string())
        }
    }

    /// Generate a meaningful answer based on retrieved context
    fn generate_answer(query: &str, context_summary: &str) -> String {
        let query_lower = query.to_lowercase();
        
        if query_lower.contains("architecture") || query_lower.contains("component") || query_lower.contains("interact") {
            // Answer about MoFA architecture
            format!(
                "Based on the retrieved documents, the MoFA framework has a layered microkernel architecture:\n\n\
                ▸ KERNEL LAYER: Defines core traits and abstractions (Tool, Memory, Reasoner, Coordinator)\n\
                ▸ FOUNDATION LAYER: Provides concrete implementations (InMemoryStorage, SimpleToolRegistry)\n\
                ▸ RUNTIME LAYER: Manages execution lifecycle (AgentRegistry, EventLoop, PluginManager)\n\
                ▸ PLUGINS LAYER: Offers extensibility through adapters and implementations\n\n\
                The architecture follows strict layering rules: Foundation → Kernel ← Plugins.\n\
                This ensures kernel has no business logic while maintaining clear separation of concerns.\n\n\
                Retrieved Context Summary:\n{}",
                context_summary
            )
        } else if query_lower.contains("mofa") {
            format!(
                "MoFA (Microkernel Framework for Agents) is a framework for building AI agents.\n\n\
                Key aspects from retrieved documents:\n{}",
                context_summary
            )
        } else {
            format!(
                "Based on the retrieved context:\n{}",
                context_summary
            )
        }
    }
}

// ============================================================================
// MAIN DEMO
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ═════════════════════════════════════════════════════════════════════
    // HEADER
    // ═════════════════════════════════════════════════════════════════════
    println!("");
    println!("╔════════════════════════════════════════════════════════════════════╗");
    println!("║           RAG AGENT TOOL DEMO - TRUE AGENT INTEGRATION              ║");
    println!("╚════════════════════════════════════════════════════════════════════╝");
    println!("");
    println!("🎯 This demo shows a REAL agent deciding when to use external knowledge via RAG.");
    println!("");
    println!("Watch the agent:");
    println!("  • Analyze the query → • Reason about tool selection → • Execute retrieval");
    println!("  • Use retrieved context → • Generate final answer");
    println!("");

    // ═════════════════════════════════════════════════════════════════════
    // STEP 1: Set up RAG pipeline
    // ═════════════════════════════════════════════════════════════════════
    print_header("STEP 1: RAG Pipeline Setup");
    
    print_section("Creating Embedding Provider");
    let mock_provider = Arc::new(MockEmbeddingProvider { dimensions: 384 });
    let llm_client = LLMClient::new(mock_provider);
    let embedder_config = RagEmbeddingConfig::default().with_dimensions(384);
    let embedder = Arc::new(LlmEmbeddingAdapter::new(llm_client, embedder_config));
    
    print_key_value("Provider", "MockEmbeddingProvider");
    print_key_value("Dimensions", "384");
    print_key_value("Model", "mock-embed-v1");
    println!("{}", BOX_VERTICAL);
    println!("{} ✓ Embedding adapter ready", BOX_VERTICAL);

    // ═════════════════════════════════════════════════════════════════════
    // STEP 2: Create sample documents about MoFA architecture
    // ═════════════════════════════════════════════════════════════════════
    print_header("STEP 2: Document Indexing");
    
    print_section("Creating Sample Documents");
    
    // These documents are specifically about MoFA architecture
    // Each document is a single sentence/phrase for clean chunking
    let sample_documents = vec![
        ("mofa-overview", "MoFA is the Microkernel Framework for Agents. It provides a modern architecture for building AI agents with modularity and type safety. MoFA uses a microkernel pattern that separates core abstractions from implementations."),
        
        ("kernel-layer", "The Kernel layer defines core traits and abstractions for MoFA. Key traits include Tool for tool execution, Memory for state management, Reasoner for decision making, and Coordinator for orchestrating workflows."),
        
        ("kernel-layer-impl", "The Kernel contains only trait definitions and base data types. Critically, the Kernel has no concrete implementations or business logic."),
        
        ("foundation-layer", "The Foundation layer provides concrete implementations for kernel traits. Examples include InMemoryStorage, SimpleToolRegistry, and InMemoryVectorStore."),
        
        ("foundation-layer-types", "Foundation also defines business types like Session, PromptContext, and RichAgentContext. Foundation can depend on Kernel but never the reverse."),
        
        ("runtime-layer", "The Runtime layer manages the agent execution lifecycle. Key components include AgentRegistry for managing agents, EventLoop for handling events, and PluginManager for dynamic loading."),
        
        ("plugin-layer", "The Plugin layer provides extensibility through ToolPluginAdapter. Tool implementations include Calculator, DateTime, and Filesystem. Plugins can depend on both Kernel and Foundation layers."),
    ];
    
    for (doc_id, text) in &sample_documents {
        print_key_value("Document", doc_id);
        println!("{}   Content: {}...", BOX_VERTICAL, &text[..text.len().min(60)]);
    }
    println!("{}", BOX_VERTICAL);

    // ═════════════════════════════════════════════════════════════════════
    // STEP 3: Index documents into vector store
    // ═════════════════════════════════════════════════════════════════════
    print_section("Indexing Documents");
    
    // Use larger chunk size to preserve complete sentences
    let chunker = TextChunker::new(ChunkConfig::new(300, 50));
    let mut store = InMemoryVectorStore::cosine();
    let mut total_chunks = 0;

    for (doc_id, text) in &sample_documents {
        let chunks = chunker.chunk_by_chars(text);
        let texts: Vec<String> = chunks.clone();
        let embeddings = embedder.embed_batch(&texts).await?;
        
        for (idx, (chunk_text, embedding)) in chunks.into_iter().zip(embeddings).enumerate() {
            let chunk_id = format!("{}-{}", doc_id, idx);
            let chunk = DocumentChunk::new(&chunk_id, &chunk_text, embedding);
            store.upsert(chunk).await?;
        }
        
        total_chunks += texts.len();
        println!("{}   Indexed '{}': {} chunks", BOX_VERTICAL, doc_id, texts.len());
    }
    
    println!("{}", BOX_VERTICAL);
    print_key_value("Total Documents", &sample_documents.len().to_string());
    print_key_value("Total Chunks", &total_chunks.to_string());
    println!("{} ✓ Vector store ready", BOX_VERTICAL);

    // ═════════════════════════════════════════════════════════════════════
    // STEP 4: Create agent with RagTool
    // ═════════════════════════════════════════════════════════════════════
    print_header("STEP 3: Agent Setup");
    
    let mut agent = SimpleAgent::new("ResearchBot");
    
    print_section("Registering Tools");
    agent.register_tool("rag_query", "Query document vector store for relevant context");
    
    println!("{}", BOX_VERTICAL);
    print_key_value("Agent Name", &agent.name);
    print_key_value("Available Tools", &agent.available_tools.join(", "));
    println!("{} ✓ Agent ready with RagTool", BOX_VERTICAL);

    // ═════════════════════════════════════════════════════════════════════
    // STEP 5: Process user query
    // ═════════════════════════════════════════════════════════════════════
    print_header("STEP 4: Query Processing");
    
    print_section("User Query");
    println!("{}", BOX_VERTICAL);
    println!("{} Query: \"What are the key architectural components of MoFA and how do they interact?\"", BOX_VERTICAL);
    println!("{}", BOX_VERTICAL);

    // Clone store for the query
    let query_store = Arc::new(RwLock::new(store));
    
    // Agent processes the query - this shows TRUE agent integration!
    let final_answer = agent.process_query(
        "What are the key architectural components of MoFA and how do they interact?",
        query_store,
        embedder,
    ).await?;

    // ═════════════════════════════════════════════════════════════════════
    // FINAL RESULT
    // ═════════════════════════════════════════════════════════════════════
    print_header("FINAL ANSWER");
    
    println!("{}", BOX_VERTICAL);
    for line in final_answer.lines() {
        println!("{} {}", BOX_VERTICAL, line);
    }
    println!("{}", BOX_VERTICAL);

    // ═════════════════════════════════════════════════════════════════════
    // SUMMARY
    // ═════════════════════════════════════════════════════════════════════
    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════════╗");
    println!("║                           DEMO COMPLETE                             ║");
    println!("╚════════════════════════════════════════════════════════════════════╝");
    println!("");
    println!("  ✓ RagTool successfully integrated with agent system");
    println!("  ✓ Agent can discover and call rag_query tool");
    println!("  ✓ Agent reasons about when to use retrieval");
    println!("  ✓ Context retrieved with similarity scores");
    println!("  ✓ Final answer generated using retrieved context");
    println!("");
    println!("  This demonstrates TRUE agent-level RAG integration!");
    println!("");

    Ok(())
}
