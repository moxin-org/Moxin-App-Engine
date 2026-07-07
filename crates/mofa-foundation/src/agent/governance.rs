//! Governance primitives for safe tool execution and content handling.

use mofa_kernel::agent::components::tool::ToolMetadata;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use regex::{Regex, RegexBuilder};
use std::collections::HashSet;

#[derive(Clone)]
struct RedactionRule {
    label: &'static str,
    pattern: Regex,
}

#[derive(Clone)]
pub struct GovernancePolicy {
    pub allowed_tools: HashSet<String>,
    pub denied_tools: HashSet<String>,
    pub dangerous_tools_require_allowlist: bool,
    moderation_patterns: Vec<Regex>,
    redaction_rules: Vec<RedactionRule>,
}

impl Default for GovernancePolicy {
    fn default() -> Self {
        let moderation_patterns = vec![
            compile_case_insensitive_word("malware").expect("valid moderation regex"),
            compile_case_insensitive_word("credential dump").expect("valid moderation regex"),
            compile_case_insensitive_word("data exfiltration").expect("valid moderation regex"),
        ];
        let redaction_rules = vec![
            RedactionRule {
                label: "EMAIL",
                pattern: Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b")
                    .expect("valid email regex"),
            },
            RedactionRule {
                label: "SSN",
                pattern: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("valid ssn regex"),
            },
            RedactionRule {
                label: "PHONE",
                pattern: Regex::new(r"\b(?:\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b")
                    .expect("valid phone regex"),
            },
        ];

        Self {
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            dangerous_tools_require_allowlist: true,
            moderation_patterns,
            redaction_rules,
        }
    }
}

impl GovernancePolicy {
    pub fn allow_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.allowed_tools.insert(tool_name.into());
        self
    }

    pub fn deny_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.denied_tools.insert(tool_name.into());
        self
    }

    pub fn with_moderation_terms(mut self, terms: &[&str]) -> AgentResult<Self> {
        let mut patterns = Vec::with_capacity(terms.len());
        for term in terms {
            patterns.push(compile_case_insensitive_word(term)?);
        }
        self.moderation_patterns = patterns;
        Ok(self)
    }
}

#[derive(Clone, Default)]
pub struct GovernancePipeline {
    policy: GovernancePolicy,
}

impl GovernancePipeline {
    pub fn new(policy: GovernancePolicy) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &GovernancePolicy {
        &self.policy
    }

    pub fn authorize_tool(&self, tool_name: &str, metadata: &ToolMetadata) -> AgentResult<()> {
        if self.policy.denied_tools.contains(tool_name) {
            return Err(AgentError::ValidationFailed(format!(
                "tool '{tool_name}' denied by governance policy"
            )));
        }

        if metadata.is_dangerous
            && self.policy.dangerous_tools_require_allowlist
            && !self.policy.allowed_tools.contains(tool_name)
        {
            return Err(AgentError::ValidationFailed(format!(
                "dangerous tool '{tool_name}' blocked (not allowlisted)"
            )));
        }

        Ok(())
    }

    pub fn moderate_text(&self, text: &str) -> AgentResult<()> {
        for pattern in &self.policy.moderation_patterns {
            if pattern.is_match(text) {
                return Err(AgentError::ValidationFailed(
                    "content blocked by moderation policy".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn redact_text(&self, text: &str) -> String {
        let mut redacted = text.to_string();
        for rule in &self.policy.redaction_rules {
            let replacement = format!("[REDACTED:{}]", rule.label);
            redacted = rule
                .pattern
                .replace_all(&redacted, replacement.as_str())
                .to_string();
        }
        redacted
    }
}

fn compile_case_insensitive_word(term: &str) -> AgentResult<Regex> {
    let escaped = regex::escape(term);
    let pattern = format!(r"\b{}\b", escaped);
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| AgentError::ConfigError(format!("invalid moderation term '{term}': {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dangerous_metadata() -> ToolMetadata {
        ToolMetadata::new().dangerous()
    }

    #[test]
    fn denies_dangerous_tool_when_not_allowlisted() {
        let pipeline = GovernancePipeline::new(GovernancePolicy::default());
        let err = pipeline
            .authorize_tool("shell_exec", &dangerous_metadata())
            .expect_err("dangerous tool should be denied by default");
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn allows_dangerous_tool_when_allowlisted() {
        let policy = GovernancePolicy::default().allow_tool("shell_exec");
        let pipeline = GovernancePipeline::new(policy);
        assert!(
            pipeline
                .authorize_tool("shell_exec", &dangerous_metadata())
                .is_ok()
        );
    }

    #[test]
    fn redacts_email_and_ssn() {
        let pipeline = GovernancePipeline::default();
        let input = "Contact me at dev@example.com and ssn 111-22-3333";
        let redacted = pipeline.redact_text(input);
        assert!(!redacted.contains("dev@example.com"));
        assert!(!redacted.contains("111-22-3333"));
        assert!(redacted.contains("[REDACTED:EMAIL]"));
        assert!(redacted.contains("[REDACTED:SSN]"));
    }

    #[test]
    fn moderation_uses_word_boundaries() {
        let policy = GovernancePolicy::default()
            .with_moderation_terms(&["ban"])
            .expect("valid policy");
        let pipeline = GovernancePipeline::new(policy);

        let blocked = pipeline.moderate_text("please BAN this command");
        assert!(blocked.is_err());

        let allowed = pipeline.moderate_text("a banner is fine");
        assert!(allowed.is_ok());
    }
}
