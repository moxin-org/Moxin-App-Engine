//! Tests for TestStatus predicates and Serde support on report types.

use mofa_testing::report::{TestCaseResult, TestReport, TestStatus};
use std::time::Duration;

#[test]
fn is_passed_returns_true_for_passed() {
    assert!(TestStatus::Passed.is_passed());
    assert!(!TestStatus::Failed.is_passed());
    assert!(!TestStatus::Skipped.is_passed());
}

#[test]
fn is_failed_returns_true_for_failed() {
    assert!(TestStatus::Failed.is_failed());
    assert!(!TestStatus::Passed.is_failed());
    assert!(!TestStatus::Skipped.is_failed());
}

#[test]
fn is_skipped_returns_true_for_skipped() {
    assert!(TestStatus::Skipped.is_skipped());
    assert!(!TestStatus::Passed.is_skipped());
    assert!(!TestStatus::Failed.is_skipped());
}

#[test]
fn test_status_serde_roundtrip() {
    for status in [TestStatus::Passed, TestStatus::Failed, TestStatus::Skipped] {
        let json = serde_json::to_string(&status).unwrap();
        let back: TestStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, status);
    }
}

#[test]
fn test_case_result_serde_roundtrip() {
    let result = TestCaseResult {
        name: "my_test".into(),
        status: TestStatus::Failed,
        duration: Duration::from_millis(42),
        error: Some("assertion failed".into()),
        metadata: vec![("key".into(), "value".into())],
    };

    let json = serde_json::to_string(&result).unwrap();
    let back: TestCaseResult = serde_json::from_str(&json).unwrap();

    assert_eq!(back.name, "my_test");
    assert_eq!(back.status, TestStatus::Failed);
    assert_eq!(back.duration, Duration::from_millis(42));
    assert_eq!(back.error.as_deref(), Some("assertion failed"));
    assert_eq!(back.metadata, vec![("key".into(), "value".into())]);
}

#[test]
fn test_report_serde_roundtrip_all_fields() {
    let report = TestReport {
        suite_name: "my-suite".into(),
        results: vec![
            TestCaseResult {
                name: "test_a".into(),
                status: TestStatus::Passed,
                duration: Duration::from_millis(10),
                error: None,
                metadata: Vec::new(),
            },
            TestCaseResult {
                name: "test_b".into(),
                status: TestStatus::Failed,
                duration: Duration::from_millis(25),
                error: Some("boom".into()),
                metadata: vec![("env".into(), "ci".into())],
            },
            TestCaseResult {
                name: "test_c".into(),
                status: TestStatus::Skipped,
                duration: Duration::from_millis(0),
                error: None,
                metadata: Vec::new(),
            },
        ],
        total_duration: Duration::from_millis(100),
        timestamp: 1_234_567_890,
    };

    let json = serde_json::to_string_pretty(&report).unwrap();
    let back: TestReport = serde_json::from_str(&json).unwrap();

    assert_eq!(back.suite_name, "my-suite");
    assert_eq!(back.total(), 3);
    assert_eq!(back.passed(), 1);
    assert_eq!(back.failed(), 1);
    assert_eq!(back.skipped(), 1);
    assert_eq!(back.total_duration, Duration::from_millis(100));
    assert_eq!(back.timestamp, 1_234_567_890);
}

#[test]
fn deserialized_report_has_correct_pass_rate() {
    let report = TestReport {
        suite_name: "rate-test".into(),
        results: vec![
            TestCaseResult {
                name: "t1".into(),
                status: TestStatus::Passed,
                duration: Duration::from_millis(10),
                error: None,
                metadata: Vec::new(),
            },
            TestCaseResult {
                name: "t2".into(),
                status: TestStatus::Failed,
                duration: Duration::from_millis(10),
                error: Some("fail".into()),
                metadata: Vec::new(),
            },
        ],
        total_duration: Duration::from_millis(20),
        timestamp: 0,
    };

    let json = serde_json::to_string(&report).unwrap();
    let back: TestReport = serde_json::from_str(&json).unwrap();

    assert!((back.pass_rate() - 0.5).abs() < f64::EPSILON);
}
