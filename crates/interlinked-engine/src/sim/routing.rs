use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{BinaryHeap, HashMap};

use super::graph::{BuiltPath, DState, DistEntry, Edge, EdgeKind, Graph, PathStats, SvcLayerIndex};
use crate::model::{Scenario, Service, Stop};

fn service_is_active_for_graph(svc: &Service) -> bool {
    if matches!(svc.service_enabled, Some(false)) {
        return false;
    }
    if let Some(tph) = svc.operating_tph {
        if !tph.is_finite() || tph <= 0.0 {
            return false;
        }
    }
    if !svc.headway_s.is_finite() || svc.headway_s <= 0.0 || svc.headway_s >= 86_399.0 {
        return false;
    }
    if let Some(units) = svc.stock_units_assigned {
        if units == 0 {
            return false;
        }
    }
    true
}

pub(crate) fn build_graph(
    s: &Scenario,
    stop_index: &HashMap<String, usize>,
    _zone_index: &HashMap<String, usize>,
) -> Result<Graph, String> {
    let zone_count = s.world.zones.len();
    let stop_count = s.world.stops.len();

    // stop_in(i)=2*i, stop_out(i)=2*i+1 (we keep these for access/egress + transfers)
    let stop_nodes = 2 * stop_count;

    // Service-layer nodes: one per (service, stop in its sequence)
    let svc_nodes_start = stop_nodes;
    let mut node_of: HashMap<(String, String), usize> = HashMap::new();
    let mut next_node = svc_nodes_start;

    for svc in &s.world.services {
        if !service_is_active_for_graph(svc) {
            continue;
        }
        for sid in &svc.stop_sequence {
            if stop_index.contains_key(sid) {
                let key = (svc.id.clone(), sid.clone());
                if let Entry::Vacant(entry) = node_of.entry(key) {
                    entry.insert(next_node);
                    next_node += 1;
                }
            }
        }
    }

    let svc_nodes_count = next_node - svc_nodes_start;
    let zone_nodes_start = svc_nodes_start + svc_nodes_count;
    let mut svcstop_of_node: HashMap<usize, (String, String)> = HashMap::new();
    let mut svc_mode_of_node: HashMap<usize, String> = HashMap::new();
    let mut svc_mode_of_service: HashMap<String, String> = HashMap::new();
    for svc in &s.world.services {
        if !service_is_active_for_graph(svc) {
            continue;
        }
        svc_mode_of_service.insert(svc.id.clone(), svc.mode.clone());
    }
    for ((svc_id, stop_id), node) in node_of.iter() {
        svcstop_of_node.insert(*node, (svc_id.clone(), stop_id.clone()));
        if let Some(mode) = svc_mode_of_service.get(svc_id) {
            svc_mode_of_node.insert(*node, mode.clone());
        }
    }

    let node_count = zone_nodes_start + 2 * zone_count;
    let mut adj = vec![Vec::<Edge>::new(); node_count];

    // Helper closures
    let stop_in = |si: usize| 2 * si;
    let stop_out = |si: usize| 2 * si + 1;

    let zone_access = |zi: usize| zone_nodes_start + zi;
    let zone_egress = |zi: usize| zone_nodes_start + zone_count + zi;

    // 0) Neutral within-stop edges stop_in <-> stop_out (0)
    for si in 0..stop_count {
        adj[stop_in(si)].push(Edge {
            to: stop_out(si),
            gc_s: 0.0,
            raw_time_s: 0.0,
            kind: EdgeKind::Neutral,
            link_idx: None,
            transfer_penalty_s: None,
        });
        adj[stop_out(si)].push(Edge {
            to: stop_in(si),
            gc_s: 0.0,
            raw_time_s: 0.0,
            kind: EdgeKind::Neutral,
            link_idx: None,
            transfer_penalty_s: None,
        });
    }

    // 1) Physical link lookup: (from_stop, to_stop, mode) -> (link_idx, ivt_s)
    let mut link_lookup: HashMap<(String, String, String), (usize, f64)> = HashMap::new();
    for (li, link) in s.world.links.iter().enumerate() {
        if link.speed_mps <= 0.0 {
            return Err(format!("link {} has non-positive speed_mps", link.id));
        }
        let ivt_s = link.distance_m / link.speed_mps;
        link_lookup.insert(
            (
                link.from_stop.clone(),
                link.to_stop.clone(),
                link.mode.clone(),
            ),
            (li, ivt_s),
        );
    }

    // 2) Boarding edges + alight edges per service-stop
    for svc in &s.world.services {
        if !service_is_active_for_graph(svc) {
            continue;
        }
        if svc.headway_s <= 0.0 {
            return Err(format!("service {} has non-positive headway_s", svc.id));
        }
        let wait_raw = 0.5 * svc.headway_s;
        let board_pen = svc.board_penalty_s.unwrap_or(0.0);

        // GC: weight waiting, but boarding penalty is unweighted (already seconds-equivalent)
        let gc = (wait_raw * s.params.wait_weight) + board_pen;
        let raw = wait_raw + board_pen;

        for sid in &svc.stop_sequence {
            let Some(&si) = stop_index.get(sid) else {
                continue;
            };
            let key = (svc.id.clone(), sid.clone());
            let Some(&svc_node) = node_of.get(&key) else {
                continue;
            };

            // Arrive at stop_in, board the service: stop_in -> svc_node
            adj[stop_in(si)].push(Edge {
                to: svc_node,
                gc_s: gc,
                raw_time_s: raw,
                kind: EdgeKind::Wait,
                link_idx: None,
                transfer_penalty_s: None,
            });

            // Alight from service-layer to stop_in: svc_node -> stop_in (0)
            adj[svc_node].push(Edge {
                to: stop_in(si),
                gc_s: 0.0,
                raw_time_s: 0.0,
                kind: EdgeKind::Alight,
                link_idx: None,
                transfer_penalty_s: None,
            });
        }
    }

    // 3) Ride edges along services: svc_node(a) -> svc_node(b) for consecutive stops
    for svc in &s.world.services {
        if !service_is_active_for_graph(svc) {
            continue;
        }
        for w in svc.stop_sequence.windows(2) {
            let a = &w[0];
            let b = &w[1];

            let Some(&na) = node_of.get(&(svc.id.clone(), a.clone())) else {
                continue;
            };
            let Some(&nb) = node_of.get(&(svc.id.clone(), b.clone())) else {
                continue;
            };

            let (li, ivt_s) = link_lookup
                .get(&(a.clone(), b.clone(), svc.mode.clone()))
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "service {} missing physical link {} -> {} for mode {}",
                        svc.id, a, b, svc.mode
                    )
                })?;

            // v0: include dwell at downstream stop as a time cost
            let raw = ivt_s + svc.dwell_s;
            let gc = (ivt_s * s.params.ivt_weight) + svc.dwell_s;

            adj[na].push(Edge {
                to: nb,
                gc_s: gc,
                raw_time_s: raw,
                kind: EdgeKind::Ride,
                link_idx: Some(li),
                transfer_penalty_s: None,
            });
        }
    }

    // 4) Transfer edges: stop_in -> stop_in (time + penalty)
    for tr in &s.world.transfers {
        let from_si = *stop_index
            .get(&tr.from_stop)
            .ok_or_else(|| format!("transfer references unknown from_stop {}", tr.from_stop))?;
        let to_si = *stop_index
            .get(&tr.to_stop)
            .ok_or_else(|| format!("transfer references unknown to_stop {}", tr.to_stop))?;

        let pen = tr.penalty_s.unwrap_or(s.params.transfer_penalty_s);
        let raw = tr.time_s + pen;
        let gc = raw;

        adj[stop_in(from_si)].push(Edge {
            to: stop_in(to_si),
            gc_s: gc,
            raw_time_s: raw,
            kind: EdgeKind::Transfer,
            link_idx: None,
            transfer_penalty_s: Some(pen),
        });
    }

    add_mode_aware_interchanges(s, &mut adj, stop_index, &stop_in)?;

    // 5) Access/egress edges: zone_access -> stop_in, stop_in -> zone_egress
    let walk_speed = s.params.access_walk_speed_mps;
    let r = s.params.access_radius_m;

    let mut access_edges = 0usize;
    let mut egress_edges = 0usize;

    for (zi, zone) in s.world.zones.iter().enumerate() {
        let mut in_radius = 0usize;
        let mut nearest: Option<(usize, f64)> = None;

        for (si, stop) in s.world.stops.iter().enumerate() {
            let d = hypot(zone.x - stop.x, zone.y - stop.y);
            match nearest {
                Some((_, best)) if d >= best => {}
                None => {
                    nearest = Some((si, d));
                }
                _ => {
                    nearest = Some((si, d));
                }
            }

            if d <= r {
                let walk_s = d / walk_speed;
                let gc = walk_s * s.params.walk_weight;

                adj[zone_access(zi)].push(Edge {
                    to: stop_in(si),
                    gc_s: gc,
                    raw_time_s: walk_s,
                    kind: EdgeKind::Walk,
                    link_idx: None,
                    transfer_penalty_s: None,
                });
                access_edges += 1;

                adj[stop_in(si)].push(Edge {
                    to: zone_egress(zi),
                    gc_s: gc,
                    raw_time_s: walk_s,
                    kind: EdgeKind::Walk,
                    link_idx: None,
                    transfer_penalty_s: None,
                });
                egress_edges += 1;
                in_radius += 1;
            }
        }

        if in_radius == 0 {
            if let Some((si, d)) = nearest {
                // Sparse/coarse demand surfaces can leave no stops inside strict radius.
                // Always connect to the nearest stop so OD demand never disconnects entirely.
                let walk_s = d / walk_speed;
                let gc = walk_s * s.params.walk_weight * 1.25;

                adj[zone_access(zi)].push(Edge {
                    to: stop_in(si),
                    gc_s: gc,
                    raw_time_s: walk_s,
                    kind: EdgeKind::Walk,
                    link_idx: None,
                    transfer_penalty_s: None,
                });
                access_edges += 1;

                adj[stop_in(si)].push(Edge {
                    to: zone_egress(zi),
                    gc_s: gc,
                    raw_time_s: walk_s,
                    kind: EdgeKind::Walk,
                    link_idx: None,
                    transfer_penalty_s: None,
                });
                egress_edges += 1;
            }
        }
    }

    let transfer_edges: usize = adj
        .iter()
        .map(|edges| {
            edges
                .iter()
                .filter(|e| matches!(e.kind, EdgeKind::Transfer))
                .count()
        })
        .sum();

    Ok(Graph {
        adj,
        access_edges,
        egress_edges,
        transfer_edges,
        svc_index: SvcLayerIndex {
            svcstop_of_node,
            svc_mode_of_node,
            zone_nodes_start,
        },
    })
}

