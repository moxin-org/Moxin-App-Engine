use std::collections::VecDeque;

use async_trait::async_trait;
use mofa_testing::{
    AgentTest, GoldenCompareMode, GoldenDiff, GoldenSnapshot, GoldenStore, GoldenTestConfig,
    GoldenTurnSnapshot, NormalizerChain, RegexNormalizer, ScenarioAgent, ScenarioTurnOutput,
    ToolCallRecord, WhitespaceNormalizer, compare_golden, run_golden_test,
};
use serde_json::json;
use tempfile::TempDir;

// ─── Helper: scripted agent ─────────────────────────────────────────────────

struct ScriptedAgent {
    outputs: VecDeque<Result<ScenarioTurnOutput, String>>,
}

impl ScriptedAgent {
    fn new(outputs: Vec<Result<ScenarioTurnOutput, String>>) -> Self {
        Self {
            outputs: outputs.into(),
        }
    }
}

#[async_trait]
impl ScenarioAgent for ScriptedAgent {
    async fn execute_turn(
        &mut self,
        _system_prompt: Option<&str>,
        _user_input: &str,
    ) -> Result<ScenarioTurnOutput, String> {
        self.outputs
            .pop_front()
            .unwrap_or_else(|| Err("no scripted output available".to_string()))
    }
}

fn make_temp_dir() -> TempDir {
    tempfile::tempdir().expect("should create temp dir")
}

// ═══════════════════════════════════════════════════════════════════════════
// GoldenSnapshot: serialization tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_json_roundtrip() {
    let snapshot = GoldenSnapshot::new(
        "test_agent",
        vec![GoldenTurnSnapshot {
            user_input: "Hello".to_string(),
            response: "Hi there!".to_string(),
            tool_calls: vec![ToolCallRecord {
                name: "greet".to_string(),
                arguments: json!({"lang": "en"}),
            }],
        }],
    )
    .with_metadata("model", "gpt-4")
    .with_metadata("version", "1.0");

    let json = snapshot.to_json().expect("serialize");
    let deserialized = GoldenSnapshot::from_json(&json).expect("deserialize");

    assert_eq!(snapshot, deserialized);
    assert_eq!(deserialized.metadata.get("model").unwrap(), "gpt-4");
}

#[test]
fn snapshot_yaml_roundtrip() {
    let snapshot = GoldenSnapshot::new(
        "test_agent",
        vec![GoldenTurnSnapshot {
            user_input: "test".to_string(),
            response: "result".to_string(),
            tool_calls: vec![],
        }],
    );

    let yaml = snapshot.to_yaml().expect("serialize");
    let deserialized = GoldenSnapshot::from_yaml(&yaml).expect("deserialize");

    assert_eq!(snapshot, deserialized);
}

#[test]
fn snapshot_with_multiple_turns() {
    let snapshot = GoldenSnapshot::new(
        "multi_turn",
        vec![
            GoldenTurnSnapshot {
                user_input: "turn1".to_string(),
                response: "response1".to_string(),
                tool_calls: vec![],
            },
            GoldenTurnSnapshot {
                user_input: "turn2".to_string(),
                response: "response2".to_string(),
                tool_calls: vec![ToolCallRecord {
                    name: "search".to_string(),
                    arguments: json!({}),
                }],
            },
        ],
    );

    let json = snapshot.to_json().expect("serialize");
    let deserialized = GoldenSnapshot::from_json(&json).expect("deserialize");

    assert_eq!(deserialized.turns.len(), 2);
    assert_eq!(deserialized.turns[1].tool_calls.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// GoldenStore: filesystem operations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn store_save_and_load() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    let snapshot = GoldenSnapshot::new(
        "weather_test",
        vec![GoldenTurnSnapshot {
            user_input: "weather?".to_string(),
            response: "sunny".to_string(),
            tool_calls: vec![],
        }],
    );

    let path = store.save(&snapshot).expect("save should succeed");
    assert!(path.exists());
    assert!(path.to_string_lossy().contains("weather_test.golden.json"));

    let loaded = store.load("weather_test").expect("load should succeed");
    assert_eq!(loaded, snapshot);
}

#[test]
fn store_exists_check() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    assert!(!store.exists("nonexistent"));

    let snapshot = GoldenSnapshot::new("exists_test", vec![GoldenTurnSnapshot {
        user_input: "x".to_string(),
        response: "y".to_string(),
        tool_calls: vec![],
    }]);
    store.save(&snapshot).expect("save");

    assert!(store.exists("exists_test"));
}

