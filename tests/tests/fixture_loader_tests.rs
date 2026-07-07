use mofa_testing::{fixture_path, load_fixture};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GenericFixture {
    case_name: String,
    kind: String,
    target: String,
    suite_name: String,
}

#[test]
fn load_scenario_fixture_from_yaml() {
    let spec: GenericFixture =
        load_fixture("scenarios/basic.yaml").expect("yaml fixture should load");
    assert_eq!(spec.case_name, "basic-yaml-scenario");
    assert_eq!(spec.kind, "contract");
    assert_eq!(spec.target, "testing-scenario");
    assert_eq!(spec.suite_name, "fixture-yaml-suite");
}

#[test]
fn load_scenario_fixture_from_json() {
    let spec: GenericFixture =
        load_fixture("scenarios/basic.json").expect("json fixture should load");
    assert_eq!(spec.case_name, "basic-json-scenario");
    assert_eq!(spec.kind, "contract");
    assert_eq!(spec.target, "testing-scenario");
    assert_eq!(spec.suite_name, "fixture-json-suite");
}

#[test]
fn scenario_spec_from_path_rejects_malformed_fixture() {
    let err = load_fixture::<GenericFixture>(fixture_path("scenarios/malformed.yaml"))
        .expect_err("malformed fixture must fail");
    let message = err.to_string();
    assert!(
        message.contains("failed to parse YAML fixture")
            || message.contains("did not find expected")
            || message.contains("invalid type"),
        "unexpected error message: {message}"
    );
}
