import type { ScenarioSidebarController } from "../app/useScenarioSidebarController";
import type { RunMeta } from "../types";

export default function ScenarioControlsSidebar(props: {
  open: boolean;
  busy: boolean;
  runs: RunMeta[];
  controller: ScenarioSidebarController;
  onRunPlanning: () => Promise<void>;
  onRebuildDemand: () => Promise<void>;
  onExportRunCsv: (runId: string) => Promise<void>;
  onExportRunJson: (runId: string) => Promise<void>;
  onCompareRuns: () => Promise<void>;
}) {
  if (!props.open) return null;

  return (
    <aside className="scenario-panel">
      <h4>Scenario Controls</h4>
      <label>Seed</label>
      <input
        value={props.controller.runConfig.deterministic_seed ?? ""}
        onChange={(event) => props.controller.onSeedChanged(event.target.value)}
      />
      <label>Horizon (s)</label>
      <input
        value={props.controller.runConfig.horizon_s ?? ""}
        onChange={(event) => props.controller.onHorizonChanged(event.target.value)}
      />
      <label>Time Bin (s)</label>
      <input
        value={props.controller.runConfig.time_bin_s ?? ""}
        onChange={(event) => props.controller.onTimeBinChanged(event.target.value)}
      />
      <div className="action-row">
        <button onClick={props.onRunPlanning} disabled={props.busy}>
          Run Planning
        </button>
        <button onClick={props.onRebuildDemand} disabled={props.busy}>
          Rebuild Demand
        </button>
      </div>
      <div className="scenario-runs">
        {props.runs.map((run) => (
          <div key={run.run_id} className="run-chip">
            <span>{run.run_id}</span>
            <button onClick={() => props.onExportRunCsv(run.run_id)}>CSV</button>
            <button onClick={() => props.onExportRunJson(run.run_id)}>JSON</button>
          </div>
        ))}
      </div>
      <label>Baseline</label>
      <select
        value={props.controller.selectedBaseRun}
        onChange={(event) => props.controller.onSelectedBaseRunChanged(event.target.value)}
      >
        <option value="">Select run</option>
        {props.runs.map((run) => (
          <option key={run.run_id} value={run.run_id}>
            {run.run_id}
          </option>
        ))}
      </select>
      <label>Candidate</label>
      <select
        value={props.controller.selectedCandidateRun}
        onChange={(event) => props.controller.onSelectedCandidateRunChanged(event.target.value)}
      >
        <option value="">Select run</option>
        {props.runs.map((run) => (
          <option key={run.run_id} value={run.run_id}>
            {run.run_id}
          </option>
        ))}
      </select>
      <button onClick={props.onCompareRuns}>Compare</button>
      {props.controller.compareSummary ? (
        <p className="hint-line">{props.controller.compareSummary}</p>
      ) : null}
    </aside>
  );
}
