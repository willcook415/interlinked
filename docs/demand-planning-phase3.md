# Demand Planning Legibility (Phase 3)

Phase 3 adds authoritative planning overlays, service-gap analysis, station catchments, corridor characterization, and build-preview evaluation on top of the existing Phase 1 + Phase 2 backbone.

The source-of-truth chain remains unchanged:

`zones -> latent OD -> assignment -> waiting/denied -> board/alight -> vehicle loads`

No Phase 3 metric is generated from UI-only counters.

## New Output Contract

Added in [`crates/interlinked-engine/src/sim/types.rs`](../crates/interlinked-engine/src/sim/types.rs):

- `PlanningOverlayConfig`
- `ZonePlanningMetrics`
- `StationPlanningMetrics`
- `CorridorPlanningMetrics`
- `LineOrServicePlanningMetrics`
- `BuildPreviewMetrics`
- `ServiceGapRankings`
- `PlanningDebugSummary`
- Supporting enums:
  - `CorridorClassification`
  - `RecommendedServiceClass`
  - `ServiceRoleClassification`
  - `BuildPreviewType`

Exposed through `SimulationOutput`:

- `planning_overlay_config`
- `zone_planning_metrics`
- `station_planning_metrics`
- `corridor_planning_metrics`
- `line_service_planning_metrics`
- `build_preview_metrics`
- `service_gap_rankings`
- `planning_debug_summary`

## Methodology

Implemented in [`crates/interlinked-engine/src/sim/planner.rs`](../crates/interlinked-engine/src/sim/planner.rs), primarily in `build_phase3_planning_outputs(...)`.

### Accessibility and coverage

Zone-level accessibility uses network generalized cost from the simulation graph and purpose-weighted decay thresholds from `PlanningOverlayConfig`.

Computed components:

- `access_to_jobs_score`
- `access_to_services_score`
- `access_to_education_score`
- `access_to_retail_leisure_score`
- `intercity_access_score`
- `composite_accessibility_score`

Service coverage remains grounded in realized vs latent (`total_realised_produced / total_latent_produced`).

### Station catchments

Each stop gets a configurable walk catchment (`station_catchment_radius_m`) and derives:

- catchment population/jobs/intensity pools
- latent/realized/unserved demand in catchment
- load pressure and overcrowding risk from actual waiting/denied/crowding states
- primary purposes served and top destination signals

### Corridor classification

Corridors are aggregated from authoritative OD corridor flows and classified heuristically by:

- dominant purpose
- distance
- settlement hierarchy/archetype context
- attractors (airport/university)
- volume and unserved pressure

Classification output:

- `urban_local`
- `urban_trunk_metro_suitable`
- `suburban_commuter_radial`
- `regional_connector`
- `intercity`
- `rural_essential_connector`
- `airport_access`
- `education_connector`
- `mixed`

### Service/line planning metrics

Service metrics use actual vehicle stop states and assigned OD path overlap:

- boardings, passenger-km proxy, peak/average load
- overloaded segment count
- strongest OD patterns
- service role classification
- utilisation score

### Service-gap rankings

`ServiceGapRankings` provides ranked outputs for:

- underserved zones
- underserved corridors
- overcrowded stations
- overcrowded services
- weak-access rural zones
- high-potential interventions

### Build previews (heuristic but data-grounded)

Preview candidates are generated for:

- new station
- line/segment connector
- service frequency uplift

Preview values are estimated from authoritative inputs (catchments, latent/unserved demand, corridor pressure, accessibility gaps, live load pressure), with explicit confidence + explanation text.

These are intentionally labeled as planning heuristics, not exact post-build forecasts.

## Inspector Consumption Guide

Future UI inspectors should read directly from these fields:

- Zone inspector: `zone_planning_metrics` (+ `zone_demand_production_layer`, `zone_demand_attraction_layer`)
- Station inspector: `station_planning_metrics` (+ `stop_flow_states`)
- Service/line inspector: `line_service_planning_metrics` (+ `vehicle_load_states`, `service_load_layer`)
- Corridor inspector: `corridor_planning_metrics` (+ `corridor_desire_lines`)
- Build preview inspector: `build_preview_metrics` (+ `service_gap_rankings.top_high_potential_interventions`)

## What Is Authoritative vs Heuristic

Fully authoritative live state:

- latent/realized/unserved OD
- waiting/boarded/alighted/denied
- vehicle loads and crowding

Authoritative aggregations:

- zone/station/service/corridor overlays derived from the above
- service-gap rankings

Heuristic (but source-grounded):

- build preview metrics and accessibility delta proxies
- recommended service class and role labels

## Known Limitations

- Preview evaluator does not rerun assignment on edited topology yet; it uses transparent proxy scoring.
- Corridor/service role classification is heuristic and threshold-driven.
- Planning overlay config is engine-defaulted at runtime (not yet scenario-authored/editor-authored).
