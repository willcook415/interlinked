use crate::*;

pub(crate) fn persist_runtime_manifest_now(
    project_root: &Path,
    manifest: &mut ProjectManifest,
) -> Result<(), String> {
    manifest.updated_at = now_string();
    crate::write_manifest(project_root, manifest)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedRuntimeState {
    #[serde(default)]
    pub(crate) tick_s: f64,
    #[serde(default)]
    pub(crate) sim_state: Option<PersistedSimState>,
    #[serde(default)]
    pub(crate) run_cfg: Option<RunConfig>,
    #[serde(default)]
    pub(crate) history: Option<SimHistory>,
    #[serde(default)]
    pub(crate) last_output: Option<SimulationOutput>,
    #[serde(default)]
    pub(crate) last_quick_kpis: Option<Kpis>,
    #[serde(default)]
    pub(crate) runtime_ops: Option<PersistedRuntimeOpsState>,
    #[serde(default)]
    pub(crate) latest_snapshot: Option<RuntimeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedSandboxStateFile {
    #[serde(default)]
    pub(crate) tick_s: f64,
    #[serde(default)]
    pub(crate) runtime: Option<PersistedRuntimeState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedSimState {
    #[serde(default)]
    pub(crate) t_s: f64,
    #[serde(default)]
    pub(crate) queue: Vec<PersistedServiceStopValue>,
    #[serde(default)]
    pub(crate) queue_cohorts: Vec<PersistedServiceStopDestinationValue>,
    #[serde(default)]
    pub(crate) time_to_next_departure_s: Vec<PersistedServiceStopValue>,
    #[serde(default)]
    pub(crate) pending_od_trips: Vec<PersistedZonePairValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedRuntimeOpsState {
    #[serde(default)]
    pub(crate) topology_hash: u64,
    #[serde(default)]
    pub(crate) trains: Vec<RuntimeTrainState>,
    #[serde(default)]
    pub(crate) queue_cohorts: Vec<PersistedServiceStopDestinationValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedServiceStopValue {
    pub(crate) service_id: String,
    pub(crate) stop_id: String,
    pub(crate) value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedServiceStopDestinationValue {
    pub(crate) service_id: String,
    pub(crate) board_stop_id: String,
    pub(crate) destination_stop_id: String,
    pub(crate) value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedZonePairValue {
    pub(crate) origin_zone_id: String,
    pub(crate) destination_zone_id: String,
    pub(crate) value: f64,
}

pub(crate) fn persisted_sim_state_from_sim_state(
    sim_state: &interlinked_engine::sim::SimState,
) -> PersistedSimState {
    let mut queue = sim_state
        .queue
        .iter()
        .filter_map(|((service_id, stop_id), value)| {
            if !value.is_finite() || *value <= 1e-9 {
                return None;
            }
            Some(PersistedServiceStopValue {
                service_id: service_id.clone(),
                stop_id: stop_id.clone(),
                value: *value,
            })
        })
        .collect::<Vec<_>>();
    queue.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.stop_id.cmp(&b.stop_id))
    });

    let mut queue_cohorts = sim_state
        .queue_cohorts
        .iter()
        .filter_map(
            |((service_id, board_stop_id, destination_stop_id), value)| {
                if !value.is_finite() || *value <= 1e-9 {
                    return None;
                }
                Some(PersistedServiceStopDestinationValue {
                    service_id: service_id.clone(),
                    board_stop_id: board_stop_id.clone(),
                    destination_stop_id: destination_stop_id.clone(),
                    value: *value,
                })
            },
        )
        .collect::<Vec<_>>();
    queue_cohorts.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.board_stop_id.cmp(&b.board_stop_id))
            .then_with(|| a.destination_stop_id.cmp(&b.destination_stop_id))
    });

    let mut time_to_next_departure_s = sim_state
        .time_to_next_departure_s
        .iter()
        .filter_map(|((service_id, stop_id), value)| {
            if !value.is_finite() || *value < 0.0 {
                return None;
            }
            Some(PersistedServiceStopValue {
                service_id: service_id.clone(),
                stop_id: stop_id.clone(),
                value: *value,
            })
        })
        .collect::<Vec<_>>();
    time_to_next_departure_s.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.stop_id.cmp(&b.stop_id))
    });

    let mut pending_od_trips = sim_state
        .pending_od_trips
        .iter()
        .filter_map(|((origin_zone_id, destination_zone_id), value)| {
            if !value.is_finite() || *value <= 1e-9 {
                return None;
            }
            Some(PersistedZonePairValue {
                origin_zone_id: origin_zone_id.clone(),
                destination_zone_id: destination_zone_id.clone(),
                value: *value,
            })
        })
        .collect::<Vec<_>>();
    pending_od_trips.sort_by(|a, b| {
        a.origin_zone_id
            .cmp(&b.origin_zone_id)
            .then_with(|| a.destination_zone_id.cmp(&b.destination_zone_id))
    });

    PersistedSimState {
        t_s: if sim_state.t_s.is_finite() {
            sim_state.t_s.max(0.0)
        } else {
            0.0
        },
        queue,
        queue_cohorts,
        time_to_next_departure_s,
        pending_od_trips,
    }
}

