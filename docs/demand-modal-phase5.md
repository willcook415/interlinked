# Demand Modal Competition (Phase 5)

Phase 5 adds authoritative modal competition and transit capture on top of the existing Phase 1-4 backbone.

The truth chain is now:

`zone demand -> latent OD -> mode choice (transit vs car vs walk vs no-trip) -> transit-captured OD -> assignment -> waiting/denied -> boarding/alighting -> vehicle loads`

No non-transit passengers are injected into station/train/service load states.

## Core structs and enums

Implemented in [`crates/interlinked-engine/src/sim/types.rs`](../crates/interlinked-engine/src/sim/types.rs):

- `TravelMode`
  - `walk`, `car`, `bus`, `metro_tram`, `suburban_rail`, `regional_rail`, `high_speed_rail`, `other_transit`, `no_trip`
- `ModeChoiceContext`
  - purpose, OD zones, day/time/season, event ids, settlement/archetype context, distance
- `ModeGeneralizedCostBreakdown`
  - access/wait/in-vehicle/transfer/fare/parking/crowding/reliability/egress/total generalized cost
- `ModeChoiceResult`
  - latent passengers, chosen mode shares, mode costs, transit capture, car/walk capture, suppressed/no-trip, winning mode, transit submode split
- `ModalDemandDiagnostics`
  - mode shares by purpose/zone/corridor/time/day and ranked modal opportunity/risk lists

## Tunability

`SyntheticEconomyConfig` now includes developer-facing modal tuning surfaces:

- `mode_utility_coefficients`
  - utility scale, transfer aversion, fare sensitivity, crowding/reliability weights, walk thresholds, car speed/parking/toll assumptions
- `purpose_mode_sensitivities`
  - value-of-time, cost sensitivity, mode constants by purpose
- `settlement_mode_constants`
  - mode constants by settlement class
- `archetype_parking_penalties`
  - destination parking penalties by archetype
- `transit_submode_preferences`
  - within-transit preference split by purpose

These are emitted in output (`synthetic_economy_config`) for inspectability.

## Planner integration

In [`crates/interlinked-engine/src/sim/planner.rs`](../crates/interlinked-engine/src/sim/planner.rs):

- Added `apply_mode_choice_capture(...)` between latent generation and assignment.
- Each active latent OD row evaluates transit/car/walk/no-trip utilities from generalized costs.
- Only `transit_captured_passengers` enter assignment and downstream stop/vehicle accounting.
- Non-transit demand remains represented in authoritative modal outputs and planning metrics.

## New authoritative outputs

`SimulationOutput` now includes:

- `mode_choice_results`
- `zone_mode_share_metrics`
- `corridor_mode_share_metrics`
- `station_transit_capture_context`
- `service_transit_capture_context`
- `citywide_mode_share_summary`
- `modal_demand_diagnostics`

Existing planning fields (`zone/station/corridor/line/service_gap`) are now modal-aware via additional capture/share fields, not replaced.

## Inspector guidance

For UI/overlay queries:

- Zone modal inspector:
  - `zone_mode_share_metrics[*]`
  - `zone_planning_metrics[*].transit_capture_share`, `car_capture_share`, `walk_capture_share`
- Corridor modal inspector:
  - `corridor_mode_share_metrics[*]`
  - `corridor_planning_metrics[*].transit_share`, `car_share`, `strongest_transit_submode`, `transit_capture_gap`
- Station/service capture inspector:
  - `station_transit_capture_context[*]`
  - `service_transit_capture_context[*]`
- Citywide dashboard:
  - `citywide_mode_share_summary`
  - `modal_demand_diagnostics.mode_share_by_time_slice`
  - `modal_demand_diagnostics.mode_share_by_day_type`

## Modal-aware planning/service-gap extensions

`ServiceGapRankings` now additionally exposes:

- `top_transit_capture_opportunity_corridors`
- `top_car_dominated_transit_viable_corridors`
- `top_overcrowded_corridors_losing_mode_share`
- `top_socially_important_low_demand_services`

`PlanningDebugSummary` now additionally exposes:

- top zones by transit share
- top car-dominated corridors
- strongest commuter transit corridors
- strongest intercity rail corridors
- zones losing transit due to transfers/crowding
- zones where parking penalties support transit

## Fixture and verification

Added:

- Fixture: [`crates/interlinked-engine/tests/fixtures/scenario_demand_modal_phase5.json`](../crates/interlinked-engine/tests/fixtures/scenario_demand_modal_phase5.json)
- Tests: [`crates/interlinked-engine/tests/demand_modal_competition_phase5.rs`](../crates/interlinked-engine/tests/demand_modal_competition_phase5.rs)

Coverage includes:

- urban vs rural transit/car split behavior
- intercity rail competitiveness
- short-trip walk competitiveness
- overcrowding-linked capture pressure visibility
- authoritative accounting conservation after mode choice insertion
- guarantee that assignment/load states are bounded by transit-captured demand

## What is heuristic vs authoritative

Fully authoritative:

- transit assignment, waiting, denied boardings, board/alight counts, vehicle loads
- transit-capture gating before assignment
- all planning/load outputs derived from simulated state

Heuristic (explicit, tunable):

- synthetic car generalized cost model (speed/parking/toll proxies)
- walk competitiveness threshold model
- mode utility constants/sensitivity defaults
- derived attribution of "lost due to crowding/fare/indirectness/reliability"

Current architecture limits:

- no explicit road network assignment or congestion propagation
- no full timetable/reliability stochastic model yet
- transit choice currently uses generalized-cost proxies before full assignment equilibrium feedback
