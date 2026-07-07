use super::*;
use serde_json::json;
use std::sync::{Arc, Mutex};

/// 教育反馈统计信息
/// Educational feedback statistical information
#[derive(Debug, Default)]
struct FeedbackStats {
    /// 连续"不懂"的次数
    /// Count of consecutive "confused" feedbacks
    consecutive_confused: u32,
    /// 当前教学模式
    /// Current teaching mode
    current_mode: String,
    /// 总反馈次数
    /// Total number of feedbacks
    total_feedback: u32,
}

/// 响应优化插件 - 根据学生反馈调整教学策略
/// Response optimization plugin - adjust strategies based on feedback
pub struct ResponseOptimizerTool {
    definition: ToolDefinition,
    feedback_stats: Arc<Mutex<FeedbackStats>>,
}

impl Default for ResponseOptimizerTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseOptimizerTool {
    pub fn new() -> Self {
        Self {
            definition: ToolDefinition {
                name: "response_optimizer".to_string(),
                description: "Educational response optimization: tracks student feedback and adjusts teaching strategies based on rules.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["record_feedback", "check_status", "reset_stats"],
                            "description": "Action to perform"
                        },
                        "feedback_type": {
                            "type": "string",
                            "enum": ["understand", "confused", "too_fast", "too_slow", "other"],
                            "description": "Type of student feedback"
                        },
                        "feedback_content": {
                            "type": "string",
                            "description": "Detailed feedback content"
                        }
                    },
                    "required": ["action"]
                }),
                requires_confirmation: false,
            },
            feedback_stats: Arc::new(Mutex::new(FeedbackStats::default())),
        }
    }

    /// 更新反馈统计并检查是否需要切换教学模式
    /// Update feedback stats and check if teaching mode switch is needed
    fn update_feedback_stats(&self, feedback_type: &str) -> String {
        let mut stats = self.feedback_stats.lock().unwrap_or_else(|e| e.into_inner());
        // 确保初始模式为normal
        // Ensure initial mode is set to normal
        if stats.current_mode.is_empty() {
            stats.current_mode = "normal".to_string();
        }

        let old_mode = stats.current_mode.clone();

        match feedback_type {
            "confused" => {
                stats.consecutive_confused += 1;
            }
            _ => {
                // 非"不懂"反馈重置连续计数
                // Reset consecutive count for non-"confused" feedback
                stats.consecutive_confused = 0;
            }
        }

        stats.total_feedback += 1;

        // 检查规则：连续三次"不懂"切换到更基础的模式
        // Check rule: three consecutive "confused" triggers basic mode
        if stats.consecutive_confused >= 3 && stats.current_mode != "basic" {
            stats.current_mode = "basic".to_string();
            return format!(
                "Mode switched from {} to basic because of {} consecutive 'confused' feedbacks",
                old_mode, stats.consecutive_confused
            );
        }
        // 如果学生表示理解且当前模式是基础模式，切换回正常模式
        // If student understands and mode is basic, switch back to normal
        else if feedback_type == "understand" && stats.current_mode == "basic" {
            stats.current_mode = "normal".to_string();
            return "Mode switched from basic to normal because student reported understanding"
                .to_string();
        }

        "No mode change".to_string()
    }
}

#[async_trait::async_trait]
impl ToolExecutor for ResponseOptimizerTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value> {
        let action = arguments["action"].as_str().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed("Action is required".to_string())
        })?;

        match action {
            "record_feedback" => {
                let feedback_type = arguments["feedback_type"].as_str().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "feedback_type is required for record_feedback".to_string(),
                    )
                })?;

                let feedback_content = arguments["feedback_content"].as_str().unwrap_or("");

                // 更新反馈统计并检查模式切换
                // Update feedback statistics and check for mode switching
                let mode_change = self.update_feedback_stats(feedback_type);

                let stats = self.feedback_stats.lock().unwrap_or_else(|e| e.into_inner());

                Ok(json!({
                    "status": "success",
                    "feedback_type": feedback_type,
                    "feedback_content": feedback_content,
                    "mode_change": mode_change,
                    "current_mode": stats.current_mode,
                    "consecutive_confused": stats.consecutive_confused,
                    "total_feedback": stats.total_feedback
                }))
            }
            "check_status" => {
                let mut stats = self.feedback_stats.lock().unwrap_or_else(|e| e.into_inner());
                // 确保初始模式为normal
                // Ensure initial mode is set to normal
                if stats.current_mode.is_empty() {
                    stats.current_mode = "normal".to_string();
                }

                Ok(json!({
                    "status": "success",
                    "current_mode": stats.current_mode,
                    "consecutive_confused": stats.consecutive_confused,
                    "total_feedback": stats.total_feedback
                }))
            }
            "reset_stats" => {
                let mut stats = self.feedback_stats.lock().unwrap_or_else(|e| e.into_inner());

                *stats = FeedbackStats::default();

                Ok(json!({
                    "status": "success",
                    "message": "Feedback stats reset successfully"
                }))
            }
            _ => Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_response_optimizer_mode_switch() {
        let optimizer = ResponseOptimizerTool::new();

        // 初始状态检查
        // Initial status check
        let status = optimizer
            .execute(json!({
                "action": "check_status"
            }))
            .await
            .unwrap();
        assert_eq!(status["current_mode"], "normal");
        assert_eq!(status["consecutive_confused"], 0);

        // 第一次反馈"不懂"
        // First feedback: "Confused"
        let result = optimizer
            .execute(json!({
                "action": "record_feedback",
                "feedback_type": "confused",
                "feedback_content": "不明白"
            }))
            .await
            .unwrap();
        assert_eq!(result["consecutive_confused"], 1);
        assert_eq!(result["current_mode"], "normal");

        // 第二次反馈"不懂"
        // Second feedback: "Confused"
        let result = optimizer
            .execute(json!({
                "action": "record_feedback",
                "feedback_type": "confused",
                "feedback_content": "还是不明白"
            }))
            .await
            .unwrap();
        assert_eq!(result["consecutive_confused"], 2);
        assert_eq!(result["current_mode"], "normal");

        // 第三次反馈"不懂" - 应该切换到基础模式
        // Third feedback: "Confused" - should switch to basic mode
        let result = optimizer
            .execute(json!({
                "action": "record_feedback",
                "feedback_type": "confused",
                "feedback_content": "完全不明白"
            }))
            .await
            .unwrap();
        assert_eq!(result["consecutive_confused"], 3);
        assert_eq!(result["current_mode"], "basic");
        assert!(
            result["mode_change"]
                .as_str()
                .unwrap()
                .contains("Mode switched")
        );

        // 反馈"理解" - 应该切换回正常模式
        // Feedback: "Understand" - should switch back to normal mode
        let result = optimizer
            .execute(json!({
                "action": "record_feedback",
                "feedback_type": "understand",
                "feedback_content": "现在明白了"
            }))
            .await
            .unwrap();
        assert_eq!(result["consecutive_confused"], 0);
        assert_eq!(result["current_mode"], "normal");

        println!("✓ 所有测试用例通过！");
        // ✓ All test cases passed!
    }
}
