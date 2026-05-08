use crate::*;

use interlinked_engine::sim::EngineFarePolicyContext;

pub(crate) struct RuntimeLoopHandle {
    pub(crate) project_path: String,
    pub(crate) tx: Sender<RuntimeAction>,
    pub(crate) pending_actions: Arc<AtomicUsize>,
    pub(crate) running: Arc<AtomicBool>,
    pub(crate) speed: Arc<AtomicU32>,
    pub(crate) clock_revision: Arc<AtomicU64>,
    pub(crate) join: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeMaterializationState {
    pub(crate) project_path: String,
    pub(crate) topology_hash: u64,
    pub(crate) scope_hash: u64,
    pub(crate) fare_hash: u64,
    pub(crate) minute_of_day: u32,
    pub(crate) last_materialized_tick: u64,
    pub(crate) adaptive_max_active_zones: usize,
    pub(crate) candidate_adaptive_max_active_zones: usize,
    pub(crate) last_tick_ms: f64,
    pub(crate) fare_policy_context: EngineFarePolicyContext,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeQueueIngestDebug {
    pub(crate) attempted_pax: f64,
    pub(crate) ingested_pax: f64,
    pub(crate) dropped_not_dispatchable_pax: f64,
    pub(crate) dropped_invalid_stop_pax: f64,
    pub(crate) remapped_to_reverse_service_pax: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeBoardingDebug {
    pub(crate) attempts: u32,
    pub(crate) attempted_pax: f64,
    pub(crate) boarded_pax: f64,
    pub(crate) left_behind_pax: f64,
    pub(crate) queue_total_before_pax: f64,
    pub(crate) queue_total_after_pax: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeOpsState {
    pub(crate) project_path: String,
    pub(crate) topology_hash: u64,
    pub(crate) profiles_by_service: HashMap<String, RuntimeServiceProfile>,
    pub(crate) stop_name_by_id: HashMap<String, String>,
    pub(crate) reverse_service_by_service: HashMap<String, String>,
    pub(crate) stop_ids_by_service: HashMap<String, HashSet<String>>,
    pub(crate) fare_base_by_service: HashMap<String, f64>,
    pub(crate) dispatch_service_ids: HashSet<String>,
    pub(crate) trains: BTreeMap<String, RuntimeTrainState>,
    pub(crate) queue_cohorts: HashMap<(String, String, String), f64>,
    pub(crate) last_queue_ingest_by_service_stop:
        HashMap<(String, String), RuntimeQueueIngestDebug>,
    pub(crate) last_boarding_by_service_stop: HashMap<(String, String), RuntimeBoardingDebug>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeTrainPhase {
    Dwell,
    Moving,
    Layover,
}

pub(crate) fn default_runtime_train_phase() -> RuntimeTrainPhase {
    RuntimeTrainPhase::Dwell
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeTrainState {
    pub(crate) train_id: String,
    pub(crate) service_id: String,
    pub(crate) line_id: String,
    pub(crate) line_name: String,
    pub(crate) mode: String,
    pub(crate) mode_variant: Option<String>,
    pub(crate) stock_tier_id: Option<String>,
    pub(crate) vehicle_capacity: f64,
    pub(crate) current_stop_index: usize,
    pub(crate) direction_step: i8,
    #[serde(default = "default_runtime_train_phase")]
    pub(crate) phase: RuntimeTrainPhase,
    pub(crate) progress: f64,
    pub(crate) remaining_s: f64,
    pub(crate) onboard_pax: f64,
    pub(crate) onboard_cohorts: HashMap<String, f64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeFareEvents {
    pub(crate) boarded_pax: f64,
    pub(crate) completed_alightings_pax: f64,
    pub(crate) liability_accrued_base: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeServiceProfile {
    pub(crate) service_id: String,
    pub(crate) line_id: String,
    pub(crate) line_name: String,
    pub(crate) mode: String,
    pub(crate) mode_variant: Option<String>,
    pub(crate) stock_tier_id: Option<String>,
    pub(crate) dwell_s: f64,
    pub(crate) turnaround_s: f64,
    pub(crate) speed_mps: f64,
    pub(crate) vehicle_capacity: f64,
    pub(crate) vehicles_on_service: usize,
    pub(crate) stop_ids: Vec<String>,
    pub(crate) stop_xy: Vec<(f64, f64)>,
    pub(crate) segment_lengths_m: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(crate) enum RuntimeAction {
    Stop,
    SetRunning(bool),
    SetSpeed(u32),
    InvalidateMaterialization,
    ForceCheckpoint,
    AdvanceOnce { recompute_quick_kpis: bool },
}

pub(crate) struct RuntimeTick {
    pub(crate) project_path: String,
    pub(crate) last_step: Instant,
}
