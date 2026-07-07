//! Async test execution engine with configurable concurrency and timeouts.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::clock::Clock;
use crate::report::{TestCaseResult, TestReport, TestReportBuilder, TestStatus};

/// Boxed future returned by a single async test case.
pub type BoxedAsyncTestFuture = Pin<Box<dyn Future<Output = AsyncTestOutcome> + Send>>;

/// Normalized outcome produced by a single async test case.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AsyncTestOutcome {
    Passed,
    Failed(String),
    Skipped(Option<String>),
}

impl AsyncTestOutcome {
    pub fn passed() -> Self {
        Self::Passed
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    pub fn skipped(reason: impl Into<String>) -> Self {
        Self::Skipped(Some(reason.into()))
    }
}

impl From<Result<(), String>> for AsyncTestOutcome {
    fn from(value: Result<(), String>) -> Self {
        match value {
            Ok(()) => Self::Passed,
            Err(msg) => Self::Failed(msg),
        }
    }
}

/// A single test case executable by [`AsyncTestRunner`].
pub struct AsyncTestCase {
    pub name: String,
    task: BoxedAsyncTestFuture,
}

impl AsyncTestCase {
    /// Create a case whose future returns [`AsyncTestOutcome`].
    pub fn new(
        name: impl Into<String>,
        task: impl Future<Output = AsyncTestOutcome> + Send + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            task: Box::pin(task),
        }
    }

    /// Create a case from a future that returns `Result<(), String>`.
    pub fn from_result(
        name: impl Into<String>,
        task: impl Future<Output = Result<(), String>> + Send + 'static,
    ) -> Self {
        Self::new(name, async move { AsyncTestOutcome::from(task.await) })
    }
}

/// Lifecycle states emitted as progress events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TestProgressState {
    Started,
    Passed,
    Failed,
    Skipped,
}

/// Structured progress event emitted as each test starts and finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestProgressEvent {
    pub test_name: String,
    pub state: TestProgressState,
    pub detail: Option<String>,
}

type EventCallback = Arc<dyn Fn(TestProgressEvent) + Send + Sync>;

/// Runtime configuration for [`AsyncTestRunner`].
#[derive(Clone)]
pub struct AsyncTestRunnerConfig {
    pub concurrency_limit: usize,
    pub per_test_timeout: Option<Duration>,
    pub clock: Option<Arc<dyn Clock>>,
    pub(crate) event_callback: Option<EventCallback>,
}

impl Default for AsyncTestRunnerConfig {
    fn default() -> Self {
        Self {
            concurrency_limit: 1,
            per_test_timeout: None,
            clock: None,
            event_callback: None,
        }
    }
}

impl AsyncTestRunnerConfig {
    pub fn with_concurrency_limit(mut self, limit: usize) -> Self {
        self.concurrency_limit = limit.max(1);
        self
    }

    pub fn with_per_test_timeout(mut self, timeout: Duration) -> Self {
        self.per_test_timeout = Some(timeout);
        self
    }

    pub fn without_timeout(mut self) -> Self {
        self.per_test_timeout = None;
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    pub fn with_event_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(TestProgressEvent) + Send + Sync + 'static,
    {
        self.event_callback = Some(Arc::new(callback));
        self
    }
}

/// Async test runner that executes tests with bounded concurrency.
pub struct AsyncTestRunner {
    config: AsyncTestRunnerConfig,
}

impl Default for AsyncTestRunner {
    fn default() -> Self {
        Self {
            config: AsyncTestRunnerConfig::default(),
        }
    }
}

impl AsyncTestRunner {
    pub fn new(config: AsyncTestRunnerConfig) -> Self {
        Self { config }
    }

    /// Execute all test cases and produce a consolidated [`TestReport`].
    pub async fn run_suite(
        &self,
        suite_name: impl Into<String>,
        test_cases: Vec<AsyncTestCase>,
    ) -> TestReport {
        let total = test_cases.len();
        let mut join_set: JoinSet<(usize, TestCaseResult)> = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(self.config.concurrency_limit.max(1)));

        for (index, test_case) in test_cases.into_iter().enumerate() {
            let timeout = self.config.per_test_timeout;
            let callback = self.config.event_callback.clone();
            let gate = Arc::clone(&semaphore);

            join_set.spawn(async move {
                let _permit = gate
                    .acquire_owned()
                    .await
                    .expect("test runner semaphore closed unexpectedly");
                let result = run_single_test_case(test_case, timeout, callback).await;
                (index, result)
            });
        }

        let mut ordered_results: Vec<Option<TestCaseResult>> = vec![None; total];
        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok((index, result)) => {
                    if let Some(slot) = ordered_results.get_mut(index) {
                        *slot = Some(result);
                    }
                }
                Err(err) => {
                    let result = TestCaseResult {
                        name: "unknown_test_case".to_string(),
                        status: TestStatus::Failed,
                        duration: Duration::from_secs(0),
                        error: Some(format!("runner task join error: {err}")),
                        metadata: vec![],
                    };
                    if let Some(slot) = ordered_results.iter_mut().find(|slot| slot.is_none()) {
                        *slot = Some(result);
                    }
                }
            }
        }

        let mut builder = TestReportBuilder::new(suite_name);
        if let Some(clock) = &self.config.clock {
            builder = builder.with_clock(Arc::clone(clock));
        }

        for result in ordered_results.into_iter().flatten() {
            builder = builder.add_result(result);
        }

        builder.build()
    }
}

async fn run_single_test_case(
    test_case: AsyncTestCase,
    timeout: Option<Duration>,
    callback: Option<EventCallback>,
) -> TestCaseResult {
    emit_event(
        &callback,
        TestProgressEvent {
            test_name: test_case.name.clone(),
            state: TestProgressState::Started,
            detail: None,
        },
    );

    let start = Instant::now();
    let outcome = match timeout {
        Some(limit) => match tokio::time::timeout(limit, test_case.task).await {
            Ok(outcome) => outcome,
            Err(_) => AsyncTestOutcome::Failed(format!(
                "test exceeded timeout of {} ms",
                limit.as_millis()
            )),
        },
        None => test_case.task.await,
    };
    let duration = start.elapsed();

    match outcome {
        AsyncTestOutcome::Passed => {
            emit_event(
                &callback,
                TestProgressEvent {
                    test_name: test_case.name.clone(),
                    state: TestProgressState::Passed,
                    detail: None,
                },
            );
            TestCaseResult {
                name: test_case.name,
                status: TestStatus::Passed,
                duration,
                error: None,
                metadata: vec![],
            }
        }
        AsyncTestOutcome::Failed(message) => {
            emit_event(
                &callback,
                TestProgressEvent {
                    test_name: test_case.name.clone(),
                    state: TestProgressState::Failed,
                    detail: Some(message.clone()),
                },
            );
            TestCaseResult {
                name: test_case.name,
                status: TestStatus::Failed,
                duration,
                error: Some(message),
                metadata: vec![],
            }
        }
        AsyncTestOutcome::Skipped(reason) => {
            emit_event(
                &callback,
                TestProgressEvent {
                    test_name: test_case.name.clone(),
                    state: TestProgressState::Skipped,
                    detail: reason.clone(),
                },
            );
            let mut metadata = vec![];
            if let Some(reason) = reason {
                metadata.push(("skip_reason".to_string(), reason));
            }
            TestCaseResult {
                name: test_case.name,
                status: TestStatus::Skipped,
                duration,
                error: None,
                metadata,
            }
        }
    }
}

fn emit_event(callback: &Option<EventCallback>, event: TestProgressEvent) {
    if let Some(cb) = callback {
        cb(event);
    }
}
