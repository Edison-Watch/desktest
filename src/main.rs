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
mod notify;
mod observation;
mod orchestration;
mod preflight;
mod provider;
mod readiness;
mod recording;
mod redact;
mod results;
mod review;
mod session;
mod setup;
mod suite;
mod tart;
mod task;
mod trajectory;
mod update;

pub(crate) use orchestration::run_task;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use orchestration::{LlmOverrides, RunConfig};

// ANSI color constants (Brand: #C3FFFD = Core Cyan, #9BA4A6 = Graphene Grey)
const CYAN: &str = "\x1b[38;2;195;255;253m";
const GREY: &str = "\x1b[38;2;155;164;166m";
const WHITE_BOLD: &str = "\x1b[1;97m";
const RESET: &str = "\x1b[0m";

fn print_banner(version: &str) {
    const LOGO_LINES: &[&str] = &[
        " ██████╗ ███████╗███████╗██╗  ██╗████████╗███████╗███████╗████████╗",
        " ██╔══██╗██╔════╝██╔════╝██║ ██╔╝╚══██╔══╝██╔════╝██╔════╝╚══██╔══╝",
        " ██║  ██║█████╗  ███████╗█████╔╝    ██║   █████╗  ███████╗   ██║",
        " ██║  ██║██╔══╝  ╚════██║██╔═██╗    ██║   ██╔══╝  ╚════██║   ██║",
        " ██████╔╝███████╗███████║██║  ██╗   ██║   ███████╗███████║   ██║",
        " ╚═════╝ ╚══════╝╚══════╝╚═╝  ╚═╝   ╚═╝   ╚══════╝╚══════╝   ╚═╝",
    ];

    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
    if !is_tty {
        for line in LOGO_LINES {
            println!("{line}");
        }
        println!("  Desktest CLI v{version} — Playwright for full-computer tests");
        println!();
        return;
    }

    // Compute the version tagline to measure its width
    let tagline_plain = format!("  Desktest CLI v{version}");
    let tagline_suffix = " \u{2014} Playwright for full-computer tests";
    let tagline_len = tagline_plain.chars().count() + tagline_suffix.chars().count();

    // Inner width = max of all content lines (logo + tagline) + 2 for left/right padding
    let max_logo = LOGO_LINES.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let inner = std::cmp::max(max_logo, tagline_len) + 2; // +2 for 1-char padding each side
    let box_width = inner + 2; // total width including both █ borders
    let tag_width = (box_width * 40) / 100; // 40% of box width per brand spec

    // ── The Hackbox (solid █ border, half-height top/bottom) ──
    // Top border: half-height bar (▄ = lower half block, hugs content)
    println!("{CYAN}{}{RESET}", "▄".repeat(box_width));

    // Tag row: solid tag continues from top-left, rest is interior space
    let tag_interior = " ".repeat(box_width - tag_width - 1); // -1 for right █ border
    println!("{CYAN}{}{}█{RESET}", "█".repeat(tag_width), tag_interior);

    // Blank row for breathing room between tag and logo
    println!("{CYAN}█{}█{RESET}", " ".repeat(inner));

    // Logo lines inside the box
    for line in LOGO_LINES {
        let visible_len = line.chars().count();
        let padding = if inner > visible_len + 1 {
            inner - visible_len - 1
        } else {
            0
        };
        println!("{CYAN}█{RESET} {line}{}{CYAN}█{RESET}", " ".repeat(padding));
    }

    // Version tagline inside the box
    let tagline_pad = if inner > tagline_len {
        inner - tagline_len
    } else {
        0
    };
    println!(
        "{CYAN}█{WHITE_BOLD}{tagline_plain}{GREY}{tagline_suffix}{}{CYAN}█{RESET}",
        " ".repeat(tagline_pad),
    );

    // Bottom border: half-height bar (▀ = upper half block, hugs content)
    println!("{CYAN}{}{RESET}", "▀".repeat(box_width));

    println!();
}

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
        llm_max_retries: cli.llm_max_retries,
    }
}

async fn maybe_start_monitor(
    monitor_enabled: bool,
    monitor_port: u16,
    monitor_bind_addr: &str,
) -> Option<monitor::MonitorHandle> {
    if !monitor_enabled {
        return None;
    }
    let handle = monitor::MonitorHandle::new(32);
    if let Some(_server) =
        monitor_server::start_monitor_server(handle.clone(), monitor_port, monitor_bind_addr).await
    {
        print_monitor_url(monitor_bind_addr, monitor_port);
        Some(handle)
    } else {
        None
    }
}

