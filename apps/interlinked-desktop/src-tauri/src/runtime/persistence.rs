use std::path::Path;

use crate::{now_string, ProjectManifest};

pub(crate) fn persist_runtime_manifest_now(
    project_root: &Path,
    manifest: &mut ProjectManifest,
) -> Result<(), String> {
    manifest.updated_at = now_string();
    crate::write_manifest(project_root, manifest)
}
