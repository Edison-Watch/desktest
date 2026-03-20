mod agent;
mod artifacts;
mod cli;
mod codify;
mod config;
mod docker;
mod error;
mod evaluator;
mod input;
mod interactive;
mod monitor;
mod monitor_server;
mod observation;
mod orchestration;
mod provider;
mod readiness;
mod recording;
mod results;
mod review;
mod screenshot;
mod setup;
mod suite;
mod task;
mod trajectory;

pub(crate) use orchestration::{parse_resolution, run_task};

use clap::Parser;
use cli::{Cli, Command};

fn setup_logging(debug: bool) {
    use tracing_subscriber::EnvFilter;

    let filter = if debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn maybe_start_monitor(monitor_enabled: bool, monitor_port: u16) -> Option<monitor::MonitorHandle> {
    if !monitor_enabled {
        return None;
    }
    let handle = monitor::MonitorHandle::new(32);
    if let Some(_server) = monitor_server::start_monitor_server(handle.clone(), monitor_port).await {
        println!("Monitor dashboard: http://localhost:{}", monitor_port);
        Some(handle)
    } else {
        None
    }
}

#[tokio::main]
async fn main() {
    // Load .env file if present (silently ignored if missing)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();
    setup_logging(cli.debug);

    // Handle subcommands first
    if let Some(command) = &cli.command {
        match command {
            Command::Validate { task } => {
                match task::TaskDefinition::load(task) {
                    Ok(task_def) => {
                        println!("Task '{}' is valid (schema v{}).", task_def.id, task_def.schema_version);
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("Validation error: {e}");
                        std::process::exit(e.exit_code());
                    }
                }
            }
            Command::Run { task } => {
                let task_def = match task::TaskDefinition::load(task) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Task load error: {e}");
                        std::process::exit(e.exit_code());
                    }
                };

                let run_config = orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);
                let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;

                let result = orchestration::run_task(task_def, run_config, cli.debug, cli.verbose, !cli.record, cli.output.clone(), monitor_handle).await;
                match result {
                    Ok(outcome) => {
                        println!("{outcome}");
                        std::process::exit(if outcome.passed { 0 } else { 1 });
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(e.exit_code());
                    }
                }
            }
            Command::Suite { dir, filter } => {
                let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;

                let result = suite::run_suite(
                    dir,
                    cli.config_flag.as_deref(),
                    filter.as_deref(),
                    &cli.output,
                    cli.debug,
                    cli.verbose,
                    !cli.record,
                    cli.resolution.as_deref(),
                    monitor_handle,
                ).await;

                match result {
                    Ok(suite_result) => {
                        std::process::exit(suite::suite_exit_code(&suite_result));
                    }
                    Err(e) => {
                        eprintln!("Suite error: {e}");
                        std::process::exit(e.exit_code());
                    }
                }
            }
            Command::Attach { task, container } => {
                let task_def = match task::TaskDefinition::load(task) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Task load error: {e}");
                        std::process::exit(e.exit_code());
                    }
                };

                let run_config = orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);
                let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;

                let result = orchestration::run_attach(
                    task_def,
                    run_config,
                    container,
                    cli.debug,
                    cli.verbose,
                    !cli.record,
                    cli.output.clone(),
                    monitor_handle,
                ).await;
                match result {
                    Ok(outcome) => {
                        println!("{outcome}");
                        std::process::exit(if outcome.passed { 0 } else { 1 });
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(e.exit_code());
                    }
                }
            }
            Command::Interactive { task, step, validate_only } => {
                let task_def = match task::TaskDefinition::load(task) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Task load error: {e}");
                        std::process::exit(e.exit_code());
                    }
                };

                let run_config = orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);

                let result = interactive::run_interactive(
                    task_def,
                    run_config,
                    cli.debug,
                    cli.verbose,
                    !cli.record,
                    cli.output.clone(),
                    *step,
                    *validate_only,
                ).await;

                match result {
                    Ok(outcome) => {
                        println!("{outcome}");
                        std::process::exit(if outcome.passed { 0 } else { 1 });
                    }
                    Err(e) => {
                        // In interactive mode (no --step, no --validate-only), Ctrl+C is expected
                        if !step && !validate_only && e.is_interrupt() {
                            std::process::exit(0);
                        }
                        eprintln!("Error: {e}");
                        std::process::exit(e.exit_code());
                    }
                }
            }
            Command::Codify { trajectory, output, steps, with_screenshots, threshold, delay } => {
                let entries = match codify::load_trajectory(trajectory) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("Error loading trajectory: {e}");
                        std::process::exit(e.exit_code());
                    }
                };

                let step_filter = match steps {
                    Some(s) => match codify::parse_steps(s) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            eprintln!("Error parsing steps: {e}");
                            std::process::exit(2);
                        }
                    },
                    None => None,
                };

                // Derive screenshots dir name from trajectory's parent directory
                let screenshots_dir_name = if *with_screenshots {
                    let parent = trajectory.parent().unwrap_or(std::path::Path::new("."));
                    let resolved = if parent.as_os_str().is_empty() {
                        std::env::current_dir().ok()
                    } else {
                        std::fs::canonicalize(parent).ok()
                    };
                    let name = resolved
                        .as_deref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string());
                    if name.is_none() {
                        eprintln!("Warning: could not derive screenshots directory name from trajectory path; assertions will reference /home/tester/<filename> directly");
                    }
                    name
                } else {
                    None
                };

                let (script, included_count) = codify::generate_replay_script(
                    &entries,
                    step_filter.as_deref(),
                    *delay,
                    *with_screenshots,
                    *threshold,
                    screenshots_dir_name.as_deref(),
                );

                match std::fs::write(output, &script) {
                    Ok(()) => {
                        println!("Replay script written to {}", output.display());
                        println!("  {} steps included (of {} total)", included_count, entries.len());
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("Error writing script: {e}");
                        std::process::exit(3);
                    }
                }
            }
            Command::Review { artifacts_dir, output, no_open } => {
                match review::generate_review_html(artifacts_dir, output) {
                    Ok(()) => {
                        println!("Review HTML written to {}", output.display());
                        if !*no_open {
                            #[cfg(target_os = "macos")]
                            let _ = std::process::Command::new("open").arg(output).spawn();
                            #[cfg(target_os = "linux")]
                            let _ = std::process::Command::new("xdg-open").arg(output).spawn();
                        }
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("Error generating review: {e}");
                        std::process::exit(e.exit_code());
                    }
                }
            }
        }
    }

    // Legacy CLI: positional args for config + instructions
    let interactive = cli.interactive;
    let result = orchestration::run_legacy(cli).await;

    match result {
        Ok(outcome) => {
            println!("{outcome}");
            std::process::exit(if outcome.passed { 0 } else { 1 });
        }
        Err(e) => {
            // In interactive mode, Ctrl+C is the expected exit path
            if interactive && e.is_interrupt() {
                std::process::exit(0);
            }
            eprintln!("Error: {e}");
            std::process::exit(e.exit_code());
        }
    }
}