pub(crate) fn build_graph_with_costs(
    s: &Scenario,
    stop_index: &HashMap<String, usize>,
    zone_index: &HashMap<String, usize>,
    prev_link_passengers: &[f64],
    extra_wait_by_board: &HashMap<(String, String), f64>, // (service_id, stop_id) -> extra wait seconds
) -> Result<Graph, String> {
    // Start from the normal graph
    let mut g = build_graph(s, stop_index, zone_index)?;

    // Adjust Ride edges' generalized cost using crowding_multiplier()
    for edges in &mut g.adj {
        for e in edges {
            if matches!(e.kind, EdgeKind::Ride) {
                if let Some(li) = e.link_idx {
                    let mult = crowding_multiplier(
                        prev_link_passengers[li],
                        s.world.links[li].capacity_per_hour,
                        s.meta.time_period_hours,
                    );
                    e.gc_s *= mult;
                }
            }
        }
    }

    // Adjust Wait edges using queue-derived extra wait (service/stop specific)
    if !extra_wait_by_board.is_empty() {
        for edges in &mut g.adj {
            for e in edges {
                if matches!(e.kind, EdgeKind::Wait) {
                    if let Some((svc_id, stop_id)) = g.svc_index.svcstop_of_node.get(&e.to) {
                        if let Some(extra) =
                            extra_wait_by_board.get(&(svc_id.clone(), stop_id.clone()))
                        {
                            // raw_time_s currently includes base expected wait + board_penalty.
                            // Add extra wait as pure waiting time (weighted in gc).
                            e.raw_time_s += *extra;
                            e.gc_s += *extra * s.params.wait_weight;
                        }
                    }
                }
            }
        }
    }

    Ok(g)
}

