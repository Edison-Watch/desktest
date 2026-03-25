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
mod trajectory;
mod update;

pub(crate) use orchestration::{parse_resolution, run_task};

use clap::Parser;
use cli::{Cli, Command};
use orchestration::LlmOverrides;

fn setup_logging(debug: bool) {
    use tracing_subscriber::EnvFilter;

    let filter = if debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn llm_overrides(cli: &Cli) -> LlmOverrides {
    LlmOverrides {
        provider: cli.provider.clone(),
        model: cli.model.clone(),
        api_key: cli.api_key.clone(),
    }
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

            if !*replay && task_def.has_replay_script() && !task_def.is_programmatic_only() {
                eprintln!(
                    "Warning: Task has 'replay_script' but running in LLM mode — did you mean --replay?"
                );
            }

            if *replay {
                if let Err(e) = task_def.apply_replay_override() {
                    eprintln!("Error: {e}");
                    std::process::exit(e.exit_code());
                }
            }

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution, &llm_overrides(&cli));

            let needs_llm = !*replay && !task_def.is_programmatic_only();
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
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
        Command::Suite { dir, filter } => {
            if cli.artifacts_dir.is_some() {
                eprintln!("Warning: --artifacts-dir is ignored for suite runs (each test manages its own artifacts directory).");
            }

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution, &llm_overrides(&cli));
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

            if !*replay && task_def.has_replay_script() && !task_def.is_programmatic_only() {
                eprintln!(
                    "Warning: Task has 'replay_script' but running in LLM mode — did you mean --replay?"
                );
            }

            if *replay {
                if let Err(e) = task_def.apply_replay_override() {
                    eprintln!("Error: {e}");
                    std::process::exit(e.exit_code());
                }
            }

            let run_config =
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution, &llm_overrides(&cli));

            let needs_llm = !*replay && !task_def.is_programmatic_only();
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle = maybe_start_monitor(cli.monitor, cli.monitor_port).await;
            let bash_enabled = cli.with_bash || cli.qa;

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
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution, &llm_overrides(&cli));

            // run_interactive_step unconditionally creates an LLM provider,
            // so any --step invocation needs an API key regardless of evaluator mode.
            let needs_llm = *step && !*validate_only;
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let bash_enabled = cli.with_bash || cli.qa;
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
            // replay_script is stored relative to the task JSON's directory, so resolve
            // it against the task file's parent before using as a filesystem write target.
            let effective_output = if let Some((ref task_path, ref value)) = overwrite_json {
                if let Some(existing_script) = value.get("replay_script").and_then(|v| v.as_str()) {
                    let task_parent = task_path.parent().unwrap_or(std::path::Path::new("."));
                    let resolved = task_parent.join(existing_script);
                    // Compare canonicalized paths to avoid spurious "ignored" warnings
                    // when the same file is referenced from different bases.
                    let resolved_canon = std::fs::canonicalize(&resolved).ok();
                    let output_canon = std::fs::canonicalize(output.as_path()).ok();
                    if resolved_canon.is_none() {
                        // Old replay_script path no longer exists — honor --output instead
                        eprintln!(
                            "Warning: replay_script path '{}' in task JSON does not exist; using --output instead",
                            resolved.display()
                        );
                        std::borrow::Cow::Borrowed(output.as_path())
                    } else if resolved_canon != output_canon {
                        eprintln!(
                            "Note: --output ignored; writing to existing replay_script path '{}' from task JSON",
                            resolved.display()
                        );
                        std::borrow::Cow::Owned(resolved)
                    } else {
                        std::borrow::Cow::Owned(resolved)
                    }
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
                // Store replay_script relative to the task JSON's directory so that
                // TaskDefinition::load resolves it correctly regardless of CWD.
                let task_dir = task_path.parent().and_then(|d| std::fs::canonicalize(d).ok())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));
                let script_abs = std::fs::canonicalize(&*effective_output)
                    .unwrap_or_else(|_| effective_output.to_path_buf());
                let script_rel = script_abs
                    .strip_prefix(&task_dir)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| {
                        eprintln!(
                            "Note: replay_script '{}' is outside the task directory; storing as absolute path (not portable across machines)",
                            script_abs.display()
                        );
                        script_abs.clone()
                    });

                let obj = value.as_object_mut().expect("task JSON must be an object");
                obj.insert(
                    "replay_script".to_string(),
                    serde_json::Value::String(script_rel.to_string_lossy().to_string()),
                );

                if *with_screenshots {
                    let dir_name = screenshots_dir_name
                        .as_deref()
                        .unwrap_or("desktest_artifacts");
                    // Resolve dir_name against CWD first to get a reliable absolute path,
                    // then make it relative to task_dir for portability.
                    let dir_cwd_raw = std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."));
                    let dir_cwd = std::fs::canonicalize(&dir_cwd_raw)
                        .unwrap_or(dir_cwd_raw);
                    let dir_abs_raw = if std::path::Path::new(dir_name).is_absolute() {
                        std::path::PathBuf::from(dir_name)
                    } else {
                        dir_cwd.join(dir_name)
                    };
                    let dir_abs = std::fs::canonicalize(&dir_abs_raw)
                        .unwrap_or(dir_abs_raw);
                    let dir_rel = dir_abs
                        .strip_prefix(&task_dir)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| {
                            eprintln!(
                                "Note: replay_screenshots_dir '{}' is outside the task directory; storing as absolute path (not portable across machines)",
                                dir_abs.display()
                            );
                            dir_abs.clone()
                        });
                    obj.insert(
                        "replay_screenshots_dir".to_string(),
                        serde_json::Value::String(dir_rel.to_string_lossy().to_string()),
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
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution, &llm_overrides(&cli));

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
                orchestration::load_config_or_defaults(&cli.config_flag, &cli.resolution, &llm_overrides(&cli));
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
    }
}
