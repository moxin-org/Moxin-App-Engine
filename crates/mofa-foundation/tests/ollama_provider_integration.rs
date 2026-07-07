use mofa_foundation::llm::OllamaProvider;

#[test]
#[ignore] // only runs if Ollama is running locally
fn test_ollama_provider_connects() {
    let p = OllamaProvider::new();
    let _ = p;
}
