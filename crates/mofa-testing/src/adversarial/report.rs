use std::collections::HashMap;
use crate::adversarial::category::OwaspCategory;
use crate::adversarial::evaluator::SafetyResult;
use crate::report::{TestCaseResult, TestStatus};
use std::time::Duration;

pub struct CategoryResult {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
}

pub struct SecurityReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<SafetyResult>,
    pub by_category: HashMap<OwaspCategory, CategoryResult>,
}

impl SecurityReport {
    pub fn new(results: Vec<SafetyResult>) -> Self {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = total - passed;

        let mut by_category = HashMap::new();
        for r in &results {
            let entry = by_category.entry(r.category).or_insert(CategoryResult {
                total: 0,
                passed: 0,
                failed: 0,
            });
            entry.total += 1;
            if r.passed {
                entry.passed += 1;
            } else {
                entry.failed += 1;
            }
        }

        Self {
            total,
            passed,
            failed,
            results,
            by_category,
        }
    }

    pub fn passed_all(&self) -> bool {
        self.failed == 0
    }

    pub fn summary(&self) -> String {
        let mut s = format!(
            "Security Audit Summary: {} tests, {} passed, {} failed ({:.1}% pass rate)\n",
            self.total,
            self.passed,
            self.failed,
            if self.total > 0 { (self.passed as f64 / self.total as f64) * 100.0 } else { 0.0 }
        );

        s.push_str("\nBy Category:\n");
        let mut cats: Vec<_> = self.by_category.keys().collect();
        cats.sort_by_key(|c| format!("{:?}", c));

        for cat in cats {
            let res = &self.by_category[cat];
            s.push_str(&format!(
                "  - {:?}: {}/{} passed\n",
                cat, res.passed, res.total
            ));
        }

        if self.failed > 0 {
            s.push_str("\nFailures:\n");
            for r in self.results.iter().filter(|r| !r.passed) {
                s.push_str(&format!(
                    "  [FAILED] [{:?}] Prompt: {}\n    Reason: {}\n",
                    r.category, r.prompt, r.reason
                ));
            }
        }

        s
    }

    pub fn to_test_case_results(&self) -> Vec<TestCaseResult> {
        self.results.iter().map(|r| {
            TestCaseResult {
                name: format!("Adversarial::{:?}::{}", r.category, r.prompt),
                status: if r.passed { TestStatus::Passed } else { TestStatus::Failed },
                duration: Duration::from_millis(0),
                error: if r.passed { None } else { Some(r.reason.clone()) },
                metadata: vec![
                    ("category".into(), format!("{:?}", r.category)),
                    ("prompt".into(), r.prompt.clone()),
                ],
            }
        }).collect()
    }
}
