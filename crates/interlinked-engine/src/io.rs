use crate::model::Scenario;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IoError {
    #[error("failed to read file: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse json: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn load_scenario_from_path(path: &str) -> Result<Scenario, IoError> {
    let bytes = std::fs::read(path)?;
    let scenario: Scenario = serde_json::from_slice(&bytes)?;
    Ok(scenario)
}

pub fn write_json_to_path<T: serde::Serialize>(path: &str, value: &T) -> Result<(), IoError> {
    let s = serde_json::to_string_pretty(value)?;
    std::fs::write(path, s)?;
    Ok(())
}
