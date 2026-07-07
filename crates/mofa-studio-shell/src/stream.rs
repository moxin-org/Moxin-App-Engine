use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

/// Represents a chunk of data streamed from an LLM provider.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// A single token from the model.
    Token(String),
    /// A tool call from the model.
    ToolCall {
        /// Unique identifier for this tool call.
        id: String,
        /// Name of the tool to invoke.
        name: String,
        /// JSON string containing arguments for the tool.
        args: String,
    },
    /// Indicates the stream has completed.
    Done,
    /// An error occurred during streaming.
    Error(String),
}

/// Trait for reading chunks from an LLM provider stream.
///
/// Implementors must be `Send + 'static` to allow use in async contexts.
pub trait MofaStream: Send + 'static {
    /// Asynchronously retrieve the next chunk from the stream.
    ///
    /// Returns `Some(StreamChunk)` if a chunk was successfully read or parsed,
    /// `None` if the stream has reached EOF.
    async fn next_chunk(&mut self) -> Option<StreamChunk>;
}

/// Stream reader for Server-Sent Events (SSE) format from LLM providers.
///
/// Reads lines from a TCP stream, parses SSE format (lines starting with "data:"),
/// and converts them to `StreamChunk` variants.
pub struct SseStream {
    reader: BufReader<TcpStream>,
}

impl SseStream {
    /// Creates a new SSE stream from a TCP connection.
    pub fn new(stream: TcpStream) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }
}

impl MofaStream for SseStream {
    async fn next_chunk(&mut self) -> Option<StreamChunk> {
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line).await {
                Ok(0) => return None,
                Ok(_) => {
                    let trimmed = line.trim();

                    if !trimmed.starts_with("data:") {
                        continue;
                    }

                    let data = match trimmed.strip_prefix("data:") {
                        Some(v) => v.trim(),
                        None => continue,
                    };

                    if data == "[DONE]" {
                        tracing::debug!("parsed chunk: done");
                        return Some(StreamChunk::Done);
                    }

                    match serde_json::from_str::<serde_json::Value>(data) {
                        Ok(json_value) => {
                            let token = json_value
                                .get("choices")
                                .and_then(|c| c.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|choice| choice.get("delta"))
                                .and_then(|delta| delta.get("content"))
                                .and_then(|content| content.as_str())
                                .unwrap_or_default()
                                .to_string();
                            tracing::debug!(token = %token, "parsed chunk: token");
                            return Some(StreamChunk::Token(token));
                        }
                        Err(err) => {
                            tracing::debug!(error = %err, "parsed chunk: error");
                            return Some(StreamChunk::Error(err.to_string()));
                        }
                    }
                }
                Err(err) => {
                    tracing::debug!(error = %err, "parsed chunk: error");
                    return Some(StreamChunk::Error(err.to_string()));
                }
            }
        }
    }
}

/// A mock stream for testing purposes.
///
/// Contains a pre-populated vector of `StreamChunk` items that are returned
/// in order by successive calls to `next_chunk()`.
pub struct MockStream {
    chunks: Vec<StreamChunk>,
    index: usize,
}

impl MockStream {
    /// Creates a new mock stream with the given chunks.
    pub fn new(chunks: Vec<StreamChunk>) -> Self {
        Self { chunks, index: 0 }
    }
}

impl MofaStream for MockStream {
    async fn next_chunk(&mut self) -> Option<StreamChunk> {
        if self.chunks.is_empty() {
            return None;
        }

        self.index += 1;
        Some(self.chunks.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_stream() {
        let chunks = vec![
            StreamChunk::Token("hello".to_string()),
            StreamChunk::Token(" world".to_string()),
            StreamChunk::Done,
        ];

        let mut stream = MockStream::new(chunks);

        // Assert first token
        match stream.next_chunk().await {
            Some(StreamChunk::Token(content)) => assert_eq!(content, "hello"),
            _ => panic!("Expected Token(hello)"),
        }

        // Assert second token
        match stream.next_chunk().await {
            Some(StreamChunk::Token(content)) => assert_eq!(content, " world"),
            _ => panic!("Expected Token( world)"),
        }

        // Assert done
        match stream.next_chunk().await {
            Some(StreamChunk::Done) => (),
            _ => panic!("Expected Done"),
        }

        // Assert stream exhausted
        match stream.next_chunk().await {
            None => (),
            _ => panic!("Expected None"),
        }
    }
}
