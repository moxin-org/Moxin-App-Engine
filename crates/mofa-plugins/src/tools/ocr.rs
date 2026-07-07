use super::*;
use serde_json::json;
use std::path::Path;
use tokio::process::Command;

// OCR TOOL : Extract texts from images using tessaract cli
// tools utilised : tessaract : For this to be utilised user needs to have tesseract cli installed
// This keeps dependencies light

pub struct OcrTool {
    definition: ToolDefinition,
}

impl Default for OcrTool {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrTool {
    pub fn new() -> Self {
        Self {
            definition: ToolDefinition {
                name: "ocr".to_string(),
                description: "Optical Character Recognition: Extract text from images using the system Tesseract binary."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "image_path": {
                            "type": "string",
                            "description": "Path to image file"
                        },

                        "lang": {
                            "type": "string",
                            "description": "Tesseract language code (e.g. 'eng', 'chi_sim'). Defaults to 'eng'."
                        },
                        "psm": {
                            "type": "integer",
                            "description": "Tesseract page segmentation mode passed as --psm (optional)."
                        }
                    },
                    "required": ["image_path"]
                }),
                requires_confirmation: false,
            },
        }
    }

    fn build_command(
        &self,
        tesseract_path: &std::path::Path,
        image_path: &str,
        lang: &str,
        psm: Option<i64>,
    ) -> Command {
        let mut cmd = Command::new(tesseract_path);
        cmd.arg(image_path).arg("-");

        if !lang.is_empty() {
            cmd.arg("-l").arg(lang);
        }
        if let Some(psm_value) = psm {
            if psm_value >= 0 {
                cmd.arg("--psm").arg(psm_value.to_string());
            }
        }

        cmd
    }
}

#[async_trait::async_trait]
impl ToolExecutor for OcrTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value> {
        //few checks before runnign command
        let image_path = arguments["image_path"].as_str().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(
                "Parameter 'image_path' is required for OCR tool".to_string(),
            )
        })?;
        if !Path::new(image_path).exists() {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Image file not found: {}",
                image_path
            )));
        }

        let lang = arguments
            .get("lang")
            .and_then(|v| v.as_str())
            .unwrap_or("eng");

        let psm = arguments.get("psm").and_then(|v| v.as_i64());

        //main operation related to tessaract
        //locating tessaract-> not found= error -> else utilising

        let tesseract_path = which::which("tesseract").map_err(|e| {
            mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Failed to locate 'tesseract'. Please install tesseract-ocr. Error : {}",
                e
            ))
        })?;

        //main operations

        let mut cmd = self.build_command(&tesseract_path, image_path, lang, psm);
        let output = cmd.output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Tesseract exited with non-zero status {:?}: {}",
                output.status.code(),
                if stderr.is_empty() {
                    stdout.clone()
                } else {
                    stderr.clone()
                }
            )));
        }

        //outputing success
        Ok(json!({
            "success":true,
            "language":lang,
            "text":stdout,
            "stderr": if stderr.len() > 2000 {
                format!("{}...[truncated]", &stderr[..2000])
            } else {
                stderr
            }


        }))
    }
}