pub(crate) fn hypot(dx: f64, dy: f64) -> f64 {
    (dx * dx + dy * dy).sqrt()
}

pub(crate) fn dijkstra(graph: &Graph, start: usize) -> Vec<DistEntry> {
    let n = graph.adj.len();
    let mut dist = vec![
        DistEntry {
            dist: f64::INFINITY,
            prev: None,
            prev_edge_kind: None,
            prev_edge_raw_time: 0.0,
            prev_edge_gc: 0.0,
            via_link: None,
            prev_edge_transfer_penalty_s: 0.0,
        };
        n
    ];

    dist[start].dist = 0.0;
    let mut heap = BinaryHeap::new();
    heap.push(DState {
        node: start,
        dist: 0.0,
    });

    while let Some(DState { node, dist: d }) = heap.pop() {
        if d > dist[node].dist {
            continue;
        }
        for e in &graph.adj[node] {
            let nd = d + e.gc_s;
            if nd < dist[e.to].dist {
                dist[e.to].dist = nd;
                dist[e.to].prev = Some(node);
                dist[e.to].prev_edge_kind = Some(e.kind);
                dist[e.to].prev_edge_raw_time = e.raw_time_s;
                dist[e.to].prev_edge_gc = e.gc_s;
                dist[e.to].via_link = e.link_idx;
                dist[e.to].prev_edge_transfer_penalty_s = if matches!(e.kind, EdgeKind::Transfer) {
                    e.transfer_penalty_s.unwrap_or(0.0)
                } else {
                    0.0
                };

                heap.push(DState {
                    node: e.to,
                    dist: nd,
                });
            }
        }
    }
    dist
}

