use interlinked_engine::platform::{
    from_base_currency, planning_economy_kpis, PlanningRunOptions, ScenarioService,
    SimulationService,
};
use interlinked_engine::sim::{compare_outputs, SimulationOutput, SimulationSettings};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use super::super::{
    economy_config, new_id, normalize_currency, now_string, read_json_file, read_manifest, runs_dir,
    scenario_path, write_json_file, write_manifest, CompareResult, ExportResult, KpiSlice,
    PlanningRunConfig, RunMeta, RunSummary, ScenarioDocumentLite,
};

fn kpi_slice(output: &SimulationOutput) -> KpiSlice {
    KpiSlice {
        total_trips: output.kpis.total_trips,
        share_trips_served: output.kpis.share_trips_served,
        mean_generalized_cost_s: output.kpis.mean_generalized_cost_s,
        mean_wait_time_s: output.kpis.mean_wait_time_s,
    }
}

#[tauri::command]
pub fn run_planning(
    project_path: String,
    run_config: Option<PlanningRunConfig>,
) -> Result<RunMeta, String> {
    let project_root = PathBuf::from(project_path);
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let mut manifest = read_manifest(&project_root)?;

    let cfg = run_config.unwrap_or(PlanningRunConfig {
        deterministic_seed: None,
        horizon_s: Some(3600.0),
        time_bin_s: Some(300.0),
        time_of_day_s: None,
    });

    let horizon_s = cfg.horizon_s.unwrap_or(3600.0).max(1.0);
    let time_bin_s = cfg.time_bin_s.unwrap_or(300.0).max(1.0);

    // Planning command parity contract:
    // `run_planning` and `run_planning_scenario` both execute through
    // `SimulationService::run_planning` so temporal assumptions do not drift by entry path.
    let settings_override = {
        let mut settings = SimulationSettings::from_params(&doc.scenario.params);
        settings.time_bin_s = time_bin_s;
        settings
    };
    let output = SimulationService::run_planning(
        &doc,
        PlanningRunOptions {
            settings_override: Some(settings_override),
            deterministic_mode: true,
            deterministic_seed: cfg.deterministic_seed,
            time_of_day_s: cfg.time_of_day_s,
            service_day_type: None,
            seasonal_profile: None,
            active_event_ids: None,
        },
    )
    .map_err(|e| e.to_string())?;

    let run_id = new_id("run");
    let run_root = runs_dir(&project_root).join(&run_id);
    fs::create_dir_all(&run_root).map_err(|e| e.to_string())?;

    let output_path = run_root.join("output.json");
    write_json_file(&output_path, &output)?;
    let econ_cfg = economy_config();
    let unlocked = manifest
        .economy
        .unlocked_countries
        .iter()
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>();
    let economy = planning_economy_kpis(&doc.scenario, &unlocked, &econ_cfg);
    let currency = normalize_currency(Some(&manifest.economy.currency));
    let horizon_hours = (horizon_s / 3600.0).max(0.0);
    let projected_base = manifest.economy.current_balance_base
        - economy.estimated_capex_base
        - economy.country_entry_charges_base
        - (economy.estimated_opex_per_hour_base * horizon_hours);
    let summary = RunSummary {
        run_id: run_id.clone(),
        total_trips: output.kpis.total_trips,
        share_trips_served: output.kpis.share_trips_served,
        mean_generalized_cost_s: output.kpis.mean_generalized_cost_s,
        mean_wait_time_s: output.kpis.mean_wait_time_s,
        total_boardings_denied: output.kpis.total_boardings_denied,
        estimated_capex: from_base_currency(economy.estimated_capex_base, &currency, &econ_cfg),
        estimated_opex_per_hour: from_base_currency(
            economy.estimated_opex_per_hour_base,
            &currency,
            &econ_cfg,
        ),
        country_entry_charges: from_base_currency(
            economy.country_entry_charges_base,
            &currency,
            &econ_cfg,
        ),
        projected_net_balance: from_base_currency(projected_base, &currency, &econ_cfg),
    };
    let summary_path = run_root.join("summary.json");
    write_json_file(&summary_path, &summary)?;

    let run_meta = RunMeta {
        run_id: run_id.clone(),
        created_at: now_string(),
        scenario_name: output.meta.scenario_name.clone(),
        seed: output.meta.seed,
        horizon_s,
        time_bin_s,
        time_of_day_s: cfg.time_of_day_s,
        output_path: output_path.to_string_lossy().to_string(),
        summary_path: summary_path.to_string_lossy().to_string(),
        meta_path: run_root.join("meta.json").to_string_lossy().to_string(),
    };
    write_json_file(&run_root.join("meta.json"), &run_meta)?;

    manifest.last_opened_run_id = Some(run_id.clone());
    manifest.recent_runs.retain(|x| x != &run_id);
    manifest.recent_runs.insert(0, run_id);
    if manifest.recent_runs.len() > 30 {
        manifest.recent_runs.truncate(30);
    }
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;

    Ok(run_meta)
}

