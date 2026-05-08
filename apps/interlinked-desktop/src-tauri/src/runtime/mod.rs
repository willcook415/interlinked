pub mod defaults;
pub mod domain;
pub mod fare;
pub mod materialization;
pub mod models;
pub mod persistence;
pub mod scheduling;
pub mod service_profiles;
pub mod snapshots;
pub mod train_kernel;
pub mod views;
pub mod worker_actions;
pub mod worker_control;
pub mod worker_loop;
pub mod worker_tick_cycle;

#[cfg(test)]
mod perf_harness;