pub(crate) fn sim_state_from_persisted(
    scenario: &Scenario,
    persisted: &PersistedSimState,
    fallback_t_s: f64,
) -> interlinked_engine::sim::SimState {
    let mut restored = init_sim_state(scenario, &RunConfig::default());
    let persisted_t = if persisted.t_s.is_finite() && persisted.t_s >= 0.0 {
        persisted.t_s
    } else {
        fallback_t_s.max(0.0)
    };
    restored.t_s = persisted_t;
    restored.queue.clear();
    restored.queue_cohorts.clear();
    restored.pending_od_trips.clear();

    let valid_service_stop_keys = scenario
        .world
        .services
        .iter()
        .flat_map(|service| {
            service
                .stop_sequence
                .iter()
                .cloned()
                .map(move |stop_id| (service.id.clone(), stop_id))
        })
        .collect::<HashSet<_>>();
    let service_stop_set = scenario
        .world
        .services
        .iter()
        .map(|service| {
            (
                service.id.clone(),
                service
                    .stop_sequence
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let valid_zone_ids = scenario
        .world
        .zones
        .iter()
        .map(|zone| zone.id.clone())
        .collect::<HashSet<_>>();

    for entry in &persisted.queue {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        if !valid_service_stop_keys.contains(&(entry.service_id.clone(), entry.stop_id.clone())) {
            continue;
        }
        restored.queue.insert(
            (entry.service_id.clone(), entry.stop_id.clone()),
            entry.value,
        );
    }

    for entry in &persisted.queue_cohorts {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        let Some(stop_set) = service_stop_set.get(&entry.service_id) else {
            continue;
        };
        if !stop_set.contains(&entry.board_stop_id)
            || !stop_set.contains(&entry.destination_stop_id)
        {
            continue;
        }
        restored.queue_cohorts.insert(
            (
                entry.service_id.clone(),
                entry.board_stop_id.clone(),
                entry.destination_stop_id.clone(),
            ),
            entry.value,
        );
    }

    for entry in &persisted.time_to_next_departure_s {
        if !entry.value.is_finite() || entry.value < 0.0 {
            continue;
        }
        if !valid_service_stop_keys.contains(&(entry.service_id.clone(), entry.stop_id.clone())) {
            continue;
        }
        restored.time_to_next_departure_s.insert(
            (entry.service_id.clone(), entry.stop_id.clone()),
            entry.value,
        );
    }

    for entry in &persisted.pending_od_trips {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        if !valid_zone_ids.contains(&entry.origin_zone_id)
            || !valid_zone_ids.contains(&entry.destination_zone_id)
        {
            continue;
        }
        restored.pending_od_trips.insert(
            (
                entry.origin_zone_id.clone(),
                entry.destination_zone_id.clone(),
            ),
            entry.value,
        );
    }

    restored
}

pub(crate) fn persisted_runtime_ops_from_runtime_ops(
    ops: &RuntimeOpsState,
) -> PersistedRuntimeOpsState {
    let mut trains = ops.trains.values().cloned().collect::<Vec<_>>();
    trains.sort_by(|a, b| a.train_id.cmp(&b.train_id));
    let mut queue_cohorts = ops
        .queue_cohorts
        .iter()
        .filter_map(
            |((service_id, board_stop_id, destination_stop_id), value)| {
                if !value.is_finite() || *value <= 1e-9 {
                    return None;
                }
                Some(PersistedServiceStopDestinationValue {
                    service_id: service_id.clone(),
                    board_stop_id: board_stop_id.clone(),
                    destination_stop_id: destination_stop_id.clone(),
                    value: *value,
                })
            },
        )
        .collect::<Vec<_>>();
    queue_cohorts.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.board_stop_id.cmp(&b.board_stop_id))
            .then_with(|| a.destination_stop_id.cmp(&b.destination_stop_id))
    });
    PersistedRuntimeOpsState {
        topology_hash: ops.topology_hash,
        trains,
        queue_cohorts,
    }
}

