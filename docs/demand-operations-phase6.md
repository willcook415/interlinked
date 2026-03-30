# Demand Operations Realism (Phase 6)

Phase 6 adds authoritative operations realism on top of the Phase 1-5 backbone.

Authoritative chain now:

`latent OD -> mode choice -> transit assignment -> waiting/boarding/alighting/load -> dwell/runtime pressure -> delay/reliability state -> transfer reliability -> planning and mode-capture interpretation`

No cosmetic delay labels are used: all Phase 6 outputs are derived from simulated stop/vehicle/boarding state.

## New core structs

Implemented in [`crates/interlinked-engine/src/sim/types.rs`](../crates/interlinked-engine/src/sim/types.rs):

- `OnTimeStatus`
  - `on_time`, `slightly_late`, `late`, `severe_delay`
- `OperationalIncidentType`
  - `none`, `minor_delay`, `major_delay`, `congestion`, `dwell_overrun`, `vehicle_short_turn`, `service_gap`, `transfer_failure`
- `OperationsReliabilityConfig`
  - base dwell, boarding/alighting coefficients, crowding dwell multiplier, delay propagation/recovery controls, headway irregularity sensitivity, transfer-window impacts, reliability penalty coefficients
- `ServiceOperationState`
  - service/run operational state including delay, dwell/runtime, headway irregularity, bunching gaps, reliability score, bottleneck stop, incident type
- `StopOperationState`
  - realised calls/headway, dwell, platform pressure, denied pressure, transfer success, operational pressure
- `TransferOperationMetrics`
  - interchange pair transfer windows, missed transfer rates/counts, delay-caused failures, interchange pressure
- `ServiceReliabilityDiagnostics`
  - ranked delay/dwell/bunching/transfer bottlenecks and reliability-linked mode-choice penalties

## Planner integration

Implemented in [`crates/interlinked-engine/src/sim/planner.rs`](../crates/interlinked-engine/src/sim/planner.rs):

1. Added `build_operations_outputs(...)` after authoritative assignment and stop/vehicle state assembly.
2. Dwell is computed from:
   - base dwell + boarding/alighting throughput load
   - crowding multiplier
   - stop/interchange class multiplier
3. Delay propagates forward by service through cumulative dwell/runtime overrun with recovery margins.
4. Realised headway and bunching/irregularity are derived from delivered calls and delay pressure.
5. Transfer reliability is computed at interchange stop + service pair level using:
   - scheduled transfer windows
   - realised windows under delay/crowding/irregularity
   - missed transfer counts/rates
6. Phase 5 mode-choice reliability cost now includes operational pressure terms from authoritative network pressure proxies.

## New authoritative output fields

`SimulationOutput` now includes:

- `service_operation_states`
- `stop_operation_states`
- `transfer_operation_metrics`
- `service_reliability_diagnostics`

Existing planning outputs are now operationally aware:

- `zone_planning_metrics[*]`
  - `reliability_penalty_s`, `operational_underservice_score`
- `station_planning_metrics[*]`
  - dwell, realised headway/irregularity, transfer success, platform pressure, operational pressure
- `corridor_planning_metrics[*]`
  - `reliability_adjusted_service_quality`, bottleneck/transfer/crowding-delay pressure
- `line_service_planning_metrics[*]`
  - scheduled vs realised headway, delay, dwell, bunching risk, reliability score, transfer success, operational pressure

`ServiceGapRankings` now also includes:

- `top_unreliable_services`
- `top_dwell_pressure_stations`
- `top_bunching_prone_lines`
- `top_missed_transfer_interchanges`
- `top_corridors_losing_capture_due_to_unreliability`
- `top_operational_bottlenecks`

## Inspector guidance for UI

- Service operations inspector:
  - read `service_operation_states` and matching `line_service_planning_metrics`
- Station/platform operations inspector:
  - read `stop_operation_states` and `station_planning_metrics`
- Transfer inspector:
  - read `transfer_operation_metrics` at interchange nodes
- Corridor reliability inspector:
  - read `corridor_planning_metrics` reliability-adjusted fields
- Regional reliability dashboard:
  - read `service_reliability_diagnostics`

## Tunability

Tune with `SyntheticEconomyConfig.operations_reliability_config`:

- dwell base and passenger coefficients
- crowding dwell inflation
- delay propagation/recovery
- irregularity and bunching thresholds
- transfer-window sensitivity to delay/crowding
- reliability penalties applied to transit generalized cost

## Fixture and verification

Added:

- Fixture: [`crates/interlinked-engine/tests/fixtures/scenario_demand_operations_phase6.json`](../crates/interlinked-engine/tests/fixtures/scenario_demand_operations_phase6.json)
- Tests: [`crates/interlinked-engine/tests/demand_operations_phase6.rs`](../crates/interlinked-engine/tests/demand_operations_phase6.rs)

Coverage includes:

- dwell inflation under high boarding/alighting pressure
- delay propagation and reliability degradation
- transfer reliability degradation under delay pressure
- operational-penalty presence in transit generalized costs
- preservation of authoritative accounting conservation

## What is heuristic vs authoritative

Fully authoritative:

- stop/vehicle flows, queues, denied boardings, board/alight/load accounting
- derived dwell/delay/headway/transfer reliability outputs from those states
- operational planning rankings derived from those outputs

Heuristic but inspectable (first pass):

- simplified delay propagation and bunching formulation
- transfer-volume proxy for missed-transfer magnitude
- reliability-to-mode-choice penalty mapping coefficients

Known architecture constraints:

- no full timetable/dispatch simulation with explicit per-trip timetable objects yet
- no road traffic microsim coupling for bus runtime congestion
- no second-by-second signalling/vehicle blocking model