fn best_edge_between(graph: &Graph, from: usize, to: usize) -> Option<&Edge> {
    graph.adj[from]
        .iter()
        .filter(|e| e.to == to)
        .min_by(|a, b| a.gc_s.partial_cmp(&b.gc_s).unwrap_or(Ordering::Equal))
}

fn build_path_from_nodes(graph: &Graph, nodes: &[usize]) -> Option<BuiltPath> {
    if nodes.len() < 2 {
        return None;
    }

    let mut link_indices = Vec::new();
    let mut board_events: Vec<(String, String)> = Vec::new();
    let mut alight_events: Vec<(String, String)> = Vec::new();
    let mut board_modes: Vec<String> = Vec::new();
    let mut board_times_s: Vec<f64> = Vec::new();

    let mut gc_s = 0.0;
    let mut walk_s = 0.0;
    let mut wait_s = 0.0;
    let mut ivt_s = 0.0;
    let mut raw_elapsed_s = 0.0;

    let mut transfer_time_s = 0.0;
    let mut transfer_penalty_s = 0.0;
    let mut boardings = 0.0;
    let mut transfer_edges = 0.0;

    for w in nodes.windows(2) {
        let from = w[0];
        let to = w[1];
        let e = best_edge_between(graph, from, to)?;

        gc_s += e.gc_s;
        let raw = e.raw_time_s;
        raw_elapsed_s += raw;

        match e.kind {
            EdgeKind::Walk => walk_s += raw,
            EdgeKind::Wait => {
                wait_s += raw;
                boardings += 1.0;
                if let Some((svc_id, stop_id)) = graph.svc_index.svcstop_of_node.get(&to) {
                    board_events.push((svc_id.clone(), stop_id.clone()));
                }
                if let Some(mode) = graph.svc_index.svc_mode_of_node.get(&to) {
                    board_modes.push(mode.clone());
                }
                board_times_s.push(raw_elapsed_s);
            }
            EdgeKind::Ride => {
                ivt_s += raw;
                if let Some(li) = e.link_idx {
                    link_indices.push(li);
                }
            }
            EdgeKind::Alight => {
                if let Some((svc_id, stop_id)) = graph.svc_index.svcstop_of_node.get(&from) {
                    alight_events.push((svc_id.clone(), stop_id.clone()));
                }
            }
            EdgeKind::Transfer => {
                transfer_edges += 1.0;
                let pen = e.transfer_penalty_s.unwrap_or(0.0);
                transfer_penalty_s += pen;
                transfer_time_s += (raw - pen).max(0.0);
            }
            EdgeKind::Neutral => {}
        }
    }

    let inferred_transfers = (boardings - 1.0_f64).max(0.0_f64);

    Some(BuiltPath {
        link_indices,
        board_events,
        alight_events,
        board_modes,
        board_times_s,
        stats: PathStats {
            gc_s,
            walk_s,
            wait_s,
            ivt_s,
            fare_base: 0.0,
            transfer_time_s,
            transfer_penalty_s,
            transfer_count: inferred_transfers.max(transfer_edges),
            boardings,
        },
    })
}

