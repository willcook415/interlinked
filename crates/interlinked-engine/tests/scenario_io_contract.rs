mod common;

use interlinked_engine::{ScenarioFileShape, ScenarioService};
use serde_json::Value;

#[test]
fn loads_legacy_flat_as_current_schema() {
    let input = common::fixture_path("flat_minimal_valid.json");
    let (doc, shape) = ScenarioService::load_from_path_with_shape(input.to_string_lossy().as_ref())
        .expect("flat fixture should load");

    assert_eq!(shape, ScenarioFileShape::LegacyFlat);
    assert_eq!(
        doc.schema_version,
        interlinked_engine::ScenarioDocument::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(doc.scenario.meta.name, "flat_minimal_valid");
}

#[test]
fn loads_wrapped_document() {
    let input = common::fixture_path("wrapped_transfer_capacity_valid.json");
    let (doc, shape) = ScenarioService::load_from_path_with_shape(input.to_string_lossy().as_ref())
        .expect("wrapped fixture should load");

    assert_eq!(shape, ScenarioFileShape::Wrapped);
    assert_eq!(
        doc.schema_version,
        interlinked_engine::ScenarioDocument::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(doc.scenario.meta.name, "wrapped_transfer_capacity_valid");
}

#[test]
fn save_writes_wrapped_document() {
    let input = common::fixture_path("flat_minimal_valid.json");
    let doc = ScenarioService::load_from_path(input.to_string_lossy().as_ref())
        .expect("fixture load should succeed");

    let output_path = common::temp_output_path("interlinked_schema_save_wrapped");
    ScenarioService::save_to_path(output_path.to_string_lossy().as_ref(), &doc)
        .expect("save should succeed");

    let raw = std::fs::read_to_string(&output_path).expect("should read output json");
    let json: Value = serde_json::from_str(&raw).expect("should parse output json");

    assert!(json.get("schema_version").is_some());
    assert!(json.get("scenario").is_some());
    assert!(json.get("meta").is_none());

    let _ = std::fs::remove_file(output_path);
}

#[test]
fn roundtrip_flat_to_wrapped_preserves_scenario_semantics() {
    let input = common::fixture_path("flat_minimal_valid.json");
    let original = ScenarioService::load_from_path(input.to_string_lossy().as_ref())
        .expect("fixture load should succeed");

    let output_path = common::temp_output_path("interlinked_schema_roundtrip");
    ScenarioService::save_to_path(output_path.to_string_lossy().as_ref(), &original)
        .expect("save should succeed");

    let roundtrip = ScenarioService::load_from_path(output_path.to_string_lossy().as_ref())
        .expect("roundtrip load should succeed");

    let a = serde_json::to_value(&original.scenario).expect("serialize original scenario");
    let b = serde_json::to_value(&roundtrip.scenario).expect("serialize roundtrip scenario");

    assert_eq!(a, b, "scenario payload must remain semantically identical");

    let _ = std::fs::remove_file(output_path);
}
