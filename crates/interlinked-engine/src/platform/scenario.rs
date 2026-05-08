use std::collections::{HashMap, HashSet};

use crate::io::IoError;
use crate::model::{DemandCell, Scenario, Zone};
use crate::write_json_to_path;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error(transparent)]
    Io(#[from] IoError),

    #[error("scenario validation failed:\n{0}")]
    Validation(String),

    #[error("scenario migration failed:\n{0}")]
    Migration(String),
}

/// Versioned wrapper around your Scenario.
/// This is the contract that becomes stable forever.
#[derive(Debug, Clone)]
pub struct ScenarioDocument {
    pub schema_version: u32,
    pub scenario: Scenario,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioDocumentWire {
    pub schema_version: u32,
    pub scenario: Scenario,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioFileShape {
    Wrapped,
    LegacyFlat,
}

impl ScenarioDocument {
    pub const CURRENT_SCHEMA_VERSION: u32 = 3;

    pub fn new_current(scenario: Scenario) -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            scenario,
        }
    }
}

/// The “platform API” surface for scenario IO, validation and migration.
/// UI, tooling, and batch runners call this, not io.rs directly.
pub struct ScenarioService;

impl ScenarioService {
    pub fn load_from_path_with_shape(
        path: &str,
    ) -> Result<(ScenarioDocument, ScenarioFileShape), ScenarioError> {
        let bytes = std::fs::read(path).map_err(IoError::from)?;

        if let Ok(wire) = serde_json::from_slice::<ScenarioDocumentWire>(&bytes) {
            let doc = ScenarioDocument {
                schema_version: wire.schema_version,
                scenario: wire.scenario,
            };
            let migrated = Self::migrate_to_current(doc)?;
            Self::validate(&migrated.scenario)?;
            return Ok((migrated, ScenarioFileShape::Wrapped));
        }

        let scenario = serde_json::from_slice::<Scenario>(&bytes).map_err(IoError::from)?;
        let doc = ScenarioDocument::new_current(scenario);
        Self::validate(&doc.scenario)?;
        Ok((doc, ScenarioFileShape::LegacyFlat))
    }

    /// Load from path (legacy: scenario.json is just Scenario today).
    /// Supports both wrapped ScenarioDocument and legacy flat Scenario files.
    pub fn load_from_path(path: &str) -> Result<ScenarioDocument, ScenarioError> {
        let (doc, _) = Self::load_from_path_with_shape(path)?;
        Ok(doc)
    }

    pub fn save_to_path(path: &str, doc: &ScenarioDocument) -> Result<(), ScenarioError> {
        let wire = ScenarioDocumentWire {
            schema_version: doc.schema_version,
            scenario: doc.scenario.clone(),
        };
        write_json_to_path(path, &wire)?;
        Ok(())
    }

    pub fn validate(s: &Scenario) -> Result<(), ScenarioError> {
        validate_scenario(s).map_err(ScenarioError::Validation)
    }

