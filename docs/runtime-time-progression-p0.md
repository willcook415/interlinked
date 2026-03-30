# Runtime Time Progression P0 Fix

## Root Cause

The runtime worker loop previously had two coupled defects:

1. It limited catch-up work to `max_steps = speed` (`1/2/4`) per loop cycle.
2. When still behind, it dropped accumulated simulation debt:
   - `dropped_steps = floor(accumulator / fixed_step)`
   - `accumulator = fixed_step * 0.5`

Under load, this guaranteed time under-run and made the in-game clock slower than wall clock, even at `1x`.

## New Runtime Stepping Model

The loop now uses truthful fixed-step accumulation:

1. `accumulator += real_elapsed * speed_multiplier`
2. Compute available fixed steps from accumulator.
3. Execute up to `max_steps_per_cycle` (configurable, bounded).
4. Subtract only executed fixed-step time from accumulator.
5. Keep remaining backlog in accumulator (no debt discard).

This preserves clock truth and prevents hidden desync between displayed time and authoritative state.

## Runtime Telemetry

`RuntimePerfTelemetry` now includes runtime-speed and backlog diagnostics:

- `fixed_step_s`
- `executed_steps_this_cycle`
- `max_steps_per_cycle`
- `backlog_steps`
- `backlog_s`
- `accumulator_s`
- `cycle_elapsed_ms`
- `avg_cycle_elapsed_ms`
- `avg_sim_step_ms`
- `real_elapsed_s`
- `game_elapsed_s`
- `target_game_elapsed_s`
- `target_speed_ratio`
- `achieved_speed_ratio`
- `achieved_vs_target_ratio`
- `under_sustained_speed`

These values are available through the authoritative runtime snapshot path.

## Runtime Scheduling Config Additions

`RuntimeSchedulingManifest` now includes:

- `max_steps_per_cycle` (bounded catch-up budget)
- `lightweight_tick_outputs` (enable reduced per-tick strategic bundle generation)

Also, `fixed_step_s` clamping was relaxed to `0.05..=1.0` to permit finer ticks where needed.

## Lightweight Tick Outputs

Stateful game ticking now supports a lightweight simulation-output mode for runtime stepping:

- Keeps authoritative core outputs required for truth:
  - assigned OD
  - board loads
  - passenger cohorts
  - stop flow states
  - vehicle load states
  - fares/KPIs/accounting
- Skips heavyweight planning/economics/temporal bundles on every tiny tick.
- Tags output version as `0.2.2-lite`.

Inspector/on-demand paths force full output generation when needed.

## Cadence Intent

- Every authoritative tick:
  - passenger/service state advancement
  - queues/boarding/alighting/load accounting
  - fare/economy tick accounting
- Lower-frequency / on-demand:
  - heavyweight strategic overlays and ranking bundles
  - full planning/economics diagnostics refreshes

This keeps the live clock truthful while reducing avoidable per-tick work.
