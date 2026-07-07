//! Inference Bridge Module
//!
//! This module provides the bridge between the Gateway's OpenAI-compatible
//! endpoints and the InferenceOrchestrator in mofa-foundation.
//!
//! Architecture:
//! ```text
//! Client Request (OpenAI format)
//!     ↓
//! InferenceBridge
//!     ↓
//! InferenceOrchestrator
//!     ↓
//! Routing Policy → Model Pool → Provider
//!     ↓
//! Response (OpenAI format)
//! ```

use crate::error::GatewayError;
use mofa_foundation::inference::{
    InferenceOrchestrator, InferenceRequest, InferenceResult, OrchestratorConfig, Precision,
    RequestPriority,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// OpenAI-compatible chat completion request
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model identifier (e.g., "gpt-4", "llama-3-13b")
    pub model: String,
    /// List of messages
    pub messages: Vec<Message>,
    /// Optional: max tokens to generate
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Optional: temperature for sampling
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Optional: streaming response
    #[serde(default)]
    pub stream: Option<bool>,
}

/// OpenAI message structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    /// Role (system, user, assistant)
    pub role: String,
    /// Message content
    pub content: String,
}

/// OpenAI-compatible chat completion response
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionResponse {
    /// Response ID
    pub id: String,
    /// Object type
    #[serde(rename = "object")]
    pub object_type: String,
    /// Created timestamp
    pub created: u64,
    /// Model used
    pub model: String,
    /// Choices (generated responses)
    pub choices: Vec<Choice>,
    /// Usage information
    pub usage: Usage,
}

/// Choice in the response
#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    /// Index
    pub index: u32,
    /// Generated message
    pub message: Message,
    /// Finish reason
    pub finish_reason: String,
}

/// Token usage information
#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    /// Tokens in prompt
    pub prompt_tokens: u32,
    /// Tokens in completion
    pub completion_tokens: u32,
    /// Total tokens
    pub total_tokens: u32,
}

/// Inference Bridge - connects Gateway to InferenceOrchestrator
pub struct InferenceBridge {
    /// The inference orchestrator
    orchestrator: Arc<Mutex<InferenceOrchestrator>>,
}

impl InferenceBridge {
    /// Create a new InferenceBridge with the given configuration
    pub fn new(config: OrchestratorConfig) -> Self {
        let orchestrator = InferenceOrchestrator::new(config);
        Self {
            orchestrator: Arc::new(Mutex::new(orchestrator)),
        }
    }

    /// Run chat completion - translate request, call orchestrator, return response
    pub async fn run_chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, GatewayError> {
        // Extract prompt from messages (last user message)
        let prompt = extract_prompt_from_messages(&request.messages)?;

        // Demo logging for visualization
        println!("Routing request via InferenceOrchestrator");
        println!("  Model: {}", request.model);
        println!("  Prompt: {}", &prompt[..prompt.len().min(50)]);

        // Create inference request
        let inference_request = InferenceRequest::new(
            request.model.clone(),
            prompt,
            // Default memory estimate (can be made smarter based on model)
            8192,
        )
        .with_priority(RequestPriority::Normal)
        .with_precision(Precision::F16);

        let inference_request = if let Some(max_tokens) = request.max_tokens {
            inference_request.with_max_tokens(max_tokens)
        } else {
            inference_request
        };
        let inference_request = if let Some(temperature) = request.temperature {
            inference_request.with_temperature(temperature)
        } else {
            inference_request
        };

        // Call the orchestrator
        let result = {
            let mut orch = self.orchestrator.lock();
            orch.infer(&inference_request)
        };

        // Convert result to OpenAI format
        Ok(convert_to_openai_response(request.model, result))
    }
}

/// Extract prompt from OpenAI messages
fn extract_prompt_from_messages(messages: &[Message]) -> Result<String, GatewayError> {
    // Find the last user message
    let mut prompt = String::new();

    for msg in messages.iter().rev() {
        if msg.role == "user" {
            prompt = msg.content.clone();
            break;
        }
    }

    if prompt.is_empty() {
        // If no user message found, concatenate all messages
        for msg in messages {
            prompt.push_str(&msg.content);
            prompt.push('\n');
        }
    }

    Ok(prompt.trim().to_string())
}

/// Convert InferenceResult to OpenAI response format
fn convert_to_openai_response(model: String, result: InferenceResult) -> ChatCompletionResponse {
    let output = result.output;
    let completion_tokens = (output.len() as f32 / 4.0) as u32; // Rough estimate
    let prompt_tokens = 50; // Rough estimate
    let total_tokens = prompt_tokens + completion_tokens;

    ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object_type: "chat.completion".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        model,
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: "assistant".to_string(),
                content: output,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_prompt_from_messages() {
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: "You are a helpful assistant.".to_string(),
            },
            Message {
                role: "user".to_string(),
                content: "Explain Rust ownership".to_string(),
            },
        ];

        let prompt = extract_prompt_from_messages(&messages).unwrap();
        assert_eq!(prompt, "Explain Rust ownership");
    }

    #[test]
    fn test_chat_completion_response_format() {
        let result = InferenceResult {
            output: "Test output".to_string(),
            routed_to: mofa_foundation::inference::RoutedBackend::Local {
                model_id: "test-model".to_string(),
            },
            actual_precision: Precision::F16,
        };

        let response = convert_to_openai_response("test-model".to_string(), result);
        assert_eq!(response.object_type, "chat.completion");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "Test output");
    }
}
