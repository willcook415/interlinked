use crate::*;

pub(crate) fn default_runtime_enabled() -> bool {
    true
}

pub(crate) fn default_runtime_fixed_step_s() -> f64 {
    0.5
}

pub(crate) fn default_runtime_max_steps_per_cycle() -> u32 {
    12
}

pub(crate) fn default_runtime_checkpoint_interval_ticks() -> u32 {
    20
}

pub(crate) fn default_runtime_snapshot_ring() -> usize {
    32
}

pub(crate) fn default_runtime_target_tick_ms() -> f64 {
    16.0
}

pub(crate) fn default_runtime_strategic_refresh_interval_ticks() -> u32 {
    8
}

pub(crate) fn default_runtime_lightweight_tick_outputs() -> bool {
    true
}

pub(crate) fn default_runtime_ops_kernel_v1() -> bool {
    true
}

pub(crate) fn default_ui_runtime_trains_v1() -> bool {
    true
}

pub(crate) fn default_fare_recognition_v1() -> bool {
    true
}

pub(crate) fn default_runtime_scheduling_manifest() -> RuntimeSchedulingManifest {
    RuntimeSchedulingManifest {
        enabled: default_runtime_enabled(),
        fixed_step_s: default_runtime_fixed_step_s(),
        max_steps_per_cycle: default_runtime_max_steps_per_cycle(),
        checkpoint_interval_ticks: default_runtime_checkpoint_interval_ticks(),
        snapshot_ring: default_runtime_snapshot_ring(),
        target_tick_ms: default_runtime_target_tick_ms(),
        strategic_refresh_interval_ticks: default_runtime_strategic_refresh_interval_ticks(),
        lightweight_tick_outputs: default_runtime_lightweight_tick_outputs(),
        runtime_ops_kernel_v1: default_runtime_ops_kernel_v1(),
        ui_runtime_trains_v1: default_ui_runtime_trains_v1(),
        fare_recognition_v1: default_fare_recognition_v1(),
    }
}

pub(crate) fn runtime_trains_authoritative_for_manifest(manifest: &ProjectManifest) -> bool {
    if manifest.session_kind == SessionKind::Game {
        return true;
    }
    manifest.runtime_scheduling.runtime_ops_kernel_v1
        && manifest.runtime_scheduling.ui_runtime_trains_v1
}

pub(crate) fn runtime_fare_recognition_enabled_for_manifest(manifest: &ProjectManifest) -> bool {
    if manifest.session_kind == SessionKind::Game {
        return true;
    }
    manifest.runtime_scheduling.fare_recognition_v1
}

pub(crate) fn enforce_game_runtime_hardcut(manifest: &mut ProjectManifest) {
    if manifest.session_kind != SessionKind::Game {
        return;
    }
    manifest.runtime_scheduling.enabled = true;
    manifest.runtime_scheduling.lightweight_tick_outputs = true;
    manifest.runtime_scheduling.runtime_ops_kernel_v1 = true;
    manifest.runtime_scheduling.ui_runtime_trains_v1 = true;
    manifest.runtime_scheduling.fare_recognition_v1 = true;
}