    /// Migration stub. When you add schema changes, implement vN -> vN+1 here.
    pub fn migrate_to_current(doc: ScenarioDocument) -> Result<ScenarioDocument, ScenarioError> {
        if doc.schema_version == ScenarioDocument::CURRENT_SCHEMA_VERSION {
            return Ok(doc);
        }
        if doc.schema_version == 2 {
            let mut out = doc.scenario.clone();
            if out.world.demand_meta.is_none() {
                let loaded_countries = out
                    .world
                    .demand_cells
                    .iter()
                    .filter_map(|c| c.country_iso2.as_ref())
                    .map(|c| c.trim().to_ascii_uppercase())
                    .filter(|c| c.len() == 2)
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                out.world.demand_meta = Some(crate::model::DemandMeta {
                    surface_version: "legacy-v2".to_string(),
                    loaded_countries,
                    source: if out.world.demand_cells.is_empty() {
                        "zones_only".to_string()
                    } else {
                        "legacy_synthetic".to_string()
                    },
                });
            }
            return Ok(ScenarioDocument {
                schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
                scenario: out,
            });
        }
        if doc.schema_version == 1 {
            let mut out = doc.scenario.clone();
            if out.world.demand_cells.is_empty() && !out.world.zones.is_empty() {
                out.world.demand_cells = out
                    .world
                    .zones
                    .iter()
                    .map(|z| DemandCell {
                        cell_id: z.id.clone(),
                        x: z.x,
                        y: z.y,
                        area_m2: 0.0,
                        residents_night: z.population.max(0.0),
                        jobs_day: z.jobs.max(0.0),
                        activity_mix_residential: 0.45,
                        activity_mix_office: 0.18,
                        activity_mix_retail: 0.12,
                        activity_mix_recreation: 0.08,
                        activity_mix_industrial: 0.10,
                        activity_mix_education: 0.04,
                        activity_mix_health: 0.03,
                        centrality_score: 0.0,
                        data_quality_score: 0.5,
                        country_iso2: z.country_iso2.clone(),
                        allocation_diagnostics: None,
                    })
                    .collect();
            } else if !out.world.demand_cells.is_empty() && out.world.zones.is_empty() {
                out.world.zones = out
                    .world
                    .demand_cells
                    .iter()
                    .map(|c| Zone {
                        id: c.cell_id.clone(),
                        x: c.x,
                        y: c.y,
                        population: c.residents_night.max(0.0),
                        jobs: c.jobs_day.max(0.0),
                        country_iso2: c.country_iso2.clone(),
                    })
                    .collect();
            }
            if out.world.demand_meta.is_none() {
                out.world.demand_meta = Some(crate::model::DemandMeta {
                    surface_version: "migrated-v1".to_string(),
                    loaded_countries: vec![],
                    source: if out.world.demand_cells.is_empty() {
                        "zones_only".to_string()
                    } else {
                        "legacy_synthetic".to_string()
                    },
                });
            }
            return Ok(ScenarioDocument {
                schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
                scenario: out,
            });
        }
        Err(ScenarioError::Migration(format!(
            "unsupported schema_version {} (current {})",
            doc.schema_version,
            ScenarioDocument::CURRENT_SCHEMA_VERSION
        )))
    }
}

