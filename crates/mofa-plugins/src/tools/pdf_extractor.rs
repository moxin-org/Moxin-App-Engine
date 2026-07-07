use super::*;
use serde_json::json;
use std::path::Path;

// PDF Extrator Tool
// it uses pdf_extract crate

pub struct PdfTool {
    definition: ToolDefinition,
}

impl Default for PdfTool {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfTool {
    pub fn new() -> Self {
        Self {
            definition: ToolDefinition {
                name: "pdf_extract_text".to_string(),
                description:
                    "Extract text from a PDF file (embedded text; for scanned PDFs use OCR first)."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pdf_path": {
                            "type": "string",
                            "description": "Path to a PDF file on disk"
                        },
                        "by_pages": {
                            "type": "boolean",
                            "description": "If true,It will return an array of page texts instead of a single concatenated string. Defaults : False."
                        },
                    },
                    "required": ["pdf_path"]
                }),
                requires_confirmation: false,
            },
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for PdfTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value> {
        let pdf_path = arguments["pdf_path"].as_str().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "Parameter 'pdf_path' is required for pdf_extract_text tool".to_string(),
            )
        })?;

        if !Path::new(pdf_path).exists() {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "PDF file not found: {}",
                pdf_path
            )));
        }

        let by_pages = arguments
            .get("by_pages")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let bytes = tokio::fs::read(pdf_path).await.map_err(|e| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Failed to read PDF file '{}': {}",
                pdf_path, e
            ))
        })?;

        if by_pages {
            let pages = tokio::task::spawn_blocking(move || {
                pdf_extract::extract_text_from_mem_by_pages(&bytes)
            })
            .await
            .map_err(|e| {
                mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                    "PDF extraction task failed: {}",
                    e
                ))
            })?
            .map_err(|e| {
                mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                    "Failed to extract PDF text: {}",
                    e
                ))
            })?;

            Ok(json!({
                "success": true,
                "pdf_path": pdf_path,
                "by_pages": true,
                "pages": pages,
            }))
        } else {
            let text =
                tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&bytes))
                    .await
                    .map_err(|e| {
                        mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                            "PDF extraction task failed: {}",
                            e
                        ))
                    })?
                    .map_err(|e| {
                        mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                            "Failed to extract PDF text: {}",
                            e
                        ))
                    })?;

            Ok(json!({
                "success": true,
                "pdf_path": pdf_path,
                "by_pages": false,
                "text": text,
            }))
        }
    }
}
