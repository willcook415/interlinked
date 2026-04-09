use crate::*;

pub use crate::contracts::*;

pub struct AppState {
    pub game: Mutex<Option<interlinked_engine::platform::GameState>>,
    pub current_project: Mutex<Option<String>>,
    pub(crate) runtime_tick: Mutex<Option<RuntimeTick>>,
    pub(crate) runtime_loop: Mutex<Option<RuntimeLoopHandle>>,
    pub runtime_snapshots: Mutex<VecDeque<RuntimeSnapshot>>,
    pub runtime_fast_snapshots: Mutex<VecDeque<RuntimeFastSnapshot>>,
    pub runtime_strategic_snapshots: Mutex<VecDeque<RuntimeStrategicSnapshot>>,
    pub(crate) runtime_materialization: Mutex<Option<RuntimeMaterializationState>>,
    pub(crate) runtime_ops: Mutex<Option<RuntimeOpsState>>,
}
