export type RunMeta = {
  run_id: string;
  created_at: string;
  scenario_name: string;
  seed: number;
  horizon_s: number;
  time_bin_s: number;
  time_of_day_s?: number | null;
  output_path: string;
  summary_path: string;
  meta_path: string;
};

export type PlanningRunConfig = {
  deterministic_seed?: number | null;
  horizon_s?: number | null;
  time_bin_s?: number | null;
  time_of_day_s?: number | null;
};

export type CompareResult = {
  base_run_id: string;
  candidate_run_id: string;
  base: {
    total_trips: number;
    share_trips_served: number;
    mean_generalized_cost_s: number;
    mean_wait_time_s: number;
  };
  candidate: {
    total_trips: number;
    share_trips_served: number;
    mean_generalized_cost_s: number;
    mean_wait_time_s: number;
  };
  delta: {
    kpis: {
      total_trips: number;
      mean_generalized_cost_s: number;
      mean_wait_time_s: number;
      mean_walk_time_s: number;
      mean_transfers: number;
    };
  };
};

export type Mission = {
  id: string;
  title: string;
  description: string;
  status: "active" | "completed" | "blocked";
};

export type AlertItem = {
  id: string;
  title: string;
  detail?: string | null;
  severity: "info" | "warn" | "critical";
  action_label?: string | null;
  target?:
    | {
        kind: "line" | "stop" | "region";
        id: string;
      }
    | null;
};
