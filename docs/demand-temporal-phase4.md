# Demand Temporal Realism (Phase 4)

Phase 4 extends the authoritative demand backbone with explicit temporal context, day/season/event modifiers, and time-aware service-pressure/planning outputs.

The authoritative chain is unchanged:

`zone demand -> latent OD (context-tagged) -> assignment -> waiting/denied -> boarding/alighting -> onboard load`

All new temporal outputs are derived from that chain; no UI-only synthetic counters are introduced.

## What Was Added

### Temporal context primitives

Implemented in [`crates/interlinked-engine/src/sim/types.rs`](../crates/interlinked-engine/src/sim/types.rs):

- `ServiceDayType`
  - `Weekday`
  - `Saturday`
  - `SundayHoliday`
- `SeasonalProfile`
  - `Neutral`
  - `SummerPeak`
  - `WinterPeak`
  - `TermTime`
  - `HolidayPeriod`
- `TemporalDemandSlice`
  - `service_day_type`
  - `time_slice`
  - `seasonal_profile`
  - `active_event_ids`

### Event and attractor modifiers

`SyntheticEconomyConfig` already carries:

- `event_demand_modifiers: Vec<EventDemandModifier>`
- `event_modifier_strength_scale`
- attractor multipliers (`attractor_strength_multipliers`)

Event modifiers now actively affect latent OD generation in [`crates/interlinked-engine/src/sim/planner.rs`](../crates/interlinked-engine/src/sim/planner.rs):

- context filter by day/time/season
- optional attractor grounding (`airport`, `university`, `stadium`, `tourism`, etc.)
- purpose-specific multipliers
- per-OD attribution via `active_event_ids` on both `LatentOdDemand` and `AssignedOdFlow`

### Canonical six-slice day profile

`DemandTimeSliceLabel` now has six active slices in generation and diagnostics:

- `EarlyMorning`
- `AmPeak`
- `Interpeak`
- `PmPeak`
- `Evening`
- `LateNight`

## Authoritative Temporal Outputs

`SimulationOutput` now exposes:

- `active_temporal_slice`
- `temporal_planning_snapshots`
- `temporal_demand_diagnostics`

### `temporal_planning_snapshots`

Each snapshot is keyed by a `TemporalDemandSlice` and contains time-aware variants of:

- `zone_planning_metrics`
- `station_planning_metrics`
- `corridor_planning_metrics`
- `line_service_planning_metrics`
- `service_gap_rankings`

The aggregate Phase 1-3 fields remain present for current active context.

### `temporal_demand_diagnostics`

Developer-facing temporal diagnostics include:

- `purpose_totals` (latent/realised/unserved by purpose + context)
- `station_pressure` (waiting/denied/pressure by context)
- `service_pressure` (utilisation/overload by context)
- `corridor_pressure` (latent/realised/unserved by context)
- `service_gap_summaries` (contextual accessibility and coverage)
- rankings:
  - `top_overloaded_stations_by_slice`
  - `top_overloaded_services_by_slice`
  - `strongest_corridors_by_slice`
  - `peak_waiting_by_station_by_slice`
  - `peak_denied_by_station_by_slice`
  - `peak_corridor_unserved_by_slice`
  - `peak_line_overload_by_slice`
  - `latent_to_realised_ratio_by_slice`
  - `overload_flip_classifications` ("only overloaded in ...")

## Temporal Generation Behavior

Latent OD generation now applies purpose multipliers across:

- time slice
- service day type
- seasonal profile
- event modifiers (context + attractor grounded)

This drives:

- commuter AM/PM pressure on weekdays
- weaker weekend work/education
- Saturday shopping/leisure uplift
- term-time education uplift around university anchors
- holiday airport/tourism/intercity uplift
- evening stadium/nightlife spikes where attractors exist

## Planning API usage

Planning requests can set explicit temporal context via `PlanningRunOptions` in [`crates/interlinked-engine/src/platform/simulation.rs`](../crates/interlinked-engine/src/platform/simulation.rs):

- `time_of_day_s`
- `service_day_type`
- `seasonal_profile`
- `active_event_ids`

If omitted, defaults are `Weekday + Interpeak + Neutral`.

## New Phase 4 fixture and tests

- Fixture:
  - [`crates/interlinked-engine/tests/fixtures/scenario_demand_temporal_phase4.json`](../crates/interlinked-engine/tests/fixtures/scenario_demand_temporal_phase4.json)
- Tests:
  - [`crates/interlinked-engine/tests/demand_temporal_realism_phase4.rs`](../crates/interlinked-engine/tests/demand_temporal_realism_phase4.rs)

They validate:

- weekday vs weekend work differences
- Saturday shopping/leisure uplift
- term-time education uplift
- holiday airport/tourism shifts
- stadium event spikes with bounded magnitude
- temporal snapshot availability
- temporal pressure/ranking coherence
- active-context conservation and consistency checks

## Inspector guidance (for future UI)

Use `temporal_planning_snapshots` as the primary query surface for time/day/season overlays:

- Zone inspector:
  - find snapshot by context
  - read `zone_planning_metrics`
- Station inspector:
  - read snapshot `station_planning_metrics`
  - cross-check `temporal_demand_diagnostics.station_pressure`
- Corridor inspector:
  - read snapshot `corridor_planning_metrics`
  - cross-check `temporal_demand_diagnostics.corridor_pressure`
- Service/line inspector:
  - read snapshot `line_service_planning_metrics`
  - cross-check `temporal_demand_diagnostics.service_pressure`

## Current limits

Fully authoritative:

- latent OD context tagging
- assignment/waiting/denied/load accounting per selected context
- temporal snapshot and pressure metrics derived from authoritative outputs

Heuristic (explicitly still heuristic):

- synthetic economic geography and attractor inference
- event intensity defaults and context catalog selection
- build preview intervention scoring (still planning heuristic, not post-build simulation)

Out of scope for this phase:

- timetable optimization
- interactive calendar/event UI
- land-use growth feedback loops