pub(crate) fn dedupe_paths(paths: Vec<BuiltPath>) -> Vec<BuiltPath> {
    // Dedupe on "physical ride sequence": the ordered list of physical link indices.
    // If two candidates share the same physical sequence, keep the lower-GC one.
    let mut best: HashMap<Vec<usize>, BuiltPath> = HashMap::new();

    for p in paths {
        let key = p.link_indices.clone();
        match best.get(&key) {
            None => {
                best.insert(key, p);
            }
            Some(existing) => {
                if p.stats.gc_s < existing.stats.gc_s {
                    best.insert(key, p);
                }
            }
        }
    }

    // Return paths sorted by GC (nice for determinism + debug readability)
    let mut out: Vec<BuiltPath> = best.into_values().collect();
    out.sort_by(|a, b| {
        a.stats
            .gc_s
            .partial_cmp(&b.stats.gc_s)
            .unwrap_or(Ordering::Equal)
    });
    out
}

fn shortest_path_nodes(graph: &Graph, start: usize, goal: usize) -> Option<(Vec<usize>, f64)> {
    let dist = dijkstra(graph, start);
    if !dist[goal].dist.is_finite() {
        return None;
    }
    let mut nodes = Vec::new();
    let mut cur = goal;
    nodes.push(cur);
    while cur != start {
        cur = dist[cur].prev?;
        nodes.push(cur);
    }
    nodes.reverse();
    Some((nodes, dist[goal].dist))
}

