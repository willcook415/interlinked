use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub(crate) enum EdgeKind {
    Walk,
    Wait,
    Ride,
    Transfer,
    Alight,
    Neutral,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub(crate) to: usize,
    pub(crate) gc_s: f64,
    pub(crate) raw_time_s: f64,
    pub(crate) kind: EdgeKind,
    pub(crate) link_idx: Option<usize>,
    pub(crate) transfer_penalty_s: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct SvcLayerIndex {
    pub(crate) svcstop_of_node: HashMap<usize, (String, String)>,
    pub(crate) svc_mode_of_node: HashMap<usize, String>,
    pub(crate) zone_nodes_start: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Graph {
    pub(crate) adj: Vec<Vec<Edge>>,
    pub(crate) access_edges: usize,
    pub(crate) egress_edges: usize,
    pub(crate) transfer_edges: usize,
    pub(crate) svc_index: SvcLayerIndex,
}

#[derive(Clone, Debug)]
pub(crate) struct DistEntry {
    pub(crate) dist: f64,
    pub(crate) prev: Option<usize>,
    pub(crate) prev_edge_kind: Option<EdgeKind>,
    pub(crate) prev_edge_raw_time: f64,
    pub(crate) prev_edge_gc: f64,
    pub(crate) via_link: Option<usize>,
    pub(crate) prev_edge_transfer_penalty_s: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct DState {
    pub(crate) node: usize,
    pub(crate) dist: f64,
}

impl Eq for DState {}
impl PartialEq for DState {
    fn eq(&self, other: &Self) -> bool {
        self.dist == other.dist && self.node == other.node
    }
}
impl Ord for DState {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .dist
            .partial_cmp(&self.dist)
            .unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for DState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PathStats {
    pub(crate) gc_s: f64,
    pub(crate) walk_s: f64,
    pub(crate) wait_s: f64,
    pub(crate) ivt_s: f64,
    pub(crate) fare_base: f64,
    pub(crate) transfer_time_s: f64,
    pub(crate) transfer_penalty_s: f64,
    pub(crate) transfer_count: f64,
    pub(crate) boardings: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltPath {
    pub(crate) link_indices: Vec<usize>,
    pub(crate) board_events: Vec<(String, String)>,
    pub(crate) alight_events: Vec<(String, String)>,
    pub(crate) board_modes: Vec<String>,
    pub(crate) board_times_s: Vec<f64>,
    pub(crate) stats: PathStats,
}
