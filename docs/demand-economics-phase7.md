# Demand Economics (Phase 7)

Phase 7 adds authoritative transport economics on top of the Phase 1-6 backbone.

Truth chain:

`latent OD -> mode choice -> transit-captured OD -> assignment -> waiting/boarding/alighting/load -> operations/reliability -> realised trips + delivered service supply -> revenue/cost/subsidy outcomes`

No decorative finance counters are used. Revenue and cost are derived from simulation state.

## Core structs and enums

Implemented in [`crates/interlinked-engine/src/sim/types.rs`](../crates/interlinked-engine/src/sim/types.rs):

- `FareModel`
  - `flat_fare`, `distance_based`, `zone_based`, `mode_based`, `transfer_discount`
- `FareModelConfig`
  - fare model selection, fare coefficients, transfer discount settings, mode supplements
- `ServiceCostProfile`
  - fixed + vehicle-hour + vehicle-km + crew/energy/maintenance + stop-call + reliability uplift
- `InfrastructureCostProfile`
  - track/station/stop capex, complexity multiplier, annualized maintenance/renewal
- `RollingStockCostProfile`
  - annualized capital + lease + maintenance + capacity/efficiency references
- `FinancialPerformanceMetrics`
  - revenue, operating cost, full cost, subsidy, farebox recovery, per-passenger metrics
- `NetworkFinancialSummary`, `ServiceFinancialMetrics`, `CorridorFinancialMetrics`, `StationFinancialContext`
- `CommercialStrengthClassification`
  - `strong`, `viable`, `marginal`, `weak`
- `SocialNecessityClassification`
  - `core`, `important`, `supportive`, `low`
- `EconomicDiagnostics`
  - ranked profitability/subsidy/reinvestment/opportunity lists + temporal finance summaries

## Revenue attribution

Implemented in [`crates/interlinked-engine/src/sim/planner.rs`](../crates/interlinked-engine/src/sim/planner.rs) via:

- `fare_for_assigned_path(...)`
- `build_economics_outputs(...)`

Revenue rules:

- Revenue is computed from `AssignedOdFlow.chosen_paths` passenger volumes.
- Fare depends on configured fare model + path distance + transfer count + mode supplements.
- Non-transit demand does not enter this revenue pipeline.
- Service-level fare revenue aggregates exactly into network-level fare revenue.

## Operating, capital, and rolling-stock cost logic

Implemented in `build_economics_outputs(...)`:

- Operating cost per service is derived from:
  - departures (headway/TPH)
  - route runtime and distance
  - service cost profile coefficients
  - peak uplift for AM/PM slices
  - reliability uplift under lower reliability
- Infrastructure annualized cost is derived from:
  - built links (km, mode profile, maintenance/renewal)
  - built stops/stations (stop/station capex profile)
- Rolling stock cost is derived from:
  - inferred/assigned vehicles required
  - annualized capital + lease + maintenance assumptions

These costs are then allocated to service/corridor/station/network outputs through inspectable shares.

## Planning integration

`apply_economics_to_planning(...)` applies authoritative finance outputs into Phase 3 planning rows:

- `zone_planning_metrics`
  - `transit_revenue_generated`, `subsidy_need_proxy`
- `station_planning_metrics`
  - `associated_revenue`, cost burden proxies, strategic value, commercial/social classes
- `corridor_planning_metrics`
  - fare/cost/subsidy/farebox + commercial/social classes
- `line_service_planning_metrics`
  - full service financial metrics + `reliability_cost_pressure`
- `build_preview_metrics`
  - revenue uplift, operating/capital uplift, farebox, subsidy, classifications, reinvestment score
- `service_gap_rankings` / `planning_debug_summary`
  - top profitable, loss-making high-ridership, subsidy-dependent social, expensive-underperforming, reinvestment and unreliability-finance lists

## New authoritative outputs

`SimulationOutput` now includes:

- `network_financial_summary`
- `service_financial_metrics`
- `corridor_financial_metrics`
- `station_financial_context`
- `economic_diagnostics`

`TemporalPlanningSnapshot` now includes:

- `network_financial_summary`

`EconomicDiagnostics` includes:

- ranking outputs for profitability/subsidy/reinvestment
- `network_financial_by_time_slice`
- `network_financial_by_day_type`

## Inspector guidance for future UI

- Network finance dashboard:
  - `network_financial_summary`
  - `economic_diagnostics.network_financial_by_time_slice`
  - `economic_diagnostics.network_financial_by_day_type`
- Service finance inspector:
  - `service_financial_metrics[*]`
  - paired with `line_service_planning_metrics[*]` for operations + finance view
- Corridor finance inspector:
  - `corridor_financial_metrics[*]`
  - paired with `corridor_planning_metrics[*]` for capture/reliability/finance context
- Station finance inspector:
  - `station_financial_context[*]`
  - paired with `station_planning_metrics[*]`
- Build-preview economics:
  - `build_preview_metrics[*].estimated_revenue_uplift`
  - `estimated_operating_cost_uplift`
  - `estimated_capital_cost`
  - `estimated_farebox_recovery`
  - `likely_subsidy_requirement`
  - `commercial_strength_classification` / `social_necessity_classification`
  - `reinvestment_case_score`

## Tunability

Phase 7 tuning sits under `SyntheticEconomyConfig`:

- `fare_model_config`
- `service_cost_profiles`
- `infrastructure_cost_profiles`
- `rolling_stock_cost_profiles`
- `economics_policy_config`

## Fixture and verification

Added:

- Fixture: [`crates/interlinked-engine/tests/fixtures/scenario_demand_economics_phase7.json`](../crates/interlinked-engine/tests/fixtures/scenario_demand_economics_phase7.json)
- Tests: [`crates/interlinked-engine/tests/demand_economics_phase7.rs`](../crates/interlinked-engine/tests/demand_economics_phase7.rs)

Coverage includes:

- revenue grounded in realised transit trips and conservation checks
- strong trunk vs weak rural financial differentiation
- service-supply-linked operating cost scaling
- non-empty infrastructure/capital accounting
- meaningful commercial/social classification separation
- populated build-preview economics
- temporal financial summaries and reliability-finance linkage

## Heuristic vs authoritative

Fully authoritative:

- transit ridership, stop/vehicle/assignment accounting inputs to finance
- path-grounded fare revenue attribution
- service supply-grounded operating cost base
- line/station/corridor/network finance outputs derived from simulation outputs

Heuristic but inspectable:

- cost proxy coefficients and annualization assumptions
- shared infrastructure allocation weighting rules
- social value proxy and classification thresholds
- build-preview economic uplift estimates

Current architecture limits:

- no full accounting ledger (tax, debt, cashflow lifecycle)
- no detailed infrastructure asset register by construction package/phasing
- no explicit per-passenger fare product/passes model yet
- corridor-level cost allocation remains aggregate-share based, not full path-level cost tracing