fn dijkstra_with_bans(
    graph: &Graph,
    start: usize,
    banned_nodes: &std::collections::HashSet<usize>,
    banned_edges: &std::collections::HashSet<(usize, usize)>,
) -> Vec<DistEntry> {
    let n = graph.adj.len();
    let mut dist = vec![
        DistEntry {
            dist: f64::INFINITY,
            prev: None,
            prev_edge_kind: None,
            prev_edge_raw_time: 0.0,
            prev_edge_gc: 0.0,
            via_link: None,
            prev_edge_transfer_penalty_s: 0.0,
        };
        n
    ];

    if banned_nodes.contains(&start) {
        return dist;
    }

    dist[start].dist = 0.0;
    let mut heap = BinaryHeap::new();
    heap.push(DState {
        node: start,
        dist: 0.0,
    });

    while let Some(DState { node, dist: d }) = heap.pop() {
        if d > dist[node].dist {
            continue;
        }
        for e in &graph.adj[node] {
            if banned_nodes.contains(&e.to) {
                continue;
            }
            if banned_edges.contains(&(node, e.to)) {
                continue;
            }
            let nd = d + e.gc_s;
            if nd < dist[e.to].dist {
                dist[e.to].dist = nd;
                dist[e.to].prev = Some(node);
                dist[e.to].prev_edge_kind = Some(e.kind);
                dist[e.to].prev_edge_raw_time = e.raw_time_s;
                dist[e.to].prev_edge_gc = e.gc_s;
                dist[e.to].via_link = e.link_idx;
                dist[e.to].prev_edge_transfer_penalty_s = if matches!(e.kind, EdgeKind::Transfer) {
                    e.transfer_penalty_s.unwrap_or(0.0)
                } else {
                    0.0
                };

                heap.push(DState {
                    node: e.to,
                    dist: nd,
                });
            }
        }
    }
    dist
}

fn reconstruct_nodes_from_dist(
    dist: &[DistEntry],
    start: usize,
    goal: usize,
) -> Option<Vec<usize>> {
    if !dist[goal].dist.is_finite() {
        return None;
    }
    let mut nodes = Vec::new();
    let mut cur = goal;
    nodes.push(cur);
    while cur != start {
        cur = dist[cur].prev?;
        nodes.push(cur);
    }
    nodes.reverse();
    Some(nodes)
}