#[test]
fn store_list_snapshots() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    for name in &["alpha", "beta", "gamma"] {
        store.save(&GoldenSnapshot::new(*name, vec![GoldenTurnSnapshot {
            user_input: "x".to_string(),
            response: "y".to_string(),
            tool_calls: vec![],
        }])).expect("save");
    }

    let listed = store.list();
    assert_eq!(listed, vec!["alpha", "beta", "gamma"]);
}

#[test]
fn store_handles_special_chars_in_name() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    let snapshot = GoldenSnapshot::new(
        "agent[berlin]",
        vec![GoldenTurnSnapshot {
            user_input: "x".to_string(),
            response: "y".to_string(),
            tool_calls: vec![],
        }],
    );

    store.save(&snapshot).expect("save should succeed");
    let path = store.snapshot_path("agent[berlin]");
    assert!(path.exists());
}

#[test]
fn store_load_nonexistent_fails() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    let result = store.load("does_not_exist");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// compare_golden: diff detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn compare_identical_outputs_no_diffs() {
    let golden = GoldenSnapshot::new(
        "test",
        vec![GoldenTurnSnapshot {
            user_input: "hello".to_string(),
            response: "Hi!".to_string(),
            tool_calls: vec![ToolCallRecord {
                name: "greet".to_string(),
                arguments: json!({}),
            }],
        }],
    );

    let actual = vec![(
        "hello".to_string(),
        ScenarioTurnOutput::new("Hi!").with_tool_call("greet", json!({})),
    )];

    let diffs = compare_golden(&golden, &actual, None);
    assert!(diffs.is_empty());
}

#[test]
fn compare_detects_response_mismatch() {
    let golden = GoldenSnapshot::new(
        "test",
        vec![GoldenTurnSnapshot {
            user_input: "hello".to_string(),
            response: "Hi!".to_string(),
            tool_calls: vec![],
        }],
    );

    let actual = vec![(
        "hello".to_string(),
        ScenarioTurnOutput::new("Hey there!"),
    )];

    let diffs = compare_golden(&golden, &actual, None);
    assert_eq!(diffs.len(), 1);
    assert!(matches!(
        &diffs[0],
        GoldenDiff::ResponseMismatch { turn: 1, expected, actual }
            if expected == "Hi!" && actual == "Hey there!"
    ));
}

#[test]
fn compare_detects_turn_count_mismatch() {
    let golden = GoldenSnapshot::new(
        "test",
        vec![
            GoldenTurnSnapshot {
                user_input: "a".to_string(),
                response: "b".to_string(),
                tool_calls: vec![],
            },
            GoldenTurnSnapshot {
                user_input: "c".to_string(),
                response: "d".to_string(),
                tool_calls: vec![],
            },
        ],
    );

    let actual = vec![("a".to_string(), ScenarioTurnOutput::new("b"))];

    let diffs = compare_golden(&golden, &actual, None);
    assert_eq!(diffs.len(), 1);
    assert!(matches!(
        &diffs[0],
        GoldenDiff::TurnCountMismatch { expected: 2, actual: 1 }
    ));
}

#[test]
fn compare_detects_tool_call_count_mismatch() {
    let golden = GoldenSnapshot::new(
        "test",
        vec![GoldenTurnSnapshot {
            user_input: "test".to_string(),
            response: "ok".to_string(),
            tool_calls: vec![
                ToolCallRecord { name: "a".to_string(), arguments: json!({}) },
                ToolCallRecord { name: "b".to_string(), arguments: json!({}) },
            ],
        }],
    );

    let actual = vec![(
        "test".to_string(),
        ScenarioTurnOutput::new("ok").with_tool_call("a", json!({})),
    )];

    let diffs = compare_golden(&golden, &actual, None);
    assert!(diffs.iter().any(|d| matches!(d, GoldenDiff::ToolCallCountMismatch { .. })));
}

#[test]
fn compare_detects_tool_name_mismatch() {
    let golden = GoldenSnapshot::new(
        "test",
        vec![GoldenTurnSnapshot {
            user_input: "test".to_string(),
            response: "ok".to_string(),
            tool_calls: vec![ToolCallRecord {
                name: "search".to_string(),
                arguments: json!({}),
            }],
        }],
    );

    let actual = vec![(
        "test".to_string(),
        ScenarioTurnOutput::new("ok").with_tool_call("lookup", json!({})),
    )];

    let diffs = compare_golden(&golden, &actual, None);
    assert!(diffs.iter().any(|d| matches!(
        d,
        GoldenDiff::ToolCallMismatch { expected_name, actual_name, .. }
            if expected_name == "search" && actual_name == "lookup"
    )));
}

