mod common;

use interlinked_engine::{ScenarioError, ScenarioService};

#[test]
fn invalid_fixture_returns_structured_validation_error() {
    let input = common::fixture_path("wrapped_invalid_missing_refs.json");

    let err = ScenarioService::load_from_path(input.to_string_lossy().as_ref())
        .expect_err("invalid fixture should fail validation");

    match err {
        ScenarioError::Validation(msg) => {
            assert!(
                msg.contains("link L_bad to_stop 'MISSING_STOP' not found"),
                "expected link reference error, got: {msg}"
            );
            assert!(
                msg.contains("service SVC_bad references missing stop 'MISSING_STOP'"),
                "expected service reference error, got: {msg}"
            );
        }
        other => panic!("expected ScenarioError::Validation, got {other}"),
    }
}