#[tauri::command]
pub fn export_scenario_report_json(
    project_path: String,
    run_id: String,
    file_path: String,
) -> Result<ExportResult, String> {
    let project_root = PathBuf::from(project_path);
    let run_root = runs_dir(&project_root).join(&run_id);
    let output: SimulationOutput = read_json_file(&run_root.join("output.json"))?;
    let meta: RunMeta = read_json_file(&run_root.join("meta.json"))?;
    let summary: RunSummary = read_json_file(&run_root.join("summary.json"))?;

    let report = serde_json::json!({
        "run": meta,
        "summary": summary,
        "output_meta": output.meta,
        "exported_at": now_string()
    });
    let out = PathBuf::from(file_path);
    write_json_file(&out, &report)?;
    Ok(ExportResult {
        run_id,
        out_path: out.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn export_scenario_report_csv(
    project_path: String,
    run_id: String,
    file_path: String,
) -> Result<ExportResult, String> {
    let project_root = PathBuf::from(project_path);
    let run_root = runs_dir(&project_root).join(&run_id);
    let output: SimulationOutput = read_json_file(&run_root.join("output.json"))?;
    let out = PathBuf::from(file_path);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let csv = format!(
        "run_id,total_trips,share_trips_served,mean_generalized_cost_s,mean_wait_time_s,total_boardings_denied\n{},{:.6},{:.6},{:.6},{:.6},{:.6}\n",
        run_id,
        output.kpis.total_trips,
        output.kpis.share_trips_served,
        output.kpis.mean_generalized_cost_s,
        output.kpis.mean_wait_time_s,
        output.kpis.total_boardings_denied
    );
    fs::write(&out, csv).map_err(|e| e.to_string())?;
    Ok(ExportResult {
        run_id,
        out_path: out.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn compare_runs(
    project_path: String,
    base_run_id: String,
    candidate_run_id: String,
) -> Result<CompareResult, String> {
    let project_root = PathBuf::from(project_path);
    let base_out: SimulationOutput = read_json_file(
        &runs_dir(&project_root)
            .join(&base_run_id)
            .join("output.json"),
    )?;
    let candidate_out: SimulationOutput = read_json_file(
        &runs_dir(&project_root)
            .join(&candidate_run_id)
            .join("output.json"),
    )?;
    let delta = compare_outputs(&base_out, &candidate_out);
    Ok(CompareResult {
        base_run_id,
        candidate_run_id,
        base: kpi_slice(&base_out),
        candidate: kpi_slice(&candidate_out),
        delta,
    })
}

#[tauri::command]
pub fn load_scenario(path: String) -> Result<ScenarioDocumentLite, String> {
    let doc = ScenarioService::load_from_path(path.as_str()).map_err(|e| format!("{e}"))?;
    Ok(ScenarioDocumentLite {
        schema_version: doc.schema_version,
        scenario: doc.scenario,
    })
}

#[tauri::command]
pub fn run_planning_scenario(path: String) -> Result<SimulationOutput, String> {
    let doc = ScenarioService::load_from_path(path.as_str()).map_err(|e| format!("{e}"))?;
    let output = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .map_err(|e| e.to_string())?;
    Ok(output)
}
