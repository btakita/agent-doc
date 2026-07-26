use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use lazily_interop::mutation::run_mutation_check;
use lazily_interop::reference::run_reference_peer;
use lazily_interop::report::{console_report, json_report, junit_report, mutation_console};
use lazily_interop::scenario::{load_registry, load_scenario, resolve_default_paths, run_scenario};
use lazily_interop::schema::{Schemas, validate_schema_files};
use lazily_interop::types::{ErrorCategory, Failure, Plane, RunReport, RunStatus};

#[derive(Debug, Parser)]
#[command(name = "lazily-interop", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one deterministic semantic scenario.
    Run {
        #[arg(long)]
        registry: Option<PathBuf>,
        #[arg(long)]
        scenario: Option<PathBuf>,
        #[arg(long)]
        spec: Option<PathBuf>,
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long, default_value_t = 10_000)]
        timeout_ms: u64,
        #[arg(long)]
        require_channels: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Console)]
        format: OutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Run the intentionally independent peer used to prove the harness.
    ReferencePeer,
    /// Prove all required defect classes make the harness red.
    MutationCheck {
        #[arg(long, default_value_t = 2_000)]
        timeout_ms: u64,
        #[arg(long)]
        json: bool,
    },
    /// Validate the frozen schemas and the default scenario/result contract.
    SchemaCheck,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Console,
    Json,
    Junit,
}

fn main() -> ExitCode {
    match run() {
        Ok(success) => {
            if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("lazily-interop: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool> {
    let cli = Cli::parse();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (_, _, default_spec) = resolve_default_paths(&root);
    match cli.command {
        Command::ReferencePeer => {
            run_reference_peer()?;
            Ok(true)
        }
        Command::SchemaCheck => {
            validate_schema_files(&root)?;
            let schemas = Schemas::load(&root, &default_spec)?;
            let default_scenario = root.join("scenarios/reference-convergence.json");
            let scenario = load_scenario(&default_scenario, &schemas)?;
            let report = RunReport::pass(
                &scenario.name,
                scenario.seed,
                scenario
                    .peers
                    .iter()
                    .map(|peer| peer.name.clone())
                    .collect(),
                Default::default(),
                "lazily-interop run",
            );
            let value = serde_json::to_value(report)?;
            schemas
                .validate_result(&value)
                .map_err(anyhow::Error::new)?;
            let failure_report = RunReport::fail(
                &scenario.name,
                scenario.seed,
                vec!["schema-probe".to_owned()],
                Default::default(),
                "lazily-interop run",
                Failure::new(
                    ErrorCategory::SchemaViolation,
                    Plane::Harness,
                    "schema-probe",
                    "intentional result-schema probe",
                ),
            );
            schemas
                .validate_result(&serde_json::to_value(failure_report)?)
                .map_err(anyhow::Error::new)?;
            println!("schemas: control, scenario, result, distributed — valid");
            Ok(true)
        }
        Command::MutationCheck { timeout_ms, json } => {
            let schemas = Schemas::load(&root, &default_spec)?;
            let scenario =
                load_scenario(&root.join("scenarios/reference-convergence.json"), &schemas)?;
            let executable = std::env::current_exe().context("locate orchestrator executable")?;
            let proofs = run_mutation_check(
                &executable,
                &scenario,
                &schemas,
                Duration::from_millis(timeout_ms),
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&proofs)?);
            } else {
                println!("{}", mutation_console(&proofs));
            }
            Ok(proofs.iter().all(|proof| proof.caught))
        }
        Command::Run {
            registry,
            scenario,
            spec,
            seed,
            timeout_ms,
            require_channels,
            format,
            output,
        } => {
            let (default_registry, default_scenario, _) = resolve_default_paths(&root);
            let registry_path = registry.unwrap_or(default_registry);
            let scenario_path = scenario.unwrap_or(default_scenario);
            let spec_root = spec.unwrap_or(default_spec);
            let schemas = Schemas::load(&root, &spec_root)?;
            let executable = std::env::current_exe().context("locate orchestrator executable")?;
            let registry = load_registry(&registry_path, &executable)?;
            let mut scenario = load_scenario(&scenario_path, &schemas)?;
            if let Some(seed) = seed {
                scenario.seed = seed;
            }
            let replay = replay_command(
                &registry_path,
                &scenario_path,
                &spec_root,
                scenario.seed,
                require_channels,
            );
            let report = run_scenario(
                &scenario,
                &registry,
                &schemas,
                Duration::from_millis(timeout_ms),
                require_channels,
                replay,
            );
            let value = serde_json::to_value(&report)?;
            schemas
                .validate_result(&value)
                .map_err(anyhow::Error::new)?;
            let rendered = match format {
                OutputFormat::Console => console_report(&report),
                OutputFormat::Json => json_report(&report),
                OutputFormat::Junit => junit_report(&report),
            };
            if let Some(path) = output {
                fs::write(&path, rendered)
                    .with_context(|| format!("write report {}", path.display()))?;
            } else {
                println!("{rendered}");
            }
            Ok(report.status == RunStatus::Pass)
        }
    }
}

fn replay_command(
    registry: &Path,
    scenario: &Path,
    spec: &Path,
    seed: u64,
    require_channels: bool,
) -> String {
    let mut command = format!(
        "lazily-interop run --registry {} --scenario {} --spec {} --seed {}",
        shell_arg(registry),
        shell_arg(scenario),
        shell_arg(spec),
        seed
    );
    if require_channels {
        command.push_str(" --require-channels");
    }
    command
}

fn shell_arg(path: &Path) -> String {
    let text = path.display().to_string();
    if text
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "/._-".contains(character))
    {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\"'\"'"))
    }
}
