# Demand & Passenger Flow Backbone (Phase 1)

This note documents the Phase 1 source-of-truth demand/flow implementation in `interlinked-engine`.

## What Was Added

- New authoritative demand/flow structs were added in:
  - `crates/interlinked-engine/src/sim/types.rs`
- These include:
  - `TripPurpose`
  - `ZoneDemandProfile`
  - `LatentOdDemand`
  - `AssignedOdFlow` (+ `AssignedPathSummary`)
  - `StopFlowState`
  - `VehicleLoadState`
  - `DemandDiagnostics` (+ consistency and layer support structs)
- `SimulationOutput` now includes:
  - `zone_demand_profiles`
  - `latent_od_demand`
  - `assigned_od_flows`
  - `stop_flow_states`
  - `vehicle_load_states`
  - `zone_demand_layer`
  - `service_load_layer`
  - `demand_diagnostics`

## Where Latent Demand Is Generated

- File: `crates/interlinked-engine/src/sim/planner.rs`
- Function:
  - `build_latent_demand_foundation(...)`
- Process:
  - Builds `ZoneDemandProfile` from zone + demand-cell attributes.
  - Generates OD latent demand by `TripPurpose` and canonical time slices:
    - AM peak, Interpeak, PM peak, Evening
  - Uses gravity-style weighting:
    - destination attraction x exp(-beta * generalized_cost)
  - Produces:
    - all-slice latent table (`latent_od_demand`)
    - active-slice OD table used by assignment (source of truth for this step)

## Where Realised Demand Is Assigned

- File: `crates/interlinked-engine/src/sim/planner.rs`
- Function:
  - `run_simulation_with_settings(...)`
- Process:
  - Assignment loop consumes active latent OD records.
  - Uses `k_shortest_paths` + logit shares for path choice.
  - Capacity-thins at boarding/alighting constraints.
  - Produces `assigned_od_flows` with:
    - `assigned_passengers`
    - `unserved_passengers`
    - `suppressed_passengers`
    - path summaries

## Where Station & Vehicle Counts Are Updated

- Station state:
  - Built from final board-load + passenger-cohort state in `run_simulation_with_settings(...)`.
  - Exported as `stop_flow_states` with:
    - waiting by destination
    - boarded/alighted/denied
    - arrivals completed
- Vehicle state:
  - Derived service-by-service, stop-by-stop from authoritative board/alight totals.
  - Exported as `vehicle_load_states` with:
    - current load
    - board/alight per stop
    - load after stop
    - max load seen
    - crowding ratio

## Source-of-Truth Query Surfaces (for future UI/map layers)

All future UI passenger counters should read from `SimulationOutput` fields above, especially:

- Zone layers:
  - `zone_demand_layer`
- Link/service load:
  - `link_loads`
  - `service_load_layer`
- Station live state:
  - `stop_flow_states`
- Vehicle live state:
  - `vehicle_load_states`
- Debug + reconciliation:
  - `demand_diagnostics`
  - `demand_diagnostics.consistency_checks`

## Verification Assets Added

- New fixture:
  - `crates/interlinked-engine/tests/fixtures/scenario_demand_flow_phase1.json`
- New tests:
  - `crates/interlinked-engine/tests/demand_flow_foundation_phase1.rs`
  - Verifies latent generation, assignment, waiting/denied behavior, unserved tracking, and consistency checks.