pub(crate) fn runtime_ops_from_persisted(
    persisted: &PersistedRuntimeOpsState,
    project_path: &str,
) -> RuntimeOpsState {
    let mut trains = BTreeMap::<String, RuntimeTrainState>::new();
    for mut train in persisted.trains.clone() {
        let train_id = train.train_id.trim().to_string();
        if train_id.is_empty() {
            continue;
        }
        if !train.vehicle_capacity.is_finite() || train.vehicle_capacity < 0.0 {
            train.vehicle_capacity = 0.0;
        }
        if !train.progress.is_finite() || train.progress < 0.0 {
            train.progress = 0.0;
        }
        if !train.remaining_s.is_finite() || train.remaining_s < 0.0 {
            train.remaining_s = 0.0;
        }
        if train.direction_step == 0 {
            train.direction_step = 1;
        }
        train
            .onboard_cohorts
            .retain(|_, value| value.is_finite() && *value > 1e-6);
        train.onboard_pax = runtime_train_onboard_total(&train);
        train.train_id = train_id.clone();
        trains.insert(train_id, train);
    }
    let mut queue_cohorts = HashMap::<(String, String, String), f64>::new();
    for entry in &persisted.queue_cohorts {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        queue_cohorts.insert(
            (
                entry.service_id.clone(),
                entry.board_stop_id.clone(),
                entry.destination_stop_id.clone(),
            ),
            entry.value,
        );
    }
    RuntimeOpsState {
        project_path: project_path.to_string(),
        topology_hash: persisted.topology_hash,
        profiles_by_service: HashMap::new(),
        stop_name_by_id: HashMap::new(),
        reverse_service_by_service: HashMap::new(),
        stop_ids_by_service: HashMap::new(),
        fare_base_by_service: HashMap::new(),
        dispatch_service_ids: HashSet::new(),
        trains,
        queue_cohorts,
        last_queue_ingest_by_service_stop: HashMap::new(),
        last_boarding_by_service_stop: HashMap::new(),
    }
}

pub(crate) fn capture_persisted_runtime_state(
    state: &tauri::State<AppState>,
    project_path: &str,
) -> Result<Option<PersistedRuntimeState>, String> {
    if !project_is_current(state, project_path)? {
        return Ok(None);
    }
    let game = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?
        .clone();
    let Some(game_state) = game else {
        return Ok(None);
    };
    let runtime_ops = state
        .runtime_ops
        .lock()
        .map_err(|_| "runtime_ops mutex poisoned".to_string())?
        .as_ref()
        .filter(|ops| ops.project_path == project_path)
        .map(persisted_runtime_ops_from_runtime_ops);
    let latest_snapshot = latest_runtime_snapshot_for_project(state.inner(), project_path)?;
    let tick_s = if game_state.tick_s.is_finite() && game_state.tick_s >= 0.0 {
        game_state.tick_s
    } else {
        0.0
    };
    Ok(Some(PersistedRuntimeState {
        tick_s,
        sim_state: Some(persisted_sim_state_from_sim_state(&game_state.sim_state)),
        run_cfg: Some(game_state.run_cfg.clone()),
        history: None,
        last_output: None,
        last_quick_kpis: None,
        runtime_ops,
        latest_snapshot,
    }))
}

pub(crate) fn apply_persisted_runtime_state_to_game(
    game_state: &mut interlinked_engine::platform::GameState,
    scenario: &Scenario,
    persisted: &PersistedRuntimeState,
) {
    game_state.kernel_state = interlinked_engine::platform::KernelPartitionState::default();
    if let Some(run_cfg) = persisted.run_cfg.as_ref() {
        game_state.run_cfg = run_cfg.clone();
    }
    let fallback_tick = if persisted.tick_s.is_finite() && persisted.tick_s >= 0.0 {
        persisted.tick_s
    } else {
        game_state.tick_s.max(0.0)
    };
    if let Some(sim_state) = persisted.sim_state.as_ref() {
        game_state.sim_state = sim_state_from_persisted(scenario, sim_state, fallback_tick);
    } else {
        game_state.sim_state.t_s = fallback_tick;
    }
    if game_state.sim_state.t_s.is_finite() && game_state.sim_state.t_s >= 0.0 {
        game_state.tick_s = game_state.sim_state.t_s;
    } else {
        game_state.tick_s = fallback_tick;
        game_state.sim_state.t_s = fallback_tick;
    }
    game_state.run_cfg.deterministic_seed = Some(scenario.meta.seed);
    if let Some(history) = persisted.history.as_ref() {
        game_state.history = history.clone();
    }
    game_state.last_quick_kpis = persisted.last_quick_kpis.clone();
    game_state.last_output = persisted.last_output.clone();
}
