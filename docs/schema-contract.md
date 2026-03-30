# Interlinked Scenario Schema Contract

## Canonical Document Shape

Persisted scenarios use the wrapped `ScenarioDocument` form:

```json
{
  "schema_version": 1,
  "scenario": {
    "meta": { "...": "..." },
    "params": { "...": "..." },
    "world": { "...": "..." }
  }
}
```

`schema_version` is the document contract version. `ScenarioService::save_to_path` always writes this wrapped format.

## Backward Compatibility

- The loader still accepts legacy flat scenario payloads with top-level `meta` / `params` / `world`.
- Legacy payloads are migrated in memory to the wrapped current format before validation.
- Builder metadata added for the line/station authoring pass is additive and optional, so older scenarios continue to load unchanged.

## Current Network Model

### `world.stops[]`

Required fields:

- `id: string`
- `x: number`
- `y: number`

Optional fields:

- `name?: string | null`
- `country_iso2?: string | null`
- `interchange_id?: string | null`
- `stop_type?: string | null`

### `world.links[]`

Required fields:

- `id: string`
- `from_stop: string`
- `to_stop: string`
- `distance_m: number`
- `mode: string`
- `speed_mps: number`

Optional fields:

- `geometry?: [number, number][] | null`
- `line_id?: string | null`
- `mode_variant?: string | null`
- `capacity_per_hour?: number | null`

### `world.services[]`

Required fields:

- `id: string`
- `mode: string`
- `stop_sequence: string[]`
- `headway_s: number`
- `dwell_s: number`
- `vehicle_capacity: number`

Optional fields:

- `line_id?: string | null`
- `name?: string | null`
- `mode_variant?: string | null`
- `direction?: string | null`
- `direction_name?: string | null`
- `display_color?: string | null`
- `board_penalty_s?: number | null`

### `world.transfers[]`

- `from_stop: string`
- `to_stop: string`
- `time_s: number`
- `penalty_s?: number | null`
- `allowed_modes?: string[] | null`

## Builder Contract

The first-pass builder assumes:

- lines are grouped by `service.line_id` when present, otherwise by `service.id`
- named stations are stored on `stop.name`
- line presentation lives on services, especially `name`, `display_color`, `direction`, and `direction_name`
- `mode_variant` distinguishes presets that share an engine mode, such as `commuter_rail` and `high_speed_rail` under `rail`
- authored link geometry stays on `link.geometry`; builder-created corridors do not create shape stops

## Canonical Mode Contract

Mode handling now follows one shared contract across simulation, planning, economics, and UI adapters:

- `mode` is the authored base mode token (for example `bus`, `metro`, `tram`, `rail`, `ferry`)
- `mode_variant` is optional subtype metadata (for example `commuter_rail`, `regional_rail`, `high_speed_rail`)
- engine canonicalization resolves this pair into a canonical transit identity:
  - `bus`
  - `tram`
  - `metro`
  - `suburban_rail`
  - `regional_rail`
  - `high_speed_rail`
  - `ferry`
  - `other_transit`
- simulation family (`TravelMode`) is derived from canonical identity:
  - `bus -> Bus`
  - `tram/metro -> MetroTram`
  - `suburban_rail -> SuburbanRail`
  - `regional_rail -> RegionalRail`
  - `high_speed_rail -> HighSpeedRail`
  - `ferry/other_transit -> OtherTransit`
- economics mode lookup is canonical-first with compatibility fallback:
  - rail variants first try variant-specific keys then fallback to `rail`
- UI display class (`bus`, `tram`, `metro`, `commuter_rail`, `rail`, `high_speed_rail`, `ferry`) is derived from canonical identity, not ad hoc string checks.

## Desktop Build APIs

The desktop shell now exposes these Tauri commands for authoring and inspection:

- `load_build_defaults`
- `apply_network_mutation`
- `inspect_station`
- `inspect_line`

`apply_network_mutation` accepts a full `ScenarioDocumentLite` draft, validates it, persists it, and in game sessions charges only positive capex delta. Deletions currently give no refund.

Planning command contract:

- project `run_planning` and file-based `run_planning_scenario` both execute through `SimulationService::run_planning`.
- planning temporal bundle behavior is explicit (always included for planning) and no longer depends on whether a state object is present.

## Save and Migration Rules

- Save paths always persist the current wrapped document form.
- Schema changes for the builder pass are intentionally additive.
- Existing imported or older scenarios without names, line ids, colors, or direction metadata remain valid; the UI falls back to ids when display metadata is absent.
