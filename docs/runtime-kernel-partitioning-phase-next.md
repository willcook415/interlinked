# Runtime Kernel Partitioning (Fast vs Strategic)

This refactor splits authoritative game stepping into two kernels:

## Fast Operational Kernel

Runs on most live ticks via `run_fast_operational_step` and updates only runtime-critical state:

- queue/cohort arrivals at service-stop edges
- departure phase progression (`time_to_next_departure_s`)
- boarding under vehicle + station throughput caps
- queue overflow clipping and denied demand
- live `board_loads`, `stop_flows`, `passenger_cohorts`, service load summaries
- authoritative `SimState` queue + clock advancement

This path avoids full planner recomputation and reuses strategic templates.

## Slow Strategic Kernel

Runs via `step_simulation` only when refresh is required. It recomputes heavyweight world state:

- latent demand / temporal demand context
- mode choice and assignment
- planning, diagnostics, economics, and strategic overlays

After refresh, output is cached into `StrategicKernelCache` and reused by fast ticks.

## Refresh Triggers

Strategic refresh runs on:

- missing cache (first step / cold start)
- strategic cadence interval (`RunConfig.strategic_refresh_interval_steps`)
- scope signature change (`SimulationScope`)
- topology signature change (scenario world hash)
- explicit invalidation from edits
- explicit force refresh (`GameStepRequest.force_strategic_refresh`)

## Cached Strategic State

`StrategicKernelCache` stores:

- topology + scope signatures
- precomputed operational board templates
- cohort arrival templates by service-stop
- a stripped `SimulationOutput` skeleton reused for fast-step outputs

## Instrumentation

Engine metrics are tracked in `KernelPerfMetrics` and surfaced in runtime telemetry:

- fast vs strategic step counts
- fast vs strategic last/average step ms
- cache hits/misses
- steps since last strategic refresh
- last strategic refresh reason

Desktop runtime telemetry fields:

- `engine_fast_steps`, `engine_strategic_steps`
- `engine_fast_last_ms`, `engine_strategic_last_ms`
- `engine_fast_avg_ms`, `engine_strategic_avg_ms`
- `engine_strategic_cache_hits`, `engine_strategic_cache_misses`
- `engine_strategic_refresh_executed`, `engine_strategic_refresh_reason`

## Authority Model

- Fast kernel remains authoritative for passenger queues, boarding/alighting/load movement state.
- Strategic kernel remains authoritative for demand/mode/assignment/planning/economics baselines.
- No UI-only fake clock/state path is introduced.
