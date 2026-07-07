use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mofa_testing::{
    AsyncTestCase, AsyncTestOutcome, AsyncTestRunner, AsyncTestRunnerConfig, MockClock,
    TestProgressState, TestStatus,
};

#[tokio::test]
async fn async_runner_collects_pass_fail_and_skipped() {
    let runner = AsyncTestRunner::default();
    let cases = vec![
        AsyncTestCase::from_result("passes", async { Ok(()) }),
        AsyncTestCase::from_result("fails", async { Err("boom".to_string()) }),
        AsyncTestCase::new("skips", async {
            AsyncTestOutcome::skipped("feature disabled")
        }),
    ];

    let report = runner.run_suite("suite", cases).await;

    assert_eq!(report.total(), 3);
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 1);
    assert_eq!(report.skipped(), 1);

    let skip = report
        .results
        .iter()
        .find(|r| r.name == "skips")
        .expect("missing skip result");
    assert_eq!(skip.status, TestStatus::Skipped);
    assert!(skip.error.is_none());
    assert!(
        skip.metadata
            .iter()
            .any(|(k, v)| k == "skip_reason" && v == "feature disabled")
    );
}

#[tokio::test]
async fn async_runner_respects_concurrency_limit() {
    let running = Arc::new(AtomicUsize::new(0));
    let max_running = Arc::new(AtomicUsize::new(0));

    let mut cases = Vec::new();
    for idx in 0..8 {
        let running = Arc::clone(&running);
        let max_running = Arc::clone(&max_running);
        cases.push(AsyncTestCase::new(format!("case-{idx}"), async move {
            let now_running = running.fetch_add(1, Ordering::SeqCst) + 1;
            update_max(&max_running, now_running);
            tokio::time::sleep(Duration::from_millis(30)).await;
            running.fetch_sub(1, Ordering::SeqCst);
            AsyncTestOutcome::passed()
        }));
    }

    let runner = AsyncTestRunner::new(AsyncTestRunnerConfig::default().with_concurrency_limit(2));
    let report = runner.run_suite("parallel", cases).await;

    assert_eq!(report.failed(), 0);
    assert_eq!(report.passed(), 8);
    assert!(
        max_running.load(Ordering::SeqCst) <= 2,
        "expected max running <= 2, got {}",
        max_running.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn async_runner_marks_timeout_as_failed() {
    let runner = AsyncTestRunner::new(
        AsyncTestRunnerConfig::default().with_per_test_timeout(Duration::from_millis(20)),
    );

    let report = runner
        .run_suite(
            "timeouts",
            vec![AsyncTestCase::new("slow", async {
                tokio::time::sleep(Duration::from_millis(80)).await;
                AsyncTestOutcome::passed()
            })],
        )
        .await;

    assert_eq!(report.total(), 1);
    assert_eq!(report.failed(), 1);
    assert!(
        report.results[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("timeout")
    );
}

#[tokio::test]
async fn async_runner_emits_progress_events() {
    let events: Arc<Mutex<Vec<(String, TestProgressState)>>> = Arc::new(Mutex::new(Vec::new()));
    let event_sink = Arc::clone(&events);

    let runner = AsyncTestRunner::new(AsyncTestRunnerConfig::default().with_event_callback(
        move |event| {
            event_sink
                .lock()
                .expect("event lock poisoned")
                .push((event.test_name, event.state));
        },
    ));

    let cases = vec![
        AsyncTestCase::from_result("p", async { Ok(()) }),
        AsyncTestCase::from_result("f", async { Err("failed".to_string()) }),
    ];

    let report = runner.run_suite("events", cases).await;
    assert_eq!(report.total(), 2);

    let snapshot = events.lock().expect("event lock poisoned").clone();
    assert!(
        snapshot
            .iter()
            .any(|(name, state)| name == "p" && *state == TestProgressState::Started)
    );
    assert!(
        snapshot
            .iter()
            .any(|(name, state)| name == "p" && *state == TestProgressState::Passed)
    );
    assert!(
        snapshot
            .iter()
            .any(|(name, state)| name == "f" && *state == TestProgressState::Started)
    );
    assert!(
        snapshot
            .iter()
            .any(|(name, state)| name == "f" && *state == TestProgressState::Failed)
    );
}

#[tokio::test]
async fn async_runner_uses_injected_clock_for_report() {
    let clock = Arc::new(MockClock::starting_at(Duration::from_millis(1234)));
    let config = AsyncTestRunnerConfig::default().with_clock(clock);
    let runner = AsyncTestRunner::new(config);

    let report = runner.run_suite("clocked", vec![]).await;

    assert_eq!(report.timestamp, 1234);
}

fn update_max(max_running: &AtomicUsize, now_running: usize) {
    let mut current_max = max_running.load(Ordering::SeqCst);
    while now_running > current_max {
        match max_running.compare_exchange(
            current_max,
            now_running,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return,
            Err(observed) => current_max = observed,
        }
    }
}