#[test]
fn compare_detects_tool_args_mismatch() {
    let golden = GoldenSnapshot::new(
        "test",
        vec![GoldenTurnSnapshot {
            user_input: "test".to_string(),
            response: "ok".to_string(),
            tool_calls: vec![ToolCallRecord {
                name: "search".to_string(),
                arguments: json!({"q": "rust"}),
            }],
        }],
    );

    let actual = vec![(
        "test".to_string(),
        ScenarioTurnOutput::new("ok").with_tool_call("search", json!({"q": "python"})),
    )];

    let diffs = compare_golden(&golden, &actual, None);
    assert!(diffs.iter().any(|d| matches!(d, GoldenDiff::ToolCallArgsMismatch { .. })));
}

#[test]
fn compare_multiple_diffs_in_single_comparison() {
    let golden = GoldenSnapshot::new(
        "test",
        vec![
            GoldenTurnSnapshot {
                user_input: "a".to_string(),
                response: "expected_a".to_string(),
                tool_calls: vec![],
            },
            GoldenTurnSnapshot {
                user_input: "b".to_string(),
                response: "expected_b".to_string(),
                tool_calls: vec![],
            },
        ],
    );

    let actual = vec![
        ("a".to_string(), ScenarioTurnOutput::new("actual_a")),
        ("b".to_string(), ScenarioTurnOutput::new("actual_b")),
    ];

    let diffs = compare_golden(&golden, &actual, None);
    assert_eq!(diffs.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Normalizer tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn whitespace_normalizer_collapses_spaces() {
    use mofa_testing::golden::Normalizer;
    let n = WhitespaceNormalizer;
    assert_eq!(n.normalize("  hello   world  "), "hello world");
    assert_eq!(n.normalize("no\textra\nspaces"), "no extra spaces");
}

#[test]
fn regex_normalizer_replaces_uuids() {
    use mofa_testing::golden::Normalizer;
    let n = RegexNormalizer::new(
        r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
        "<UUID>",
    )
    .expect("valid regex");

    let input = "ID: 550e8400-e29b-41d4-a716-446655440000 done";
    assert_eq!(n.normalize(input), "ID: <UUID> done");
}

#[test]
fn regex_normalizer_replaces_timestamps() {
    use mofa_testing::golden::Normalizer;
    let n = RegexNormalizer::new(
        r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}",
        "<TIMESTAMP>",
    )
    .expect("valid regex");

    let input = "Created at 2026-04-09T18:00:00 UTC";
    assert_eq!(n.normalize(input), "Created at <TIMESTAMP> UTC");
}

#[test]
fn normalizer_chain_applies_all() {
    use mofa_testing::golden::Normalizer;
    let chain = NormalizerChain::default_chain().expect("chain should build");

    let input = "ID: 550e8400-e29b-41d4-a716-446655440000  at  2026-04-09T18:00:00";
    let normalized = chain.normalize(input);

    assert!(normalized.contains("<UUID>"));
    assert!(normalized.contains("<TIMESTAMP>"));
    assert!(!normalized.contains("  "));
}

#[test]
fn compare_with_normalizer_ignores_whitespace_diffs() {
    use mofa_testing::golden::Normalizer;
    let golden = GoldenSnapshot::new(
        "test",
        vec![GoldenTurnSnapshot {
            user_input: "hello".to_string(),
            response: "Hi  there!".to_string(),
            tool_calls: vec![],
        }],
    );

    let actual = vec![(
        "hello".to_string(),
        ScenarioTurnOutput::new("Hi there!"),
    )];

    let normalizer = WhitespaceNormalizer;
    let diffs = compare_golden(&golden, &actual, Some(&normalizer));
    assert!(diffs.is_empty(), "whitespace differences should be normalized away");
}

#[test]
fn compare_with_normalizer_ignores_uuid_diffs() {
    use mofa_testing::golden::Normalizer;
    let golden = GoldenSnapshot::new(
        "test",
        vec![GoldenTurnSnapshot {
            user_input: "hello".to_string(),
            response: "Request 550e8400-e29b-41d4-a716-446655440000 created".to_string(),
            tool_calls: vec![],
        }],
    );

    let actual = vec![(
        "hello".to_string(),
        ScenarioTurnOutput::new("Request a1b2c3d4-e5f6-7890-abcd-ef1234567890 created"),
    )];

    let chain = NormalizerChain::default_chain().expect("chain");
    let diffs = compare_golden(&golden, &actual, Some(&chain));
    assert!(diffs.is_empty(), "UUID differences should be normalized away");
}

// ═══════════════════════════════════════════════════════════════════════════
// run_golden_test: integration tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn golden_update_mode_saves_snapshot() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());
    let config = GoldenTestConfig::update(store);

    let scenario = AgentTest::new("update_test")
        .when_user_says("Hello")
        .then_agent_should()
        .respond_containing("Hi")
        .build()
        .expect("build");

    let mut agent = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new("Hi!"))]);
    let report = run_golden_test(&config, &scenario, &mut agent).await;

    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 0);

    // Verify snapshot was written
    let reload_store = GoldenStore::new(dir.path());
    assert!(reload_store.exists("update_test"));
    let snapshot = reload_store.load("update_test").expect("load");
    assert_eq!(snapshot.turns[0].response, "Hi!");
}

