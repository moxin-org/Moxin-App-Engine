pub mod category;
pub mod corpus;
pub mod evaluator;
pub mod report;

#[cfg(test)]
mod tests;

pub use category::OwaspCategory;
pub use corpus::{AdversarialCorpus, AdversarialPrompt};
pub use evaluator::{RuleBasedEvaluator, SafetyEvaluator, SafetyResult};
pub use report::SecurityReport;

use async_trait::async_trait;

#[async_trait]
pub trait AgentUnderTest: Send + Sync {
    async fn ask(&self, prompt: &str) -> String;
}

#[async_trait]
impl<F> AgentUnderTest for F 
where F: Fn(&str) -> String + Send + Sync 
{
    async fn ask(&self, prompt: &str) -> String {
        (self)(prompt)
    }
}

impl AdversarialCorpus {
    pub async fn run_against<A: AgentUnderTest>(
        &self,
        agent: &A,
        evaluator: &dyn SafetyEvaluator,
    ) -> SecurityReport {
        let mut results = Vec::new();

        for ap in &self.prompts {
            let response = agent.ask(&ap.prompt).await;
            let result = evaluator.evaluate(&ap.prompt, &response, ap.category).await;
            results.push(result);
        }

        SecurityReport::new(results)
    }
}
