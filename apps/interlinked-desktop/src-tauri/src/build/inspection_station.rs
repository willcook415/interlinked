use std::collections::HashMap;

use interlinked_engine::model::Scenario;
use interlinked_engine::sim::SimulationOutput;
use serde::{Deserialize, Serialize};

use super::fleet_state::stop_display_name;
use super::inspection_line::compute_lines;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationJourneyTime {
    pub stop_id: String,
    pub stop_name: String,
    pub travel_time_s: f64,
    pub stops_away: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationLineSummary {
    pub line_id: String,
    pub line_name: String,
    pub mode: String,
    pub mode_variant: Option<String>,
    pub display_color: Option<String>,
    pub station_index: usize,
    pub station_count: usize,
    pub previous_station_name: Option<String>,
    pub next_station_name: Option<String>,
    pub journey_times: Vec<StationJourneyTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationInspection {
    pub stop_id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub stop_type: Option<String>,
    pub interchange_id: Option<String>,
    pub boardings_attempted: f64,
    pub boardings_served: f64,
    pub alightings_served: f64,
    pub denied_boardings: f64,
    pub queue_end: f64,
    pub station_load_current_pax: f64,
    pub station_capacity_boarding_pph: f64,
    pub station_capacity_alighting_pph: f64,
    pub station_queue_capacity_pax: f64,
    pub overflow_dropped: f64,
    pub passengers_declined_last_hour: f64,
    pub station_entries_per_hour: f64,
    pub station_exits_per_hour: f64,
    pub average_wait_to_board_s: f64,
    pub catchment_radius_m: f64,
    pub catchment_cells: usize,
    pub catchment_residents: f64,
    pub catchment_jobs: f64,
    pub catchment_mix_residential: f64,
    pub catchment_mix_office: f64,
    pub catchment_mix_retail: f64,
    pub catchment_mix_recreation: f64,
    pub catchment_mix_industrial: f64,
    pub catchment_mix_education: f64,
    pub catchment_mix_health: f64,
    pub served_lines: Vec<StationLineSummary>,
}

pub fn inspect_station_from_scenario(
    scenario: &Scenario,
    output: Option<&SimulationOutput>,
    stop_id: &str,
) -> Result<StationInspection, String> {
    let stop = scenario
        .world
        .stops
        .iter()
        .find(|candidate| candidate.id == stop_id)
        .ok_or_else(|| format!("stop not found: {stop_id}"))?;

    let lines = compute_lines(scenario);
    let board_loads = output
        .map(|value| {
            value
                .board_loads
                .iter()
                .filter(|load| load.stop_id == stop_id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let served_lines = lines
        .iter()
        .filter_map(|line| {
            let station_index = line
                .station_ids
                .iter()
                .position(|candidate| candidate == stop_id)?;
            let stop_lookup = scenario
                .world
                .stops
                .iter()
                .map(|entry| (entry.id.clone(), entry))
                .collect::<HashMap<_, _>>();

            let previous_station_name = station_index
                .checked_sub(1)
                .and_then(|idx| line.station_ids.get(idx))
                .and_then(|id| stop_lookup.get(id))
                .map(|stop| stop_display_name(stop));
            let next_station_name = line
                .station_ids
                .get(station_index + 1)
                .and_then(|id| stop_lookup.get(id))
                .map(|stop| stop_display_name(stop));
            let current_time = line
                .cumulative_time_s_by_stop_id
                .get(stop_id)
                .copied()
                .unwrap_or(0.0);
            let journey_times = line
                .station_ids
                .iter()
                .enumerate()
                .filter_map(|(idx, candidate)| {
                    if candidate == stop_id {
                        return None;
                    }
                    let other = stop_lookup.get(candidate)?;
                    let other_time = line
                        .cumulative_time_s_by_stop_id
                        .get(candidate)
                        .copied()
                        .unwrap_or(current_time);
                    Some(StationJourneyTime {
                        stop_id: candidate.clone(),
                        stop_name: stop_display_name(other),
                        travel_time_s: (other_time - current_time).abs(),
                        stops_away: station_index.abs_diff(idx),
                    })
                })
                .collect::<Vec<_>>();

            Some(StationLineSummary {
                line_id: line.line_id.clone(),
                line_name: line.name.clone(),
                mode: line.mode.clone(),
                mode_variant: line.mode_variant.clone(),
                display_color: line.display_color.clone(),
                station_index,
                station_count: line.station_ids.len(),
                previous_station_name,
                next_station_name,
                journey_times,
            })
        })
        .collect::<Vec<_>>();

    let catchment_radius_m = scenario.params.access_radius_m.max(0.0);
    let effective_radius = catchment_radius_m.max(1.0);
    let mut effective_catchment_radius_m = catchment_radius_m;
    let mut catchment_cells = 0usize;
    let mut catchment_residents = 0.0_f64;
    let mut catchment_jobs = 0.0_f64;
    let mut mix_weight_sum = 0.0_f64;
    let mut mix_residential_sum = 0.0_f64;
    let mut mix_office_sum = 0.0_f64;
    let mut mix_retail_sum = 0.0_f64;
    let mut mix_recreation_sum = 0.0_f64;
    let mut mix_industrial_sum = 0.0_f64;
    let mut mix_education_sum = 0.0_f64;
    let mut mix_health_sum = 0.0_f64;
    let mut selected_cells = Vec::<(usize, f64)>::new();
    for (idx, c) in scenario.world.demand_cells.iter().enumerate() {
        let dx = c.x - stop.x;
        let dy = c.y - stop.y;
        let dist2 = dx * dx + dy * dy;
        if dist2 > effective_radius * effective_radius {
            continue;
        }
        let dist = dist2.sqrt();
        let proximity = (1.0 - dist / effective_radius).clamp(0.0, 1.0);
        if proximity <= 0.0 {
            continue;
        }
        let decay_weight = proximity * proximity;
        selected_cells.push((idx, decay_weight));
    }
    if selected_cells.is_empty() && !scenario.world.demand_cells.is_empty() {
        // Sparse legacy surfaces can leave no cells in strict access radius.
        // Use nearest-cell soft fallback rather than hard [100% residential].
        let mut nearest = scenario
            .world
            .demand_cells
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                let dx = c.x - stop.x;
                let dy = c.y - stop.y;
                let dist = (dx * dx + dy * dy).sqrt();
                (idx, dist)
            })
            .collect::<Vec<_>>();
        nearest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let nearest_scale =
            effective_radius.max(nearest.first().map(|x| x.1).unwrap_or(1.0).max(1.0));
        for (idx, dist) in nearest.into_iter().take(24) {
            let rel = dist / nearest_scale;
            let decay_weight = 1.0 / (1.0 + rel * rel);
            if decay_weight <= 0.0 {
                continue;
            }
            effective_catchment_radius_m = effective_catchment_radius_m.max(dist);
            selected_cells.push((idx, decay_weight));
        }
    }
    for (idx, decay_weight) in selected_cells {
        let c = &scenario.world.demand_cells[idx];
        let residents = c.residents_night.max(0.0);
        let jobs = c.jobs_day.max(0.0);
        let activity_mass = (residents + jobs).max(1.0);
        let mix_weight = decay_weight * activity_mass;
        catchment_cells += 1;
        catchment_residents += residents * decay_weight;
        catchment_jobs += jobs * decay_weight;
        mix_weight_sum += mix_weight;
        mix_residential_sum += c.activity_mix_residential.max(0.0) * mix_weight;
        mix_office_sum += c.activity_mix_office.max(0.0) * mix_weight;
        mix_retail_sum += c.activity_mix_retail.max(0.0) * mix_weight;
        mix_recreation_sum += c.activity_mix_recreation.max(0.0) * mix_weight;
        mix_industrial_sum += c.activity_mix_industrial.max(0.0) * mix_weight;
        mix_education_sum += c.activity_mix_education.max(0.0) * mix_weight;
        mix_health_sum += c.activity_mix_health.max(0.0) * mix_weight;
    }
    let mut mix_values = if mix_weight_sum > 0.0 {
        [
            mix_residential_sum / mix_weight_sum,
            mix_office_sum / mix_weight_sum,
            mix_retail_sum / mix_weight_sum,
            mix_recreation_sum / mix_weight_sum,
            mix_industrial_sum / mix_weight_sum,
            mix_education_sum / mix_weight_sum,
            mix_health_sum / mix_weight_sum,
        ]
    } else {
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    };
    for v in &mut mix_values {
        if !v.is_finite() || *v < 0.0 {
            *v = 0.0;
        }
    }
    let mix_sum: f64 = mix_values.iter().sum();
    if mix_sum > 0.0 {
        for v in &mut mix_values {
            *v /= mix_sum;
        }
    } else {
        mix_values = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    }

    let mut boardings_attempted = 0.0_f64;
    let mut boardings_served = 0.0_f64;
    let mut alightings_served = 0.0_f64;
    let mut denied_boardings = 0.0_f64;
    let mut queue_end = 0.0_f64;
    let mut station_load_current_pax = 0.0_f64;
    let mut station_capacity_boarding_pph = 0.0_f64;
    let mut station_capacity_alighting_pph = 0.0_f64;
    let mut station_queue_capacity_pax = 0.0_f64;
    let mut overflow_dropped = 0.0_f64;
    let mut passengers_declined_last_hour = 0.0_f64;
    let mut station_entries_per_hour = 0.0_f64;
    let mut station_exits_per_hour = 0.0_f64;
    let mut weighted_wait_sum_s = 0.0_f64;
    let mut weighted_wait_pax = 0.0_f64;

    for load in &board_loads {
        let arrivals = if load.arrivals.is_finite() {
            load.arrivals.max(0.0)
        } else {
            0.0
        };
        let served_arrivals = if load.served_from_arrivals.is_finite() {
            load.served_from_arrivals.max(0.0)
        } else {
            0.0
        };
        let served_queue = if load.served_from_queue.is_finite() {
            load.served_from_queue.max(0.0)
        } else {
            0.0
        };
        let served_total = served_arrivals + served_queue;
        let alighted = if load.alightings_served.is_finite() {
            load.alightings_served.max(0.0)
        } else {
            0.0
        };
        let denied = if load.denied_boardings.is_finite() {
            load.denied_boardings.max(0.0)
        } else {
            0.0
        };
        let queue_value = if load.queue_end.is_finite() {
            load.queue_end.max(0.0)
        } else {
            0.0
        };
        let queue_capacity = if load.station_queue_capacity_pax.is_finite() {
            load.station_queue_capacity_pax.max(0.0)
        } else {
            0.0
        };
        let overflow = if load.overflow_dropped.is_finite() {
            load.overflow_dropped.max(0.0)
        } else {
            0.0
        };
        let wait_s = if load.extra_wait_s.is_finite() {
            load.extra_wait_s.max(0.0)
        } else {
            0.0
        };
        let period_s =
            if load.departures_observed > 0 && load.headway_s.is_finite() && load.headway_s > 0.0 {
                ((load.departures_observed as f64) * load.headway_s).max(1.0)
            } else if load.departures_in_period.is_finite()
                && load.departures_in_period > 0.0
                && load.headway_s.is_finite()
                && load.headway_s > 0.0
            {
                (load.departures_in_period * load.headway_s).max(1.0)
            } else {
                300.0
            };
        let to_hour = 3600.0 / period_s.max(1.0);
        let admitted_entries = (arrivals - overflow).max(0.0);

        boardings_attempted += arrivals;
        boardings_served += served_total;
        alightings_served += alighted;
        denied_boardings += denied;
        queue_end += queue_value;
        station_load_current_pax += queue_value;
        station_capacity_boarding_pph += if load.station_capacity_boarding_pph.is_finite() {
            load.station_capacity_boarding_pph.max(0.0)
        } else {
            0.0
        };
        station_capacity_alighting_pph += if load.station_capacity_alighting_pph.is_finite() {
            load.station_capacity_alighting_pph.max(0.0)
        } else {
            0.0
        };
        station_queue_capacity_pax += queue_capacity;
        overflow_dropped += overflow;
        passengers_declined_last_hour += overflow * to_hour;
        station_entries_per_hour += admitted_entries * to_hour;
        station_exits_per_hour += alighted * to_hour;
        weighted_wait_sum_s += wait_s * served_total;
        weighted_wait_pax += served_total;
    }

    if station_queue_capacity_pax > 0.0 {
        queue_end = queue_end.min(station_queue_capacity_pax);
        station_load_current_pax = station_load_current_pax.min(station_queue_capacity_pax);
    }
    let average_wait_to_board_s = if weighted_wait_pax > 0.0 {
        weighted_wait_sum_s / weighted_wait_pax
    } else {
        0.0
    };

    Ok(StationInspection {
        stop_id: stop.id.clone(),
        name: stop_display_name(stop),
        x: stop.x,
        y: stop.y,
        stop_type: stop.stop_type.clone(),
        interchange_id: stop.interchange_id.clone(),
        boardings_attempted,
        boardings_served,
        alightings_served,
        denied_boardings,
        queue_end,
        station_load_current_pax,
        station_capacity_boarding_pph,
        station_capacity_alighting_pph,
        station_queue_capacity_pax,
        overflow_dropped,
        passengers_declined_last_hour,
        station_entries_per_hour,
        station_exits_per_hour,
        average_wait_to_board_s,
        catchment_radius_m: effective_catchment_radius_m,
        catchment_cells,
        catchment_residents,
        catchment_jobs,
        catchment_mix_residential: mix_values[0],
        catchment_mix_office: mix_values[1],
        catchment_mix_retail: mix_values[2],
        catchment_mix_recreation: mix_values[3],
        catchment_mix_industrial: mix_values[4],
        catchment_mix_education: mix_values[5],
        catchment_mix_health: mix_values[6],
        served_lines,
    })
}