/// Print the monitor dashboard URL and a security warning if bound to a non-loopback address.
fn print_monitor_url(bind_addr: &str, port: u16) {
    if bind_addr != "127.0.0.1" && bind_addr != "::1" {
        eprintln!(
            "WARNING: Monitor dashboard is bound to {} — \
             this endpoint has no authentication and is accessible \
             to anyone who can reach this address.",
            bind_addr
        );
    }
    let display_addr = if bind_addr == "0.0.0.0" || bind_addr == "::" {
        format!("localhost:{port}")
    } else {
        config::format_host_port(bind_addr, port)
    };
    println!("Monitor dashboard: http://{display_addr} (bound to {bind_addr})");
}

/// Default glob patterns excluded from home-directory artifact collection.
const DEFAULT_ARTIFACTS_EXCLUDES: &[&str] = &[
    "node_modules",
    ".cache",
    ".npm",
    ".electron",
    ".nvm",
    "GPU Cache",
    "GPUCache",
    "ShaderCache",
];

/// Resolve the effective exclude list: user-provided patterns, or defaults.
/// Pass `--artifacts-exclude=none` to disable all default excludes.
fn resolve_artifacts_exclude(user: &[String]) -> Vec<String> {
    if user.is_empty() {
        return DEFAULT_ARTIFACTS_EXCLUDES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    if user.iter().any(|s| s.eq_ignore_ascii_case("none")) {
        // "none" anywhere in the list → disable defaults; keep any other patterns
        return user
            .iter()
            .filter(|s| !s.eq_ignore_ascii_case("none"))
            .cloned()
            .collect();
    }
    user.to_vec()
}

#[tokio::main]
async fn main() {
    // Load .env file if present (silently ignored if missing)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();
    setup_logging(cli.debug);

    let command = match &cli.command {
        Some(cmd) => cmd,
        None => {
            let mut cmd = Cli::command();
            let version = cmd.get_version().unwrap_or("unknown").to_string();
            print_banner(&version);
            let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
            if is_tty {
                // Thin cyan separator between banner and help
                println!("{CYAN}  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─{RESET}");
                println!();
            }
            if let Err(e) = cmd.print_help() {
                eprintln!("Error displaying help: {e}");
            }
            println!();
            std::process::exit(0);
        }
    };

    match command {
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

            let run_config = orchestration::load_config_or_defaults(
                &cli.config_flag,
                &cli.resolution,
                &llm_overrides(&cli),
            );

            let needs_llm = !*replay && !task_def.is_programmatic_only();
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle =
                maybe_start_monitor(cli.monitor, cli.monitor_port, &cli.monitor_bind_addr).await;
            let run = RunConfig {
                debug: cli.debug,
                verbose: cli.verbose,
                bash_enabled: cli.with_bash || cli.qa,
                no_recording: !cli.record,
                qa: cli.qa,
                artifacts_timeout_secs: cli.artifacts_timeout,
                no_artifacts: cli.no_artifacts,
                artifacts_exclude: resolve_artifacts_exclude(&cli.artifacts_exclude),
                llm_max_retries: run_config.llm_max_retries,
            };

            let result = orchestration::run_task(
                task_def,
                run_config,
                run,
                cli.output.clone(),
                monitor_handle,
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
                eprintln!(
                    "Warning: --artifacts-dir is ignored for suite runs (each test manages its own artifacts directory)."
                );
            }

            let run_config = orchestration::load_config_or_defaults(
                &cli.config_flag,
                &cli.resolution,
                &llm_overrides(&cli),
            );
            // Skip API key check for suites: tasks are discovered dynamically and
            // some may be programmatic-only. Each individual run_task call will
            // check for its own API key requirement.
            if let Err(e) = preflight::run_preflight(&run_config, false).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle =
                maybe_start_monitor(cli.monitor, cli.monitor_port, &cli.monitor_bind_addr).await;
            let run = RunConfig {
                debug: cli.debug,
                verbose: cli.verbose,
                bash_enabled: cli.with_bash || cli.qa,
                no_recording: !cli.record,
                qa: cli.qa,
                artifacts_timeout_secs: cli.artifacts_timeout,
                no_artifacts: cli.no_artifacts,
                artifacts_exclude: resolve_artifacts_exclude(&cli.artifacts_exclude),
                llm_max_retries: run_config.llm_max_retries,
            };

            let result = suite::run_suite(
                dir,
                run_config,
                filter.as_deref(),
                &cli.output,
                run,
                monitor_handle,
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

            let run_config = orchestration::load_config_or_defaults(
                &cli.config_flag,
                &cli.resolution,
                &llm_overrides(&cli),
            );

            let needs_llm = !*replay && !task_def.is_programmatic_only();
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle =
                maybe_start_monitor(cli.monitor, cli.monitor_port, &cli.monitor_bind_addr).await;
            let run = RunConfig {
                debug: cli.debug,
                verbose: cli.verbose,
                bash_enabled: cli.with_bash || cli.qa,
                no_recording: !cli.record,
                qa: cli.qa,
                artifacts_timeout_secs: cli.artifacts_timeout,
                no_artifacts: cli.no_artifacts,
                artifacts_exclude: resolve_artifacts_exclude(&cli.artifacts_exclude),
                llm_max_retries: run_config.llm_max_retries,
            };

            let result = orchestration::run_attach(
                task_def,
                run_config,
                container,
                run,
                cli.output.clone(),
                monitor_handle,
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

            let run_config = orchestration::load_config_or_defaults(
                &cli.config_flag,
                &cli.resolution,
                &llm_overrides(&cli),
            );

            // run_interactive_step unconditionally creates an LLM provider,
            // so any --step invocation needs an API key regardless of evaluator mode.
            let needs_llm = *step && !*validate_only;
            if let Err(e) = preflight::run_preflight(&run_config, needs_llm).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let run = RunConfig {
                debug: cli.debug,
                verbose: cli.verbose,
                bash_enabled: cli.with_bash || cli.qa,
                no_recording: !cli.record,
                qa: cli.qa,
                artifacts_timeout_secs: cli.artifacts_timeout,
                no_artifacts: cli.no_artifacts,
                artifacts_exclude: resolve_artifacts_exclude(&cli.artifacts_exclude),
                llm_max_retries: run_config.llm_max_retries,
            };
            let result = interactive::run_interactive(
                task_def,
                run_config,
                run,
                cli.output.clone(),
                *step,
                *validate_only,
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
                let task_dir = task_path
                    .parent()
                    .and_then(|d| std::fs::canonicalize(d).ok())
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                    });
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
                    let dir_cwd_raw =
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                    let dir_cwd = std::fs::canonicalize(&dir_cwd_raw).unwrap_or(dir_cwd_raw);
                    let dir_abs_raw = if std::path::Path::new(dir_name).is_absolute() {
                        std::path::PathBuf::from(dir_name)
                    } else {
                        dir_cwd.join(dir_name)
                    };
                    let dir_abs = std::fs::canonicalize(&dir_abs_raw).unwrap_or(dir_abs_raw);
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

            let run_config = orchestration::load_config_or_defaults(
                &cli.config_flag,
                &cli.resolution,
                &llm_overrides(&cli),
            );

            // Replay mode doesn't need LLM
            if let Err(e) = preflight::run_preflight(&run_config, false).await {
                eprintln!("Preflight check failed: {e}");
                eprintln!("\nRun `desktest doctor` for detailed diagnostics.");
                std::process::exit(e.exit_code());
            }

            let monitor_handle =
                maybe_start_monitor(cli.monitor, cli.monitor_port, &cli.monitor_bind_addr).await;
            let run = RunConfig {
                debug: cli.debug,
                verbose: cli.verbose,
                bash_enabled: cli.with_bash || cli.qa,
                no_recording: !cli.record,
                qa: cli.qa,
                artifacts_timeout_secs: cli.artifacts_timeout,
                no_artifacts: cli.no_artifacts,
                artifacts_exclude: resolve_artifacts_exclude(&cli.artifacts_exclude),
                llm_max_retries: run_config.llm_max_retries,
            };

            let result = orchestration::run_task(
                task_def,
                run_config,
                run,
                cli.output.clone(),
                monitor_handle,
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
            let run_config = orchestration::load_config_or_defaults(
                &cli.config_flag,
                &cli.resolution,
                &llm_overrides(&cli),
            );
            let all_ok = preflight::run_doctor(&run_config).await;
            std::process::exit(if all_ok { 0 } else { 1 });
        }
        Command::Update { force } => match update::run_update(*force).await {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("Update failed: {e}");
                std::process::exit(e.exit_code());
            }
        },
        Command::Monitor { watch } => {
            if cli.artifacts_dir.is_some() {
                eprintln!(
                    "Warning: --artifacts-dir is ignored for the monitor command (the monitor reads existing artifacts, it does not write them)."
                );
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
                eprintln!("Watch path '{}' is not a directory.", watch_dir.display());
                std::process::exit(2);
            }

            let handle = monitor::MonitorHandle::new(256);
            // Keep the server handle alive for the duration of the watcher loop;
            // dropping it would abort the server task.
            let monitor_addr = cli.monitor_bind_addr.as_str();
            let _server = match monitor_server::start_monitor_server(
                handle.clone(),
                port,
                monitor_addr,
            )
            .await
            {
                Some(server) => {
                    print_monitor_url(monitor_addr, port);
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