#[tokio::test]
async fn golden_strict_mode_passes_when_matching() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    // First: save a golden baseline
    store.save(&GoldenSnapshot::new(
        "strict_pass_test",
        vec![GoldenTurnSnapshot {
            user_input: "Hello".to_string(),
            response: "Hi!".to_string(),
            tool_calls: vec![],
        }],
    )).expect("save");

    let config = GoldenTestConfig::strict(GoldenStore::new(dir.path()));

    let scenario = AgentTest::new("strict_pass_test")
        .when_user_says("Hello")
        .then_agent_should()
        .respond_containing("Hi")
        .build()
        .expect("build");

    let mut agent = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new("Hi!"))]);
    let report = run_golden_test(&config, &scenario, &mut agent).await;

    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 0);
}

#[tokio::test]
async fn golden_strict_mode_fails_when_mismatched() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    store.save(&GoldenSnapshot::new(
        "strict_fail_test",
        vec![GoldenTurnSnapshot {
            user_input: "Hello".to_string(),
            response: "Hi!".to_string(),
            tool_calls: vec![],
        }],
    )).expect("save");

    let config = GoldenTestConfig::strict(GoldenStore::new(dir.path()));

    let scenario = AgentTest::new("strict_fail_test")
        .when_user_says("Hello")
        .then_agent_should()
        .respond_containing("Hi")
        .build()
        .expect("build");

    // Different response than golden
    let mut agent = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new("Hey there!"))]);
    let report = run_golden_test(&config, &scenario, &mut agent).await;

    assert_eq!(report.failed(), 1);
    assert!(report.results[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("response"));
}

#[tokio::test]
async fn golden_strict_mode_fails_when_no_snapshot_exists() {
    let dir = make_temp_dir();
    let config = GoldenTestConfig::strict(GoldenStore::new(dir.path()));

    let scenario = AgentTest::new("missing_golden")
        .when_user_says("Hello")
        .then_agent_should()
        .respond_containing("Hi")
        .build()
        .expect("build");

    let mut agent = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new("Hi!"))]);
    let report = run_golden_test(&config, &scenario, &mut agent).await;

    assert_eq!(report.failed(), 1);
    assert!(report.results[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("not found"));
}

#[tokio::test]
async fn golden_update_then_strict_roundtrip() {
    let dir = make_temp_dir();

    // Step 1: Update mode — record golden baseline
    let scenario = AgentTest::new("roundtrip_test")
        .when_user_says("What's 2+2?")
        .then_agent_should()
        .respond_containing("4")
        .build()
        .expect("build");

    let update_config = GoldenTestConfig::update(GoldenStore::new(dir.path()));
    let mut agent1 = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new("The answer is 4."))]);
    let update_report = run_golden_test(&update_config, &scenario, &mut agent1).await;
    assert_eq!(update_report.passed(), 1);

    // Step 2: Strict mode — same output should pass
    let strict_config = GoldenTestConfig::strict(GoldenStore::new(dir.path()));
    let mut agent2 = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new("The answer is 4."))]);
    let strict_report = run_golden_test(&strict_config, &scenario, &mut agent2).await;
    assert_eq!(strict_report.passed(), 1);
    assert_eq!(strict_report.failed(), 0);
}

