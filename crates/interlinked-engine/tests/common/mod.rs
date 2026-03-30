use std::path::PathBuf;

#[allow(dead_code)]
pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[allow(dead_code)]
pub fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

#[allow(dead_code)]
pub fn temp_output_path(prefix: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos();

    std::env::temp_dir().join(format!("{prefix}_{nanos}.json"))
}

#[allow(dead_code)]
pub fn assert_abs_close(actual: f64, expected: f64, eps: f64, label: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff <= eps,
        "{label}: expected {expected}, got {actual}, diff {diff}, eps {eps}"
    );
}
