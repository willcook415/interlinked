use clap::{Parser, Subcommand};
use interlinked_engine::{
    PlanningRunOptions, ScenarioFileShape, ScenarioService, SimulationService,
};

#[derive(Parser)]
#[command(name = "interlinked")]
#[command(about = "Interlinked simulation CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a scenario JSON file and write results next to it
    Run {
        /// Path to scenario.json
        scenario_path: String,
        /// Optional output path (defaults to scenario directory / results.json)
        #[arg(long)]
        out: Option<String>,
    },
    /// Migrate a scenario file to canonical wrapped ScenarioDocument format
    Migrate {
        /// Path to input scenario file
        scenario_path: String,

        /// Optional explicit output path
        #[arg(long)]
        out: Option<String>,

        /// Overwrite input file in place
        #[arg(long)]
        in_place: bool,
    },
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { scenario_path, out } => {
            let doc = ScenarioService::load_from_path(&scenario_path)
                .map_err(|e| format!("load/validate error: {e}"))?;

            let output = SimulationService::run_planning(&doc, PlanningRunOptions::default())?;

            let out_path = out.unwrap_or_else(|| {
                let p = std::path::Path::new(&scenario_path);
                let dir = p.parent().unwrap_or_else(|| std::path::Path::new("."));
                dir.join("results.json").to_string_lossy().to_string()
            });

            // We’ll keep writing output exactly like before (results.json is SimulationOutput)
            interlinked_engine::write_json_to_path(&out_path, &output)
                .map_err(|e| format!("write error: {e}"))?;

            println!("✅ Ran scenario: {}", output.meta.scenario_name);
            println!("➡️  Wrote results to: {}", out_path);
            println!("Trips: {:.2}", output.kpis.total_trips);

            Ok(())
        }
        Commands::Migrate {
            scenario_path,
            out,
            in_place,
        } => {
            if in_place && out.is_some() {
                return Err("cannot use --in-place and --out together".to_string());
            }

            let (doc, shape) = ScenarioService::load_from_path_with_shape(&scenario_path)
                .map_err(|e| format!("load/validate error: {e}"))?;

            let output_path = if in_place {
                scenario_path.clone()
            } else if let Some(explicit) = out {
                explicit
            } else {
                default_migrated_output_path(&scenario_path)
            };

            ScenarioService::save_to_path(&output_path, &doc)
                .map_err(|e| format!("save error: {e}"))?;

            println!("Migrated scenario");
            println!("Source: {}", scenario_path);
            println!("Destination: {}", output_path);
            println!("Detected shape: {}", shape_label(shape));
            println!("Schema version: {}", doc.schema_version);
            Ok(())
        }
    }
}

fn default_migrated_output_path(scenario_path: &str) -> String {
    let p = std::path::Path::new(scenario_path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("scenario");
    let filename = format!("{stem}.migrated.json");
    let out = p
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(filename);
    out.to_string_lossy().to_string()
}

fn shape_label(shape: ScenarioFileShape) -> &'static str {
    match shape {
        ScenarioFileShape::Wrapped => "wrapped",
        ScenarioFileShape::LegacyFlat => "legacy_flat",
    }
}

#[cfg(test)]
mod tests {
    use super::default_migrated_output_path;
    use std::path::Path;

    #[test]
    fn default_migrated_output_path_uses_sibling_name() {
        let p = default_migrated_output_path("data/osm_import/leeds_osm/scenario.json");
        let path = Path::new(&p);

        assert_eq!(
            path.file_name().and_then(|x| x.to_str()),
            Some("scenario.migrated.json")
        );

        let parent = path.parent().expect("migrated path should have parent");
        assert!(parent.ends_with(Path::new("data/osm_import/leeds_osm")));
    }
}
