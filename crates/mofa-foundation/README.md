# mofa-foundation

MoFA Foundation - Core building blocks and utilities

## Installation

```toml
[dependencies]
mofa-foundation = "0.1"
```

## Features

- LLM integration (OpenAI, Ollama, Cohere\* providers)
- RAG pipeline with pluggable embedding backends
- Agent abstractions and implementations
- Persistence layer (PostgreSQL, MySQL, SQLite support)
- Actor-based execution using Ractor
- ReAct agent pattern implementation
- Rich agent context and session management
- Memory and reasoner components

\* Cohere support requires the `cohere` feature flag: `mofa-foundation = { version = "0.1", features = ["cohere"] }`

## Documentation

- [API Documentation](https://docs.rs/mofa-foundation)
- [Main Repository](https://github.com/mofa-org/mofa)

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