pub(crate) fn k_shortest_paths(
    graph: &Graph,
    start: usize,
    goal: usize,
    k: usize,
) -> Vec<BuiltPath> {
    use std::collections::{BinaryHeap as BH, HashSet};

    let k = k.max(1);
    let mut a: Vec<Vec<usize>> = Vec::new();

    let Some((p0, c0)) = shortest_path_nodes(graph, start, goal) else {
        return vec![];
    };
    a.push(p0);
    let _ = c0;

    #[derive(Clone, Debug)]
    struct Cand {
        cost: f64,
        nodes: Vec<usize>,
    }
    impl Eq for Cand {}
    impl PartialEq for Cand {
        fn eq(&self, other: &Self) -> bool {
            self.cost == other.cost
        }
    }
    impl Ord for Cand {
        fn cmp(&self, other: &Self) -> Ordering {
            other
                .cost
                .partial_cmp(&self.cost)
                .unwrap_or(Ordering::Equal)
        }
    }
    impl PartialOrd for Cand {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let mut b: BH<Cand> = BH::new();
    let mut seen: HashSet<Vec<usize>> = HashSet::new();
    seen.insert(a[0].clone());

    for k_i in 1..k {
        let prev = &a[k_i - 1];

        for spur_idx in 0..prev.len().saturating_sub(1) {
            let spur_node = prev[spur_idx];
            let root_path = &prev[..=spur_idx];

            // Ban nodes in the root path except the spur node to avoid loops
            let mut banned_nodes: HashSet<usize> = HashSet::new();
            for &n in &root_path[..root_path.len().saturating_sub(1)] {
                banned_nodes.insert(n);
            }

            // Ban edges that would recreate an already accepted path sharing this root
            let mut banned_edges: HashSet<(usize, usize)> = HashSet::new();
            for p in &a {
                if p.len() > spur_idx + 1 && p[..=spur_idx] == *root_path {
                    banned_edges.insert((p[spur_idx], p[spur_idx + 1]));
                }
            }

            let dist = dijkstra_with_bans(graph, spur_node, &banned_nodes, &banned_edges);
            let Some(mut spur_path) = reconstruct_nodes_from_dist(&dist, spur_node, goal) else {
                continue;
            };

            // Concatenate root (excluding spur node duplicate) + spur
            let mut total = root_path[..root_path.len().saturating_sub(1)].to_vec();
            total.append(&mut spur_path);

            if !seen.insert(total.clone()) {
                continue;
            }

            // cost = sum of best edges along total
            let mut cost = 0.0;
            let mut ok = true;
            for w in total.windows(2) {
                if let Some(e) = best_edge_between(graph, w[0], w[1]) {
                    cost += e.gc_s;
                } else {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }

            b.push(Cand { cost, nodes: total });
        }

        // Next best candidate becomes the next accepted path
        let Some(next) = b.pop() else {
            break;
        };
        a.push(next.nodes);
        let _ = next.cost;
    }

    a.into_iter()
        .filter_map(|nodes| build_path_from_nodes(graph, &nodes))
        .collect()
}

fn add_mode_aware_interchanges(
    s: &Scenario,
    adj: &mut [Vec<Edge>],
    stop_index: &HashMap<String, usize>,
    stop_in: &dyn Fn(usize) -> usize,
) -> Result<(), String> {
    let Some(rules) = &s.world.transfer_rules else {
        return Ok(());
    };

    // stop_id -> list of modes that serve it (from services)
    let mut modes_serving: HashMap<String, Vec<String>> = HashMap::new();
    for svc in &s.world.services {
        if !service_is_active_for_graph(svc) {
            continue;
        }
        for sid in &svc.stop_sequence {
            if stop_index.contains_key(sid) {
                let entry = modes_serving.entry(sid.clone()).or_default();
                if !entry.contains(&svc.mode) {
                    entry.push(svc.mode.clone());
                }
            }
        }
    }

    // group stops by interchange_id
    let mut groups: HashMap<String, Vec<&Stop>> = HashMap::new();
    for st in &s.world.stops {
        if let Some(gid) = &st.interchange_id {
            groups.entry(gid.clone()).or_default().push(st);
        }
    }

    for (_gid, stops) in groups {
        for a in &stops {
            for b in &stops {
                if a.id == b.id {
                    continue;
                }

                let Some(&a_si) = stop_index.get(&a.id) else {
                    continue;
                };
                let Some(&b_si) = stop_index.get(&b.id) else {
                    continue;
                };

                let a_modes = modes_serving.get(&a.id).cloned().unwrap_or_default();
                let b_modes = modes_serving.get(&b.id).cloned().unwrap_or_default();
                if a_modes.is_empty() || b_modes.is_empty() {
                    continue;
                }

                let dist_m = hypot(a.x - b.x, a.y - b.y);

                for from_mode in &a_modes {
                    for to_mode in &b_modes {
                        let Some(rule) = rules
                            .iter()
                            .find(|r| r.from_mode == *from_mode && r.to_mode == *to_mode)
                        else {
                            continue;
                        };

                        if let Some(maxd) = rule.max_distance_m {
                            if dist_m > maxd {
                                continue;
                            }
                        }

                        let walk_time_s = dist_m / rule.walk_speed_mps.max(0.1);
                        let time_s = rule.base_time_s + walk_time_s;
                        let pen_s = rule.penalty_s;

                        let raw = time_s + pen_s;
                        let gc = raw; // penalty already in seconds-equivalent

                        adj[stop_in(a_si)].push(Edge {
                            to: stop_in(b_si),
                            gc_s: gc,
                            raw_time_s: raw,
                            kind: EdgeKind::Transfer,
                            link_idx: None,
                            transfer_penalty_s: Some(pen_s),
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

pub(crate) fn crowding_multiplier(
    load: f64,
    capacity_per_hour: Option<f64>,
    time_period_hours: f64,
) -> f64 {
    let cap_per_hour = capacity_per_hour.unwrap_or(0.0);
    if cap_per_hour <= 0.0 || time_period_hours <= 0.0 {
        return 1.0;
    }

    let cap = cap_per_hour * time_period_hours;
    if cap <= 0.0 {
        return 1.0;
    }

    let ratio = load / cap;

    if ratio <= 1.0 {
        1.0
    } else {
        // BPR-style multiplier (stable, proportional to IVT)
        let alpha = 0.15;
        let beta = 4.0;
        1.0 + alpha * ratio.powf(beta)
    }
}
