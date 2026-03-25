mod agent;
mod artifacts;
mod bug_report;
mod cli;
mod codify;
mod config;
mod docker;
mod error;
mod evaluator;
mod interactive;
mod logs;
mod monitor;
mod monitor_server;
mod monitor_watcher;
mod observation;
mod orchestration;
mod preflight;
mod provider;
mod readiness;
mod recording;
mod redact;
mod results;
mod review;
mod setup;
mod suite;
mod task;
mod telemetry;
mod trajectory;
mod update;

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

async fn maybe_start_monitor(
    monitor_enabled: bool,
    monitor_port: u16,
) -> Option<monitor::MonitorHandle> {
    if !monitor_enabled {
        return None;
    }
    let handle = monitor::MonitorHandle::new(32);
    if let Some(_server) = monitor_server::start_monitor_server(handle.clone(), monitor_port).await
    {
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

    let mut telemetry_client = telemetry::TelemetryClient::load_or_init();

    // Handle `desktest telemetry <action>` subcommand early
    if let Command::Telemetry { action } = &cli.command {
        telemetry_client.handle_command(action);
        std::process::exit(0);
    }

    // Check consent / show prompt / nudge (only for test commands)
    if telemetry::is_test_command(&cli.command) {
        telemetry_client.check_consent();
    }

    match &cli.command {
        Command::Validate { task } => match task::TaskDefinition::load(task) {
            Ok(task_def) => {
                println!(
                    "Task '{}' is valid (schema v{}).",
                    task_def.id, task_def.schema_version
                );
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Validation error: {e}");
                std::process::exit(e.exit_code());
            }
        },
        Command::Run { task, replay } => {
            let mut task_def = match task::TaskDefinition::load(task) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Task load error: {e}");
                    std::process::exit(e.exit_code());
                }
            };

            if *replay {
                if let Err(e) = task_def.apply_replay_override() {
                    eprintln!("Error: {e}");
                    std::process::exit(e.exit_code());
                }
            }

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);

            let needs_llm = !*replay && !task_def.is_programmatic_only();
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;
            let bash_enabled = cli.with_bash || cli.qa;
            let start_time = std::time::Instant::now();

            let cmd_name = telemetry::command_name(&cli.command);
            let is_replay = *replay;
            let result = orchestration::run_task(
                task_def,
                run_config,
                cli.debug,
                cli.verbose,
                bash_enabled,
                !cli.record,
                cli.output.clone(),
                monitor_handle,
                cli.qa,
                cli.artifacts_dir.clone(),
            )
            .await;

            record_run_event(&mut telemetry_client, &result, cmd_name, cli.qa, is_replay, bash_enabled, start_time);

            // Print result immediately so the user doesn't wait for telemetry flush
            let exit_code = match &result {
                Ok(outcome) => {
                    println!("{outcome}");
                    if outcome.passed { 0 } else { 1 }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    e.exit_code()
                }
            };

            let effective_artifacts_dir = cli.artifacts_dir.clone().unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join("desktest_artifacts")
            });
            telemetry_client.set_artifacts_dir(effective_artifacts_dir);
            telemetry_client.flush().await;

            std::process::exit(exit_code);
        }
        Command::Suite { dir, filter } => {
            if cli.artifacts_dir.is_some() {
                eprintln!("Warning: --artifacts-dir is ignored for suite runs (each test manages its own artifacts directory).");
            }

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);
            // Skip API key check for suites: tasks are discovered dynamically and
            // some may be programmatic-only. Each individual run_task call will
            // check for its own API key requirement.
            if let Err(e) = preflight::run_preflight(&run_config, false).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;
            let bash_enabled = cli.with_bash || cli.qa;

            let start_time = std::time::Instant::now();
            let result = suite::run_suite(
                dir,
                cli.config_flag.as_deref(),
                filter.as_deref(),
                &cli.output,
                cli.debug,
                cli.verbose,
                bash_enabled,
                !cli.record,
                cli.resolution.as_deref(),
                monitor_handle,
                cli.qa,
            )
            .await;

            // Record suite telemetry event
            match &result {
                Ok(suite_result) => {
                    let mut event = telemetry::build_event(&telemetry_client, "suite_completed", "suite");
                    event.status = Some(if suite_result.summary.failed == 0 && suite_result.summary.errors == 0 { "pass" } else { "fail" }.to_string());
                    event.duration_ms = Some(suite_result.total_duration_ms);
                    event.used_qa_mode = cli.qa;
                    event.used_bash = bash_enabled;
                    telemetry_client.record_event(event);
                }
                Err(e) => {
                    let mut event = telemetry::build_event(&telemetry_client, "error", "suite");
                    event.status = Some("error".to_string());
                    event.duration_ms = Some(start_time.elapsed().as_millis() as u64);
                    event.error_category = Some(format!("exit_{}", e.exit_code()));
                    telemetry_client.record_event(event);
                }
            }
            // Print result immediately so the user doesn't wait for telemetry flush
            let exit_code = match &result {
                Ok(suite_result) => suite::suite_exit_code(suite_result),
                Err(e) => {
                    eprintln!("Suite error: {e}");
                    e.exit_code()
                }
            };

            // Suite manages its own per-test artifacts dirs, but set the output dir for suite-level upload
            if let Some(ref dir) = cli.artifacts_dir {
                telemetry_client.set_artifacts_dir(dir.clone());
            }
            telemetry_client.flush().await;

            std::process::exit(exit_code);
        }
        Command::Attach {
            task,
            container,
            replay,
        } => {
            let mut task_def = match task::TaskDefinition::load(task) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Task load error: {e}");
                    std::process::exit(e.exit_code());
                }
            };

            if *replay {
                if let Err(e) = task_def.apply_replay_override() {
                    eprintln!("Error: {e}");
                    std::process::exit(e.exit_code());
                }
            }

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);

            let needs_llm = !*replay && !task_def.is_programmatic_only();
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;
            let bash_enabled = cli.with_bash || cli.qa;
            let start_time = std::time::Instant::now();
            let is_replay = *replay;

            let result = orchestration::run_attach(
                task_def,
                run_config,
                container,
                cli.debug,
                cli.verbose,
                bash_enabled,
                !cli.record,
                cli.output.clone(),
                monitor_handle,
                cli.qa,
                cli.artifacts_dir.clone(),
            )
            .await;

            record_run_event(&mut telemetry_client, &result, "attach", cli.qa, is_replay, bash_enabled, start_time);

            // Print result immediately so the user doesn't wait for telemetry flush
            let exit_code = match &result {
                Ok(outcome) => {
                    println!("{outcome}");
                    if outcome.passed { 0 } else { 1 }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    e.exit_code()
                }
            };

            let effective_artifacts_dir = cli.artifacts_dir.clone().unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join("desktest_artifacts")
            });
            telemetry_client.set_artifacts_dir(effective_artifacts_dir);
            telemetry_client.flush().await;

            std::process::exit(exit_code);
        }
        Command::Interactive {
            task,
            step,
            validate_only,
        } => {
            let task_def = match task::TaskDefinition::load(task) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Task load error: {e}");
                    std::process::exit(e.exit_code());
                }
            };

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);

            // run_interactive_step unconditionally creates an LLM provider,
            // so any --step invocation needs an API key regardless of evaluator mode.
            let needs_llm = *step && !*validate_only;
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let bash_enabled = cli.with_bash || cli.qa;
            let start_time = std::time::Instant::now();
            let result = interactive::run_interactive(
                task_def,
                run_config,
                cli.debug,
                cli.verbose,
                bash_enabled,
                !cli.record,
                cli.output.clone(),
                *step,
                *validate_only,
                cli.qa,
                cli.artifacts_dir.clone(),
            )
            .await;

            // In interactive mode (no --step, no --validate-only), Ctrl+C is expected — not an error
            let is_expected_interrupt = matches!(&result, Err(e) if !step && !validate_only && e.is_interrupt());

            if !is_expected_interrupt {
                record_run_event(&mut telemetry_client, &result, "interactive", cli.qa, false, bash_enabled, start_time);
            }

            // Print result immediately so the user doesn't wait for telemetry flush
            let exit_code = match &result {
                Ok(outcome) => {
                    println!("{outcome}");
                    if outcome.passed { 0 } else { 1 }
                }
                Err(e) => {
                    if is_expected_interrupt {
                        0
                    } else {
                        eprintln!("Error: {e}");
                        e.exit_code()
                    }
                }
            };

            let effective_artifacts_dir = cli.artifacts_dir.clone().unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join("desktest_artifacts")
            });
            telemetry_client.set_artifacts_dir(effective_artifacts_dir);
            telemetry_client.flush().await;

            std::process::exit(exit_code);
        }
        Command::Codify {
            trajectory,
            output,
            overwrite,
            steps,
            with_screenshots,
            threshold,
            delay,
        } => {
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
                    eprintln!(
                        "Warning: could not derive screenshots directory name from trajectory path; assertions will reference /home/tester/<filename> directly"
                    );
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

            // Load task JSON once if --overwrite is provided (used for both path resolution and update)
            let overwrite_json: Option<(std::path::PathBuf, serde_json::Value)> =
                if let Some(task_path) = &overwrite {
                    let raw = match std::fs::read_to_string(task_path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Error reading task JSON for --overwrite: {e}");
                            std::process::exit(2);
                        }
                    };
                    let value: serde_json::Value = match serde_json::from_str(&raw) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Error parsing task JSON for --overwrite: {e}");
                            std::process::exit(2);
                        }
                    };
                    Some((task_path.clone(), value))
                } else {
                    None
                };

            // If the task JSON already has a replay_script, overwrite that path instead.
            // replay_script is CWD-relative (evaluator resolves it from CWD), so use it directly.
            let effective_output = if let Some((ref _task_path, ref value)) = overwrite_json {
                if let Some(existing_script) = value.get("replay_script").and_then(|v| v.as_str()) {
                    if existing_script != output.to_string_lossy().as_ref() {
                        eprintln!(
                            "Note: --output ignored; writing to existing replay_script path '{}' from task JSON",
                            existing_script
                        );
                    }
                    std::borrow::Cow::Owned(std::path::PathBuf::from(existing_script))
                } else {
                    std::borrow::Cow::Borrowed(output.as_path())
                }
            } else {
                std::borrow::Cow::Borrowed(output.as_path())
            };

            match std::fs::write(&*effective_output, &script) {
                Ok(()) => {
                    println!("Replay script written to {}", effective_output.display());
                    println!(
                        "  {} steps included (of {} total)",
                        included_count,
                        entries.len()
                    );
                }
                Err(e) => {
                    eprintln!("Error writing script: {e}");
                    std::process::exit(3);
                }
            }

            // Patch the task JSON with replay_script/replay_screenshots_dir (preserves unknown fields)
            if let Some((task_path, mut value)) = overwrite_json {
                // Store replay_script as CWD-relative (evaluator resolves from CWD)
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                let script_abs = std::fs::canonicalize(&*effective_output)
                    .unwrap_or_else(|_| effective_output.to_path_buf());
                let cwd_abs = std::fs::canonicalize(&cwd).unwrap_or(cwd);
                let script_rel = script_abs
                    .strip_prefix(&cwd_abs)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| effective_output.to_path_buf());

                let obj = value.as_object_mut().expect("task JSON must be an object");
                obj.insert(
                    "replay_script".to_string(),
                    serde_json::Value::String(script_rel.to_string_lossy().to_string()),
                );

                if *with_screenshots {
                    let dir_name = screenshots_dir_name
                        .as_deref()
                        .unwrap_or("desktest_artifacts");
                    obj.insert(
                        "replay_screenshots_dir".to_string(),
                        serde_json::Value::String(dir_name.to_string()),
                    );
                } else {
                    obj.remove("replay_screenshots_dir");
                }

                let json = serde_json::to_string_pretty(&value).expect("serialize task JSON");
                match std::fs::write(&task_path, json) {
                    Ok(()) => {
                        println!("Updated {} with replay_script", task_path.display());
                    }
                    Err(e) => {
                        eprintln!("Error updating task JSON: {e}");
                        std::process::exit(3);
                    }
                }
            }

            std::process::exit(0);
        }
        Command::Replay {
            task,
            script,
            screenshots_dir,
        } => {
            eprintln!(
                "Warning: `desktest replay` is deprecated. Instead, set 'replay_script' in your task JSON and use `desktest run --replay`."
            );

            let mut task_def = match task::TaskDefinition::load(task) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Task load error: {e}");
                    std::process::exit(e.exit_code());
                }
            };

            // Set replay fields from CLI args and delegate to shared method
            task_def.replay_script = Some(script.to_string_lossy().to_string());
            task_def.replay_screenshots_dir = screenshots_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string());

            if let Err(e) = task_def.apply_replay_override() {
                eprintln!("Error: {e}");
                std::process::exit(e.exit_code());
            }

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);

            // Replay mode doesn't need LLM
            if let Err(e) = preflight::run_preflight(&run_config, false).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;
            let bash_enabled = cli.with_bash || cli.qa;

            let result = orchestration::run_task(
                task_def,
                run_config,
                cli.debug,
                cli.verbose,
                bash_enabled,
                !cli.record,
                cli.output.clone(),
                monitor_handle,
                cli.qa,
                cli.artifacts_dir.clone(),
            )
            .await;
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
        Command::Logs {
            artifacts_dir,
            brief,
            step,
            steps,
        } => {
            let step_filter = match (step, steps) {
                (Some(n), _) => Some(vec![*n]),
                (_, Some(s)) => match codify::parse_steps(s) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        eprintln!("Error parsing steps: {e}");
                        std::process::exit(2);
                    }
                },
                _ => None,
            };
            match logs::print_logs(artifacts_dir, *brief, step_filter) {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(e.exit_code());
                }
            }
        }
        Command::Doctor => {
            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution);
            let all_ok = preflight::run_doctor(&run_config).await;
            std::process::exit(if all_ok { 0 } else { 1 });
        }
        Command::Update { force } => {
            match update::run_update(*force).await {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("Update failed: {e}");
                    std::process::exit(e.exit_code());
                }
            }
        }
        Command::Monitor { watch } => {
            if cli.artifacts_dir.is_some() {
                eprintln!("Warning: --artifacts-dir is ignored for the monitor command (the monitor reads existing artifacts, it does not write them).");
            }
            let watch_dir = watch.clone();
            let port = cli.monitor_port;
            if !watch_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&watch_dir) {
                    eprintln!(
                        "Cannot create watch directory '{}': {e}",
                        watch_dir.display()
                    );
                    std::process::exit(2);
                }
            } else if !watch_dir.is_dir() {
                eprintln!(
                    "Watch path '{}' is not a directory.",
                    watch_dir.display()
                );
                std::process::exit(2);
            }

            let handle = monitor::MonitorHandle::new(256);
            // Keep the server handle alive for the duration of the watcher loop;
            // dropping it would abort the server task.
            let _server = match monitor_server::start_monitor_server(handle.clone(), port).await {
                Some(server) => {
                    println!("Monitor dashboard: http://localhost:{}", port);
                    println!(
                        "Watching {} for phase directories (Ctrl+C to stop)",
                        watch_dir.display()
                    );
                    server
                }
                None => {
                    eprintln!("Failed to start monitor server on port {port}");
                    std::process::exit(3);
                }
            };

            // Run the watcher until Ctrl+C
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => {
                    println!("\nShutting down monitor.");
                    std::process::exit(0);
                }
                _ = monitor_watcher::run_watcher(watch_dir, handle) => {
                    // run_watcher loops forever, so this arm shouldn't complete
                }
            }
        }
        Command::Review {
            artifacts_dir,
            output,
            no_open,
        } => match review::generate_review_html(artifacts_dir, output) {
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
        },
        Command::Telemetry { .. } => {
            // Handled above before the match; this arm is unreachable
            unreachable!()
        }
    }
}

/// Record a telemetry event for a single test run (used by Run and Attach commands).
fn record_run_event(
    client: &mut telemetry::TelemetryClient,
    result: &Result<crate::error::AgentOutcome, crate::error::AppError>,
    command: &str,
    qa: bool,
    replay: bool,
    bash_enabled: bool,
    start_time: std::time::Instant,
) {
    let duration_ms = start_time.elapsed().as_millis() as u64;
    let mut event = telemetry::build_event(client, "test_completed", command);
    event.duration_ms = Some(duration_ms);
    event.used_qa_mode = qa;
    event.used_replay = replay;
    event.used_bash = bash_enabled;

    match result {
        Ok(outcome) => {
            event.status = Some(if outcome.passed { "pass" } else { "fail" }.to_string());
        }
        Err(e) => {
            event.event_type = "error".to_string();
            event.status = Some("error".to_string());
            event.error_category = Some(format!("exit_{}", e.exit_code()));
        }
    }

    client.record_event(event);
}
