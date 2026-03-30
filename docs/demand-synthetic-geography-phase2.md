# Demand & Passenger Flow Backbone (Phase 2)

Phase 2 upgrades latent demand generation with synthetic economic geography while preserving the Phase 1 authoritative accounting chain:

`zones -> latent OD -> assignment -> stop/platform queues -> boarding/alighting -> vehicle loads -> arrivals`

All passenger counts remain source-of-truth simulation outputs from `SimulationOutput`.

## New Geography Model

Implemented in [`crates/interlinked-engine/src/sim/types.rs`](../crates/interlinked-engine/src/sim/types.rs) and consumed in [`crates/interlinked-engine/src/sim/planner.rs`](../crates/interlinked-engine/src/sim/planner.rs).

### Added enums

- `SettlementClass`
  - `GlobalCityCore`, `MajorCity`, `RegionalCity`, `LargeTown`, `SmallTown`, `Village`, `Rural`, `SpecialNode`
- `ZoneArchetype`
  - `Cbd`, `InnerResidential`, `OuterSuburb`, `TownCentre`, `IndustrialEstate`, `BusinessPark`, `UniversityDistrict`, `RetailLeisureDistrict`, `AirportZone`, `PortLogisticsZone`, `VillageCentre`, `RuralResidential`, `RuralAgricultural`
- `SpecialAttractorType`
  - `Airport`, `Port`, `University`, `Stadium`, `Hospital`, `TourismLandmark`, `GovernmentCentre`, `LogisticsHub`

### New synthetic config

- `SyntheticEconomyConfig`
  - archetype trait weights (`archetype_traits`)
  - settlement multipliers by purpose (`settlement_class_multipliers`)
  - trip rates by purpose (`purpose_trip_rates`)
  - generalized-cost and distance decay by purpose (`purpose_gc_decay_beta`, `purpose_distance_decay_beta`)
  - centrality/regional weighting (`centrality_weight`, `regional_importance_weight`)
  - corridor bonuses (`corridor_bonus_*`)
  - rural demand floors (`rural_baseline_trip_floor_per_person`, `rural_essential_demand_floor_per_person`)
  - special attractor multipliers (`attractor_strength_multipliers`)
  - time-slice purpose multipliers (`time_slice_purpose_multipliers`)

### Extended zone profile

- `ZoneDemandProfile` now includes:
  - geography fields (`archetype`, `settlement_class`, densities/intensities, centrality, regional importance)
  - behavior fields (`car_dependency`, `transit_affinity`, nearest service centre)
  - anchor fields (`special_attractors`)
  - derived attraction scores (`work_attractiveness`, `education_attractiveness`, `shopping_attractiveness`, `leisure_attractiveness`, `essential_service_attractiveness`, `intercity_importance`)

## Where Latent Demand Is Generated

- Function: `build_latent_demand_foundation(...)` in `planner.rs`
- Inputs:
  - zone profiles from `build_zone_demand_profiles(...)`
  - purpose shares and time-slice multipliers
  - purpose-specific attraction and impedance
  - corridor bonuses and rural nearest-service-centre bias
- Output:
  - authoritative `latent_od_demand` (all slices)
  - active-slice latent OD used for assignment

## Purpose-Specific Behaviour

Implemented via:

- `origin_production_factor(...)`
- `purpose_attraction(...)`
- `purpose_impedance(...)`
- `time_slice_multiplier(...)`
- `corridor_bonus(...)`
- `rural_service_center_bias(...)`

Effects:

- Work: residential-origin commuter pull to strong job/central zones.
- Education: directional pull to education-rich/university zones.
- Shopping/Leisure: different time-of-day and trip-length tolerance.
- Essential: persistent baseline with rural nearest-service-centre preference.
- Intercity: hierarchy and anchor-sensitive long-range demand.

## Phase 1 Accounting Integrity

Phase 2 only changes latent generation shape. Assignment and flow accounting remain Phase 1 authoritative logic in `run_simulation_with_settings(...)`:

- no UI-only counters
- no disconnected passenger model
- conservation checks continue in `DemandDiagnostics.consistency_checks`

## Layer-Ready Outputs (Authoritative)

`SimulationOutput` now carries:

- `zone_economic_geography_layer`
- `zone_demand_production_layer`
- `zone_demand_attraction_layer`
- `corridor_desire_lines`
- `service_gap_layer`

These are directly derived from latent/assigned/stateful flow outputs and are suitable for future map layers.

## Diagnostic Outputs

`DemandDiagnostics` includes Phase 2 inspectability fields:

- `top_centrality_zones`
- `top_work_attractors`
- `top_intercity_pairs`
- `strongest_commuter_corridors`
- `strongest_rural_to_town_flows`
- `strongest_anchor_flows`

## Phase 2 Demo/Verification Assets

- Fixture:
  - `crates/interlinked-engine/tests/fixtures/scenario_demand_synthetic_geography_phase2.json`
- Tests:
  - `crates/interlinked-engine/tests/demand_synthetic_geography_phase2.rs`

These validate hierarchy effects, rural baseline demand, special attractor impacts, corridor outputs, and flow conservation continuity.

## Current Architectural Compromise

`SyntheticEconomyConfig` is currently defaulted in the planner and emitted in outputs for inspectability. It is not yet scenario-authored input. This keeps the backbone deterministic and testable now, while leaving room for future scenario/editor-level tuning.