pub(crate) fn validate_scenario(s: &Scenario) -> Result<(), String> {
    let mut errors: Vec<String> = vec![];

    if s.meta.name.trim().is_empty() {
        errors.push("meta.name must not be empty".to_string());
    }

    // Unique IDs
    ensure_unique_ids("zone", s.world.zones.iter().map(|z| &z.id), &mut errors);
    ensure_unique_ids("stop", s.world.stops.iter().map(|x| &x.id), &mut errors);
    ensure_unique_ids("link", s.world.links.iter().map(|x| &x.id), &mut errors);
    ensure_unique_ids(
        "service",
        s.world.services.iter().map(|x| &x.id),
        &mut errors,
    );

    // Reference sets
    let stop_ids: HashSet<&str> = s.world.stops.iter().map(|x| x.id.as_str()).collect();
    let _mode_set: HashSet<&str> = s.world.links.iter().map(|l| l.mode.as_str()).collect();

    // Links reference valid stops
    for l in &s.world.links {
        if !stop_ids.contains(l.from_stop.as_str()) {
            errors.push(format!(
                "link {} from_stop '{}' not found",
                l.id, l.from_stop
            ));
        }
        if !stop_ids.contains(l.to_stop.as_str()) {
            errors.push(format!("link {} to_stop '{}' not found", l.id, l.to_stop));
        }
        if l.distance_m <= 0.0 {
            errors.push(format!("link {} distance_m must be > 0", l.id));
        }
        if l.speed_mps <= 0.0 {
            errors.push(format!("link {} speed_mps must be > 0", l.id));
        }
    }

    // Services stop_sequence references valid stops
    for sv in &s.world.services {
        if sv.stop_sequence.len() < 2 {
            errors.push(format!(
                "service {} stop_sequence must have >= 2 stops",
                sv.id
            ));
        }
        for st in &sv.stop_sequence {
            if !stop_ids.contains(st.as_str()) {
                errors.push(format!(
                    "service {} references missing stop '{}'",
                    sv.id, st
                ));
            }
        }
        if sv.headway_s <= 0.0 {
            errors.push(format!("service {} headway_s must be > 0", sv.id));
        }
        if sv.vehicle_capacity <= 0.0 {
            errors.push(format!("service {} vehicle_capacity must be > 0", sv.id));
        }
        if let Some(profile) = &sv.rolling_stock_profile {
            if let Some(value) = profile.package_id.as_ref() {
                if value.trim().is_empty() {
                    errors.push(format!(
                        "service {} rolling_stock_profile.package_id must not be empty",
                        sv.id
                    ));
                }
            }
            if let Some(cars) = profile.cars_per_unit {
                if cars == 0 {
                    errors.push(format!(
                        "service {} rolling_stock_profile.cars_per_unit must be > 0 when set",
                        sv.id
                    ));
                }
            }
            if let Some(value) = profile.speed_level.as_ref() {
                if value.trim().is_empty() {
                    errors.push(format!(
                        "service {} rolling_stock_profile.speed_level must not be empty",
                        sv.id
                    ));
                }
            }
            if let Some(value) = profile.comfort_level.as_ref() {
                if value.trim().is_empty() {
                    errors.push(format!(
                        "service {} rolling_stock_profile.comfort_level must not be empty",
                        sv.id
                    ));
                }
            }
            for order in &profile.pending_orders {
                if order.order_id.trim().is_empty() {
                    errors.push(format!(
                        "service {} rolling_stock_profile.pending_orders order_id must not be empty",
                        sv.id
                    ));
                }
                if order.units == 0 {
                    errors.push(format!(
                        "service {} rolling_stock_profile.pending_orders {} units must be > 0",
                        sv.id, order.order_id
                    ));
                }
                if let Some(status) = order.status.as_ref() {
                    if status.trim().is_empty() {
                        errors.push(format!(
                            "service {} rolling_stock_profile.pending_orders {} status must not be empty",
                            sv.id, order.order_id
                        ));
                    }
                }
                if let Some(total) = order.total_cost_base {
                    if !total.is_finite() || total < 0.0 {
                        errors.push(format!(
                            "service {} rolling_stock_profile.pending_orders {} total_cost_base must be finite and >= 0",
                            sv.id, order.order_id
                        ));
                    }
                }
                if let (Some(placed), Some(eta)) = (order.placed_at_tick_s, order.eta_at_tick_s) {
                    if eta + 1e-6 < placed {
                        errors.push(format!(
                            "service {} rolling_stock_profile.pending_orders {} eta_at_tick_s must be >= placed_at_tick_s",
                            sv.id, order.order_id
                        ));
                    }
                }
            }
        }
        if let Some(schedule) = &sv.schedule_profile {
            let minutes = [
                ("peak_start_minute", schedule.peak_start_minute),
                ("peak_end_minute", schedule.peak_end_minute),
                ("overnight_start_minute", schedule.overnight_start_minute),
                ("overnight_end_minute", schedule.overnight_end_minute),
            ];
            for (label, value) in minutes {
                if let Some(v) = value {
                    if v >= 1440 {
                        errors.push(format!(
                            "service {} schedule_profile.{} must be in [0, 1439]",
                            sv.id, label
                        ));
                    }
                }
            }
            if let (Some(start), Some(end)) = (schedule.peak_start_minute, schedule.peak_end_minute)
            {
                if start == end {
                    errors.push(format!(
                        "service {} schedule_profile peak window must not have identical start/end",
                        sv.id
                    ));
                }
            }
            if let (Some(start), Some(end)) = (
                schedule.overnight_start_minute,
                schedule.overnight_end_minute,
            ) {
                if start == end {
                    errors.push(format!(
                        "service {} schedule_profile overnight window must not have identical start/end",
                        sv.id
                    ));
                }
            }
            let tph_values = [
                ("tph_peak", schedule.tph_peak),
                ("tph_off_peak", schedule.tph_off_peak),
                ("tph_overnight", schedule.tph_overnight),
            ];
            for (label, value) in tph_values {
                if let Some(tph) = value {
                    if !tph.is_finite() || tph < 0.0 {
                        errors.push(format!(
                            "service {} schedule_profile.{} must be finite and >= 0",
                            sv.id, label
                        ));
                    }
                }
            }
        }
    }

    // Transfers reference valid stops
    for t in &s.world.transfers {
        if !stop_ids.contains(t.from_stop.as_str()) {
            errors.push(format!("transfer from_stop '{}' not found", t.from_stop));
        }
        if !stop_ids.contains(t.to_stop.as_str()) {
            errors.push(format!("transfer to_stop '{}' not found", t.to_stop));
        }
        if t.time_s < 0.0 {
            errors.push(format!(
                "transfer {} -> {} time_s must be >= 0",
                t.from_stop, t.to_stop
            ));
        }
    }

    // Transfer rules sanity
    if let Some(rules) = &s.world.transfer_rules {
        for r in rules {
            if r.base_time_s < 0.0 {
                errors.push(format!(
                    "transfer_rule {}->{} base_time_s must be >= 0",
                    r.from_mode, r.to_mode
                ));
            }
            if r.walk_speed_mps <= 0.0 {
                errors.push(format!(
                    "transfer_rule {}->{} walk_speed_mps must be > 0",
                    r.from_mode, r.to_mode
                ));
            }
        }
    }

    // Params sanity
    if s.params.access_walk_speed_mps <= 0.0 {
        errors.push("params.access_walk_speed_mps must be > 0".to_string());
    }
    if s.params.access_radius_m <= 0.0 {
        errors.push("params.access_radius_m must be > 0".to_string());
    }
    if s.params.route_choice_k == 0 {
        errors.push("params.route_choice_k must be >= 1".to_string());
    }
    let purpose_sum = s.params.purpose_share_home_work
        + s.params.purpose_share_home_education
        + s.params.purpose_share_home_retail
        + s.params.purpose_share_home_recreation
        + s.params.purpose_share_other;
    if !purpose_sum.is_finite() || purpose_sum <= 0.0 {
        errors.push("params purpose shares must sum to > 0".to_string());
    }

    for (i, slice) in s.params.demand_profile.iter().enumerate() {
        if slice.label.trim().is_empty() {
            errors.push(format!(
                "params.demand_profile[{i}] label must not be empty"
            ));
        }
        if !slice.start_s.is_finite() || !slice.end_s.is_finite() {
            errors.push(format!(
                "params.demand_profile[{i}] start_s/end_s must be finite"
            ));
        }
        if slice.start_s < 0.0 || slice.start_s >= 86_400.0 {
            errors.push(format!(
                "params.demand_profile[{i}] start_s must be in [0, 86400)"
            ));
        }
        if slice.end_s < 0.0 || slice.end_s >= 86_400.0 {
            errors.push(format!(
                "params.demand_profile[{i}] end_s must be in [0, 86400)"
            ));
        }
        if slice.multiplier <= 0.0 || !slice.multiplier.is_finite() {
            errors.push(format!(
                "params.demand_profile[{i}] multiplier must be finite and > 0"
            ));
        }
    }

    for (i, c) in s.world.demand_cells.iter().enumerate() {
        if c.cell_id.trim().is_empty() {
            errors.push(format!("world.demand_cells[{i}] cell_id must not be empty"));
        }
        if !c.x.is_finite() || !c.y.is_finite() {
            errors.push(format!("world.demand_cells[{i}] x/y must be finite"));
        }
        if c.residents_night < 0.0 || !c.residents_night.is_finite() {
            errors.push(format!(
                "world.demand_cells[{i}] residents_night must be finite and >= 0"
            ));
        }
        if c.jobs_day < 0.0 || !c.jobs_day.is_finite() {
            errors.push(format!(
                "world.demand_cells[{i}] jobs_day must be finite and >= 0"
            ));
        }
        let mixes = [
            c.activity_mix_residential,
            c.activity_mix_office,
            c.activity_mix_retail,
            c.activity_mix_recreation,
            c.activity_mix_industrial,
            c.activity_mix_education,
            c.activity_mix_health,
        ];
        if mixes.iter().any(|v| !v.is_finite() || *v < 0.0) {
            errors.push(format!(
                "world.demand_cells[{i}] activity mixes must be finite and >= 0"
            ));
        }
    }

    if let Some(meta) = s.world.demand_meta.as_ref() {
        if meta.surface_version.trim().is_empty() {
            errors.push("world.demand_meta.surface_version must not be empty".to_string());
        }
        if meta.source.trim().is_empty() {
            errors.push("world.demand_meta.source must not be empty".to_string());
        }
        for (i, iso) in meta.loaded_countries.iter().enumerate() {
            let code = iso.trim().to_ascii_uppercase();
            if code.len() != 2 || !code.chars().all(|ch| ch.is_ascii_alphabetic()) {
                errors.push(format!(
                    "world.demand_meta.loaded_countries[{i}] must be ISO2 alpha code"
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Ok(Err(errors.join("\n"))?)
    }
}

fn ensure_unique_ids<'a, I>(label: &str, iter: I, errors: &mut Vec<String>)
where
    I: Iterator<Item = &'a String>,
{
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for id in iter {
        *seen.entry(id.as_str()).or_insert(0) += 1;
    }
    for (id, n) in seen {
        if n > 1 {
            errors.push(format!("{label} id '{}' appears {n} times", id));
        }
    }
}