#[tokio::test]
async fn golden_strict_with_normalizer_passes_uuid_diff() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    store.save(&GoldenSnapshot::new(
        "uuid_test",
        vec![GoldenTurnSnapshot {
            user_input: "create".to_string(),
            response: "Created item 550e8400-e29b-41d4-a716-446655440000".to_string(),
            tool_calls: vec![],
        }],
    )).expect("save");

    let config = GoldenTestConfig::strict(GoldenStore::new(dir.path()))
        .with_normalizer(NormalizerChain::default_chain().expect("chain"));

    let scenario = AgentTest::new("uuid_test")
        .when_user_says("create")
        .then_agent_should()
        .respond_containing("Created")
        .build()
        .expect("build");

    let mut agent = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new(
        "Created item a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    ))]);

    let report = run_golden_test(&config, &scenario, &mut agent).await;
    assert_eq!(report.passed(), 1, "UUID diff should be normalized away");
}

#[tokio::test]
async fn golden_multi_turn_comparison() {
    let dir = make_temp_dir();

    let scenario = AgentTest::new("multi_turn_golden")
        .when_user_says("Hello")
        .then_agent_should()
        .respond_containing("Hi")
        .when_user_says("Goodbye")
        .then_agent_should()
        .respond_containing("Bye")
        .build()
        .expect("build");

    // Update
    let update_config = GoldenTestConfig::update(GoldenStore::new(dir.path()));
    let mut agent1 = ScriptedAgent::new(vec![
        Ok(ScenarioTurnOutput::new("Hi there!")),
        Ok(ScenarioTurnOutput::new("Bye!")),
    ]);
    run_golden_test(&update_config, &scenario, &mut agent1).await;

    // Strict — matching
    let strict_config = GoldenTestConfig::strict(GoldenStore::new(dir.path()));
    let mut agent2 = ScriptedAgent::new(vec![
        Ok(ScenarioTurnOutput::new("Hi there!")),
        Ok(ScenarioTurnOutput::new("Bye!")),
    ]);
    let report = run_golden_test(&strict_config, &scenario, &mut agent2).await;
    assert_eq!(report.passed(), 1);

    // Strict — second turn changed
    let mut agent3 = ScriptedAgent::new(vec![
        Ok(ScenarioTurnOutput::new("Hi there!")),
        Ok(ScenarioTurnOutput::new("See you later!")),
    ]);
    let report2 = run_golden_test(&strict_config, &scenario, &mut agent3).await;
    assert_eq!(report2.failed(), 1);
}

#[tokio::test]
async fn golden_with_tool_calls_comparison() {
    let dir = make_temp_dir();
    let store = GoldenStore::new(dir.path());

    store.save(&GoldenSnapshot::new(
        "tool_golden",
        vec![GoldenTurnSnapshot {
            user_input: "search".to_string(),
            response: "Found results".to_string(),
            tool_calls: vec![ToolCallRecord {
                name: "web_search".to_string(),
                arguments: json!({"q": "rust"}),
            }],
        }],
    )).expect("save");

    let config = GoldenTestConfig::strict(GoldenStore::new(dir.path()));

    let scenario = AgentTest::new("tool_golden")
        .when_user_says("search")
        .then_agent_should()
        .respond_containing("Found")
        .build()
        .expect("build");

    // Same tool call — should pass
    let mut agent1 = ScriptedAgent::new(vec![Ok(
        ScenarioTurnOutput::new("Found results")
            .with_tool_call("web_search", json!({"q": "rust"})),
    )]);
    let report1 = run_golden_test(&config, &scenario, &mut agent1).await;
    assert_eq!(report1.passed(), 1);

    // Different tool args — should fail
    let mut agent2 = ScriptedAgent::new(vec![Ok(
        ScenarioTurnOutput::new("Found results")
            .with_tool_call("web_search", json!({"q": "python"})),
    )]);
    let report2 = run_golden_test(&config, &scenario, &mut agent2).await;
    assert_eq!(report2.failed(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// GoldenDiff: display formatting
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_diff_display_is_readable() {
    let diff = GoldenDiff::ResponseMismatch {
        turn: 1,
        expected: "hello".to_string(),
        actual: "hi".to_string(),
    };
    let msg = diff.to_string();
    assert!(msg.contains("turn 1"));
    assert!(msg.contains("hello"));
    assert!(msg.contains("hi"));
}

#[test]
fn golden_diff_serialization() {
    let diff = GoldenDiff::ToolCallArgsMismatch {
        turn: 2,
        tool_name: "search".to_string(),
        expected: json!({"q": "a"}),
        actual: json!({"q": "b"}),
    };

    let json = serde_json::to_string(&diff).expect("serialize");
    let deserialized: GoldenDiff = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(diff, deserialized);
}
