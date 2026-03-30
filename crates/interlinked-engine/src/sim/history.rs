use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Kpis, SimState, SimulationOutput};

/// Controls how much history we store and at what granularity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryConfig {
    /// If true, record one frame per call to step_game/step_simulation wrapper.
    pub enabled: bool,

    /// Maximum number of frames to keep (ring buffer behavior).
    /// When exceeded, oldest frames are dropped.
    pub max_frames: usize,

    /// If true, store full board_loads in each frame (largest payload).
    pub record_board_loads: bool,

    /// If true, store per-step queue map snapshot (can be large).
    /// If false, only store queue summary stats.
    pub record_queue_map: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_frames: 10_000,       // future-proof: long sessions, but bounded
            record_board_loads: true, // your option 3
            record_queue_map: false,  // default: keep payload sensible
        }
    }
}

/// Compact summary of queues after a step.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueueSummary {
    pub total_queue: f64,
    pub max_queue: f64,
    pub queued_keys: usize,
}

/// One time-series frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryFrame {
    pub t_s: f64,
    pub kpis: Kpis,
    pub queue_summary: QueueSummary,

    /// Optional full queue map snapshot (service_id, stop_id) -> queue_end
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_map: Option<HashMap<(String, String), f64>>,

    /// Optional heavy payload: board loads (your existing output type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_loads: Option<Vec<super::types::BoardLoad>>,
}

/// Bounded time-series storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimHistory {
    pub cfg: HistoryConfig,
    pub frames: Vec<HistoryFrame>,
}

impl SimHistory {
    pub fn new(cfg: HistoryConfig) -> Self {
        Self {
            cfg,
            frames: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn is_enabled(&self) -> bool {
        self.cfg.enabled
    }

    /// Push a new frame derived from simulation output + resulting state.
    pub fn push(&mut self, out: &SimulationOutput, state_after: &SimState) {
        if !self.cfg.enabled {
            return;
        }

        let mut summary = QueueSummary::default();
        for (_, q) in state_after.queue.iter() {
            if q.is_finite() && *q >= 0.0 {
                summary.total_queue += *q;
                if *q > summary.max_queue {
                    summary.max_queue = *q;
                }
            }
        }
        summary.queued_keys = state_after.queue.len();

        let queue_map = if self.cfg.record_queue_map {
            Some(state_after.queue.clone())
        } else {
            None
        };

        let board_loads = if self.cfg.record_board_loads {
            Some(out.board_loads.clone())
        } else {
            None
        };

        self.frames.push(HistoryFrame {
            t_s: state_after.t_s,
            kpis: out.kpis.clone(),
            queue_summary: summary,
            queue_map,
            board_loads,
        });

        // enforce bounded storage (drop oldest)
        if self.frames.len() > self.cfg.max_frames {
            let overflow = self.frames.len() - self.cfg.max_frames;
            self.frames.drain(0..overflow);
        }
    }

    /// Get the most recent frame (useful for UI streaming).
    pub fn last(&self) -> Option<&HistoryFrame> {
        self.frames.last()
    }
}
