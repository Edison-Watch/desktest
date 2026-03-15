mod agent;
mod artifacts;
mod config;
mod docker;
mod error;
mod evaluator;
mod input;
mod observation;
mod provider;
mod readiness;
mod recording;
mod results;
mod screenshot;
mod setup;
mod suite;
mod task;
mod trajectory;

use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use config::Config;
use error::{AgentOutcome, AppError};
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "tent",
    about = "LLM-powered desktop app tester",
    after_help = "\
EXAMPLES:
  Legacy mode (backward compatible):
    tent config.json instructions.md
    tent --interactive config.json instructions.md

  Subcommand mode:
    tent run task.json
    tent run task.json --config config.json --output ./results
    tent suite ./tests --filter gedit
    tent interactive task.json
    tent validate task.json"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Path to the JSON config file (legacy positional arg)
    pub config_pos: Option<std::path::PathBuf>,

    /// Path to the instructions Markdown file (legacy positional arg)
    pub instructions: Option<std::path::PathBuf>,

    /// Path to config JSON file (API key, provider, display settings)
    #[arg(long = "config", global = true)]
    pub config_flag: Option<std::path::PathBuf>,

    /// Output directory for results (default: ./test-results/)
    #[arg(long, global = true, default_value = results::DEFAULT_OUTPUT_DIR)]
    pub output: std::path::PathBuf,

    /// Enable debug mode (verbose logging)
    #[arg(long, default_value_t = false, global = true)]
    pub debug: bool,

    /// Enable verbose trajectory logging (includes full LLM responses in trajectory.jsonl)
    #[arg(long, default_value_t = false, global = true)]
    pub verbose: bool,

    /// Disable video recording of test sessions
    #[arg(long, default_value_t = false, global = true)]
    pub no_recording: bool,

    /// Interactive mode: start container and app, then wait for Ctrl+C (no agent) [legacy flag]
    #[arg(long, default_value_t = false, hide = true)]
    pub interactive: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Validate a task JSON file against the schema without running anything
    #[command(after_help = "\
EXAMPLES:
  tent validate task.json
  tent validate tests/gedit-save.json")]
    Validate {
        /// Path to the task JSON file to validate
        task: std::path::PathBuf,
    },

    /// Run a single test from a task JSON file
    #[command(after_help = "\
EXAMPLES:
  tent run task.json
  tent run task.json --config config.json
  tent run task.json --output ./my-results --verbose
  tent run task.json --no-recording --debug")]
    Run {
        /// Path to the task JSON file
        task: std::path::PathBuf,
    },

    /// Run a suite of tests from a directory of task JSON files
    #[command(after_help = "\
EXAMPLES:
  tent suite ./tests
  tent suite ./tests --filter gedit
  tent suite ./tests --config config.json --output ./results")]
    Suite {
        /// Path to the directory containing task JSON files
        dir: std::path::PathBuf,

        /// Run only tests matching this name pattern
        #[arg(long)]
        filter: Option<String>,
    },

    /// Start a container with a task for interactive development and debugging
    #[command(after_help = "\
EXAMPLES:
  tent interactive task.json                   # Start container, run setup, pause
  tent interactive task.json --step            # Run agent one step at a time
  tent interactive task.json --validate-only   # Skip agent, run evaluation only
  tent interactive task.json --config c.json   # Use custom config")]
    Interactive {
        /// Path to the task JSON file
        task: std::path::PathBuf,

        /// Run agent one step at a time, pausing after each step
        #[arg(long, default_value_t = false)]
        step: bool,

        /// Skip agent loop, run programmatic evaluation only
        #[arg(long, default_value_t = false)]
        validate_only: bool,
    },
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

                let run_config = load_config_or_defaults(&cli.config_flag);

                let result = run_task(task_def, run_config, cli.debug, cli.verbose, cli.no_recording, cli.output.clone()).await;
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
                let result = suite::run_suite(
                    dir,
                    cli.config_flag.as_deref(),
                    filter.as_deref(),
                    &cli.output,
                    cli.debug,
                    cli.verbose,
                    cli.no_recording,
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
            Command::Interactive { task, step, validate_only } => {
                let task_def = match task::TaskDefinition::load(task) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Task load error: {e}");
                        std::process::exit(e.exit_code());
                    }
                };

                let run_config = load_config_or_defaults(&cli.config_flag);

                let result = run_interactive(
                    task_def,
                    run_config,
                    cli.debug,
                    cli.verbose,
                    cli.no_recording,
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
        }
    }

    // Legacy CLI: positional args for config + instructions
    let interactive = cli.interactive;
    let result = run_legacy(cli).await;

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

/// Load config from --config flag path or use task defaults.
fn load_config_or_defaults(config_flag: &Option<std::path::PathBuf>) -> Config {
    if let Some(config_path) = config_flag {
        match config::Config::load_and_validate(config_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Config error: {e}");
                std::process::exit(e.exit_code());
            }
        }
    } else {
        config::Config::from_task_defaults()
    }
}

async fn run_legacy(cli: Cli) -> Result<AgentOutcome, AppError> {
    // 1. Validate config
    let config_path = cli.config_pos.ok_or_else(|| {
        AppError::Config("Missing config file argument. Usage: tent <config.json> <instructions.md>\n\nOr use subcommands: tent run <task.json>, tent suite <dir>, tent interactive <task.json>, tent validate <task.json>".into())
    })?;
    let config = Config::load_and_validate(&config_path)?;

    // 2. Read instructions
    let instructions_path = cli.instructions.ok_or_else(|| {
        AppError::Config("Missing instructions file argument. Usage: tent <config.json> <instructions.md>".into())
    })?;
    let instructions = std::fs::read_to_string(&instructions_path)
        .map_err(|e| AppError::Config(format!("Cannot read instructions file: {e}")))?;

    // 3. Set up artifacts directory
    let artifacts_dir = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    // 4. Create and start Docker container
    info!("Creating Docker container...");
    let session = docker::DockerSession::create(&config, None).await?;

    // Run the main logic, racing against Ctrl+C.
    // No matter how we exit (success, error, or signal), cleanup always runs.
    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_inner(&config, &session, &artifacts_dir, &instructions, cli.debug, cli.interactive) => r,
    };

    // Always collect artifacts and clean up
    info!("Collecting artifacts...");
    let _ = artifacts::collect_artifacts(&session, &artifacts_dir).await;

    info!("Cleaning up container...");
    let _ = session.cleanup().await;

    result
}

async fn run_inner(
    config: &Config,
    session: &docker::DockerSession,
    artifacts_dir: &std::path::Path,
    instructions: &str,
    debug: bool,
    interactive: bool,
) -> Result<AgentOutcome, AppError> {
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    // 5. Wait for desktop to be ready
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 6. Deploy app into container
    info!("Deploying app...");
    let app_path = session.deploy_app(config).await?;

    // 7. Get stable baseline windows, launch app, wait for app window
    info!("Waiting for stable window baseline...");
    let baseline_windows = readiness::get_stable_window_list(session).await?;

    let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
    info!("Launching app: {app_path}");
    session.launch_app(&app_path, is_appimage).await?;

    // Give the app a moment to start (or crash)
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check if the app process is still running
    let pgrep_cmd = format!("pgrep -f {} || true", shell_escape::escape(app_path.as_str().into()));
    let ps_check = session
        .exec(&["bash", "-c", &pgrep_cmd])
        .await;
    if let Ok(output) = &ps_check {
        if output.trim().is_empty() {
            // App process not found - check the log for errors
            let log = session
                .exec(&["cat", "/tmp/app.log"])
                .await
                .unwrap_or_default();
            if !log.trim().is_empty() {
                tracing::warn!("App process died. Log output:\n{log}");
            } else {
                tracing::warn!("App process not found and no log output");
            }
        }
    }

    info!("Waiting for app window...");
    readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;

    // 8. Print VNC info
    if let Some(port) = session.vnc_host_port().await {
        println!("VNC available at {}:{}", config.vnc_bind_addr, port);
    }

    // 9. Interactive mode: just wait, or run agent loop
    if interactive {
        println!("Interactive mode: container is running. Press Ctrl+C to stop.");
        println!("Container ID: {}", session.container_id);
        println!("  docker exec -it {} bash", session.container_id);
        // Wait forever (Ctrl+C is handled by the select! in run())
        std::future::pending::<()>().await;
        unreachable!()
    }

    info!("Starting agent loop...");
    let llm_client = provider::create_provider(
        &config.provider,
        &config.api_key,
        &config.model,
        &config.api_base_url,
    )?;
    let mut agent_loop = agent::AgentLoop::new(
        llm_client,
        session,
        artifacts_dir.to_path_buf(),
        instructions.to_string(),
        debug,
    );
    agent_loop.run().await
}

/// Internal result from run_task_inner, preserving evaluation details for results.json.
struct TaskRunResult {
    outcome: AgentOutcome,
    eval_result: Option<evaluator::EvaluationResult>,
    /// True when an agent loop was run (LLM or hybrid mode).
    agent_ran: bool,
}

/// Run a test from a task definition file.
///
/// This is the new task-based flow: load task → create container → wait for desktop →
/// run setup steps → deploy & launch app → run agent loop → cleanup.
pub(crate) async fn run_task(
    task_def: task::TaskDefinition,
    mut config: Config,
    debug: bool,
    verbose: bool,
    no_recording: bool,
    output_dir: std::path::PathBuf,
) -> Result<AgentOutcome, AppError> {
    let start = Instant::now();

    // Populate config app fields from task definition (needed when no --config file)
    config.apply_task_app(&task_def.app);

    // Set up artifacts directory
    let artifacts_dir = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    // Determine custom Docker image from task definition
    let custom_image = match &task_def.app {
        task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
        _ => None,
    };

    // Create and start Docker container (inside select! so Ctrl+C works during image pull)
    info!("Creating Docker container...");
    let session = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C) during container setup");
            return Err(AppError::Infra("Interrupted by user".into()));
        }
        r = docker::DockerSession::create(&config, custom_image) => r?,
    };

    // Validate custom image (after session exists so we can clean up on failure)
    if custom_image.is_some() {
        if let Err(e) = session.validate_custom_image().await {
            let _ = session.cleanup().await;
            return Err(e);
        }
    }

    let test_id = task_def.id.clone();

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_task_inner(&task_def, &config, &session, &artifacts_dir, debug, verbose, no_recording) => r,
    };

    // Always collect artifacts and clean up
    info!("Collecting artifacts...");
    let _ = artifacts::collect_artifacts(&session, &artifacts_dir).await;

    info!("Cleaning up container...");
    let _ = session.cleanup().await;

    let duration_ms = start.elapsed().as_millis() as u64;

    // Write results.json
    let test_result = match &result {
        Ok(run_result) if !run_result.agent_ran => {
            // Programmatic-only mode: no agent verdict
            results::from_evaluation(
                &test_id,
                run_result.eval_result.as_ref().expect("programmatic mode has eval_result"),
                duration_ms,
            )
        }
        Ok(run_result) => results::from_outcome(
            &test_id,
            &run_result.outcome,
            run_result.eval_result.as_ref(),
            duration_ms,
        ),
        Err(e) => results::from_error(&test_id, e, duration_ms),
    };
    if let Err(e) = results::write_results(&test_result, &output_dir) {
        tracing::warn!("Failed to write results.json: {e}");
    }

    result.map(|r| r.outcome)
}

async fn run_task_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &docker::DockerSession,
    artifacts_dir: &std::path::Path,
    debug: bool,
    verbose: bool,
    no_recording: bool,
) -> Result<TaskRunResult, AppError> {
    use task::EvaluatorMode;

    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    // Determine evaluation mode (default to LLM if no evaluator configured)
    let eval_mode = task_def
        .evaluator
        .as_ref()
        .map(|e| &e.mode)
        .unwrap_or(&EvaluatorMode::Llm);

    info!("Evaluation mode: {}", match eval_mode {
        EvaluatorMode::Llm => "llm",
        EvaluatorMode::Programmatic => "programmatic",
        EvaluatorMode::Hybrid => "hybrid",
    });

    // 1. Wait for desktop to be ready
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 2. Run setup steps from task definition (after desktop readiness, before app launch)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config).await?;
    }

    // 3. Deploy and launch app
    let is_docker_image = matches!(task_def.app, task::AppConfig::DockerImage { .. });

    if is_docker_image {
        // Custom Docker image: no deployment needed, launch via entrypoint_cmd if provided
        info!("Custom Docker image: skipping app deployment");

        if let task::AppConfig::DockerImage { entrypoint_cmd, .. } = &task_def.app {
            if let Some(cmd) = entrypoint_cmd {
                info!("Waiting for stable window baseline...");
                let baseline_windows = readiness::get_stable_window_list(session).await?;

                info!("Launching app via entrypoint_cmd: {cmd}");
                session.exec_detached_with_log(
                    &["bash", "-c", cmd],
                    "/tmp/app.log",
                ).await?;

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                let pgrep_cmd = format!("pgrep -f {} || true", shell_escape::escape(cmd.as_str().into()));
                let ps_check = session.exec(&["bash", "-c", &pgrep_cmd]).await;
                if let Ok(output) = &ps_check {
                    if output.trim().is_empty() {
                        let log = session.exec(&["cat", "/tmp/app.log"]).await.unwrap_or_default();
                        if !log.trim().is_empty() {
                            tracing::warn!("App process died. Log output:\n{log}");
                        } else {
                            tracing::warn!("App process not found and no log output");
                        }
                    }
                }

                info!("Waiting for app window...");
                readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
            } else {
                info!("No entrypoint_cmd specified, assuming app starts automatically in custom image");
            }
        }
    } else {
        // Standard flow: deploy app into container and launch
        info!("Deploying app...");
        let app_path = session.deploy_app(config).await?;

        info!("Waiting for stable window baseline...");
        let baseline_windows = readiness::get_stable_window_list(session).await?;

        let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
        info!("Launching app: {app_path}");
        session.launch_app(&app_path, is_appimage).await?;

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let pgrep_cmd = format!(
            "pgrep -f {} || true",
            shell_escape::escape(app_path.as_str().into())
        );
        let ps_check = session.exec(&["bash", "-c", &pgrep_cmd]).await;
        if let Ok(output) = &ps_check {
            if output.trim().is_empty() {
                let log = session
                    .exec(&["cat", "/tmp/app.log"])
                    .await
                    .unwrap_or_default();
                if !log.trim().is_empty() {
                    tracing::warn!("App process died. Log output:\n{log}");
                } else {
                    tracing::warn!("App process not found and no log output");
                }
            }
        }

        info!("Waiting for app window...");
        readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
    }

    // 6. Print VNC info
    if let Some(port) = session.vnc_host_port().await {
        println!("VNC available at {}:{}", config.vnc_bind_addr, port);
    }

    // 7. Start video recording (after app is ready so we skip the boot/setup filler)
    let recording = if no_recording {
        None
    } else {
        match recording::Recording::start(session, config.display_width, config.display_height).await {
            Ok(rec) => Some(rec),
            Err(e) => {
                tracing::warn!("Failed to start recording: {e}");
                None
            }
        }
    };

    // 8. Run agent loop and/or evaluation based on mode
    let result = match eval_mode {
        EvaluatorMode::Programmatic => {
            // Programmatic mode: skip agent loop, run evaluation directly
            info!("Programmatic mode: skipping agent loop, running evaluation...");

            let evaluator = task_def.evaluator.as_ref().expect(
                "Programmatic mode requires evaluator config (validated at task load time)",
            );
            let eval_result =
                evaluator::run_evaluation(session, evaluator, artifacts_dir).await?;

            print_validation_results(None, Some(&eval_result));

            Ok(TaskRunResult {
                outcome: AgentOutcome {
                    passed: eval_result.passed,
                    reasoning: format_evaluation_reasoning(None, Some(&eval_result)),
                    screenshot_count: 0,
                },
                eval_result: Some(eval_result),
                agent_ran: false,
            })
        }
        EvaluatorMode::Llm => {
            // LLM mode: run agent loop only, use agent verdict
            info!("Starting agent loop v2 (LLM-only evaluation)...");
            let agent_loop_result = run_agent_loop(task_def, config, session, artifacts_dir, debug, verbose, recording.as_ref()).await;

            // Stop recording unconditionally (before propagating any error)
            if let Some(rec) = &recording {
                rec.stop(session).await;
                rec.collect(session, artifacts_dir).await;
            }

            let agent_outcome = agent_loop_result?;

            print_validation_results(Some(&agent_outcome), None);

            Ok(TaskRunResult {
                outcome: agent_outcome,
                eval_result: None,
                agent_ran: true,
            })
        }
        EvaluatorMode::Hybrid => {
            // Hybrid mode: run agent loop AND programmatic evaluation, both must pass
            info!("Starting agent loop v2 (hybrid evaluation)...");
            let agent_loop_result = run_agent_loop(task_def, config, session, artifacts_dir, debug, verbose, recording.as_ref()).await;

            // Stop recording unconditionally (before propagating any error)
            if let Some(rec) = &recording {
                rec.stop(session).await;
                rec.collect(session, artifacts_dir).await;
            }

            let agent_outcome = agent_loop_result?;

            info!("Agent loop complete, running programmatic evaluation...");
            let evaluator = task_def.evaluator.as_ref().expect(
                "Hybrid mode requires evaluator config (validated at task load time)",
            );
            let eval_result =
                evaluator::run_evaluation(session, evaluator, artifacts_dir).await?;

            let both_passed = agent_outcome.passed && eval_result.passed;

            print_validation_results(Some(&agent_outcome), Some(&eval_result));

            Ok(TaskRunResult {
                outcome: AgentOutcome {
                    passed: both_passed,
                    reasoning: format_evaluation_reasoning(Some(&agent_outcome), Some(&eval_result)),
                    screenshot_count: agent_outcome.screenshot_count,
                },
                eval_result: Some(eval_result),
                agent_ran: true,
            })
        }
    };

    // For Programmatic mode (which doesn't go through the agent loop branches above),
    // stop recording here if it was started.
    if matches!(eval_mode, EvaluatorMode::Programmatic) {
        if let Some(rec) = &recording {
            rec.stop(session).await;
            rec.collect(session, artifacts_dir).await;
        }
    }

    result
}

/// Run the v2 agent loop (used by LLM and hybrid modes).
async fn run_agent_loop(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &docker::DockerSession,
    artifacts_dir: &std::path::Path,
    debug: bool,
    verbose: bool,
    recording: Option<&recording::Recording>,
) -> Result<AgentOutcome, AppError> {
    let llm_client = provider::create_provider(
        &config.provider,
        &config.api_key,
        &config.model,
        &config.api_base_url,
    )?;

    let loop_config = agent::loop_v2::AgentLoopV2Config {
        debug,
        verbose,
        ..Default::default()
    };
    let mut agent_loop = agent::loop_v2::AgentLoopV2::new(
        llm_client,
        session,
        artifacts_dir.to_path_buf(),
        &task_def.instruction,
        config.display_width,
        config.display_height,
        loop_config,
        recording,
    );
    agent_loop.run().await
}

/// Run the interactive subcommand: starts container, runs setup, provides dev access.
async fn run_interactive(
    task_def: task::TaskDefinition,
    config: Config,
    debug: bool,
    verbose: bool,
    no_recording: bool,
    output_dir: std::path::PathBuf,
    step: bool,
    validate_only: bool,
) -> Result<AgentOutcome, AppError> {
    if validate_only {
        // --validate-only: skip agent loop, run programmatic evaluation only
        // Force evaluation mode to programmatic, reusing the existing run_task flow
        // but we need to check that the task has an evaluator config
        if task_def.evaluator.is_none() {
            return Err(AppError::Config(
                "interactive --validate-only requires a task with an evaluator config".into(),
            ));
        }

        // Create a modified task with programmatic mode for the evaluator
        let mut task_def = task_def;
        if let Some(ref mut eval) = task_def.evaluator {
            eval.mode = task::EvaluatorMode::Programmatic;
        }

        return run_task(task_def, config, debug, verbose, no_recording, output_dir).await;
    }

    if step {
        // --step: run agent one step at a time, pausing after each
        return run_interactive_step(task_def, config, debug, verbose, no_recording, output_dir).await;
    }

    // Default interactive: start container, run setup, print VNC info, pause
    run_interactive_pause(task_def, config, debug, no_recording).await
}

/// Interactive mode: start container, run setup steps, print VNC info, pause.
async fn run_interactive_pause(
    task_def: task::TaskDefinition,
    mut config: Config,
    debug: bool,
    no_recording: bool,
) -> Result<AgentOutcome, AppError> {
    config.apply_task_app(&task_def.app);
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    // Determine custom Docker image from task definition
    let custom_image = match &task_def.app {
        task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
        _ => None,
    };

    info!("Creating Docker container...");
    let session = docker::DockerSession::create(&config, custom_image).await?;

    if custom_image.is_some() {
        if let Err(e) = session.validate_custom_image().await {
            let _ = session.cleanup().await;
            return Err(e);
        }
    }

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_interactive_pause_inner(&task_def, &config, &session, timeout, debug, no_recording) => r,
    };

    // Always clean up
    info!("Collecting artifacts...");
    let artifacts_dir = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
        .join("artifacts");
    let _ = artifacts::collect_artifacts(&session, &artifacts_dir).await;

    info!("Cleaning up container...");
    let _ = session.cleanup().await;

    result
}

async fn run_interactive_pause_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &docker::DockerSession,
    timeout: Duration,
    debug: bool,
    _no_recording: bool,
) -> Result<AgentOutcome, AppError> {
    // 1. Wait for desktop
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 2. Run setup steps
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config).await?;
    }

    // 3. Deploy and launch app
    let is_docker_image = matches!(task_def.app, task::AppConfig::DockerImage { .. });

    if is_docker_image {
        info!("Custom Docker image: skipping app deployment");
        if let task::AppConfig::DockerImage { entrypoint_cmd, .. } = &task_def.app {
            if let Some(cmd) = entrypoint_cmd {
                info!("Launching app via entrypoint_cmd: {cmd}");
                session.exec_detached_with_log(&["bash", "-c", cmd], "/tmp/app.log").await?;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    } else {
        info!("Deploying app...");
        let app_path = session.deploy_app(config).await?;
        let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
        info!("Launching app: {app_path}");
        session.launch_app(&app_path, is_appimage).await?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // 4. Print VNC info and container info
    if let Some(port) = session.vnc_host_port().await {
        println!("VNC available at {}:{}", config.vnc_bind_addr, port);
    }

    println!("\nInteractive mode: container is running with task '{}'.", task_def.id);
    println!("  Instruction: {}", task_def.instruction);
    println!("  Container ID: {}", session.container_id);
    println!("  docker exec -it {} bash", session.container_id);
    println!("\nPress Ctrl+C to stop and clean up.");

    // Wait forever until Ctrl+C
    std::future::pending::<()>().await;
    unreachable!()
}

/// Interactive step mode: run agent one step at a time, pausing after each.
async fn run_interactive_step(
    task_def: task::TaskDefinition,
    mut config: Config,
    debug: bool,
    verbose: bool,
    no_recording: bool,
    output_dir: std::path::PathBuf,
) -> Result<AgentOutcome, AppError> {
    let start = Instant::now();
    config.apply_task_app(&task_def.app);

    let artifacts_dir = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    let custom_image = match &task_def.app {
        task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
        _ => None,
    };

    info!("Creating Docker container...");
    let session = docker::DockerSession::create(&config, custom_image).await?;

    if custom_image.is_some() {
        if let Err(e) = session.validate_custom_image().await {
            let _ = session.cleanup().await;
            return Err(e);
        }
    }

    let test_id = task_def.id.clone();
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_interactive_step_inner(&task_def, &config, &session, &artifacts_dir, timeout, debug, verbose, no_recording) => r,
    };

    // Collect artifacts and clean up
    info!("Collecting artifacts...");
    let _ = artifacts::collect_artifacts(&session, &artifacts_dir).await;
    info!("Cleaning up container...");
    let _ = session.cleanup().await;

    let duration_ms = start.elapsed().as_millis() as u64;

    // Write results
    let test_result = match &result {
        Ok(run_result) if !run_result.agent_ran => {
            results::from_evaluation(
                &test_id,
                run_result.eval_result.as_ref().expect("eval_result"),
                duration_ms,
            )
        }
        Ok(run_result) => results::from_outcome(
            &test_id,
            &run_result.outcome,
            run_result.eval_result.as_ref(),
            duration_ms,
        ),
        Err(e) => results::from_error(&test_id, e, duration_ms),
    };
    if let Err(e) = results::write_results(&test_result, &output_dir) {
        tracing::warn!("Failed to write results.json: {e}");
    }

    result.map(|r| r.outcome)
}

async fn run_interactive_step_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &docker::DockerSession,
    artifacts_dir: &std::path::Path,
    timeout: Duration,
    debug: bool,
    verbose: bool,
    no_recording: bool,
) -> Result<TaskRunResult, AppError> {
    use task::EvaluatorMode;

    // 1. Wait for desktop
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 2. Run setup steps
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config).await?;
    }

    // 3. Deploy and launch app (same as run_task_inner)
    let is_docker_image = matches!(task_def.app, task::AppConfig::DockerImage { .. });
    if is_docker_image {
        info!("Custom Docker image: skipping app deployment");
        if let task::AppConfig::DockerImage { entrypoint_cmd, .. } = &task_def.app {
            if let Some(cmd) = entrypoint_cmd {
                let baseline_windows = readiness::get_stable_window_list(session).await?;
                info!("Launching app via entrypoint_cmd: {cmd}");
                session.exec_detached_with_log(&["bash", "-c", cmd], "/tmp/app.log").await?;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                info!("Waiting for app window...");
                readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
            }
        }
    } else {
        info!("Deploying app...");
        let app_path = session.deploy_app(config).await?;
        let baseline_windows = readiness::get_stable_window_list(session).await?;
        let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
        info!("Launching app: {app_path}");
        session.launch_app(&app_path, is_appimage).await?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        info!("Waiting for app window...");
        readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
    }

    // Print VNC info
    if let Some(port) = session.vnc_host_port().await {
        println!("VNC available at {}:{}", config.vnc_bind_addr, port);
    }

    // 4. Start video recording (after app is ready so we skip the boot/setup filler)
    let recording = if no_recording {
        None
    } else {
        match recording::Recording::start(session, config.display_width, config.display_height).await {
            Ok(rec) => Some(rec),
            Err(e) => {
                tracing::warn!("Failed to start recording: {e}");
                None
            }
        }
    };

    // 5. Run agent loop in step mode
    info!("Starting agent loop v2 in step mode...");
    let llm_client = provider::create_provider(
        &config.provider,
        &config.api_key,
        &config.model,
        &config.api_base_url,
    )?;

    let loop_config = agent::loop_v2::AgentLoopV2Config {
        debug,
        verbose,
        ..Default::default()
    };
    let mut agent_loop = agent::loop_v2::AgentLoopV2::new(
        llm_client,
        session,
        artifacts_dir.to_path_buf(),
        &task_def.instruction,
        config.display_width,
        config.display_height,
        loop_config,
        recording.as_ref(),
    );

    let agent_loop_result = agent_loop.run_step_by_step().await;

    // Stop recording unconditionally (before propagating any error)
    if let Some(rec) = &recording {
        rec.stop(session).await;
        rec.collect(session, artifacts_dir).await;
    }

    let agent_outcome = agent_loop_result?;

    // 6. Run evaluation if needed
    let eval_mode = task_def
        .evaluator
        .as_ref()
        .map(|e| &e.mode)
        .unwrap_or(&EvaluatorMode::Llm);

    let eval_result = if matches!(eval_mode, EvaluatorMode::Hybrid | EvaluatorMode::Programmatic) {
        info!("Running programmatic evaluation...");
        let evaluator = task_def.evaluator.as_ref().expect("evaluator config");
        Some(evaluator::run_evaluation(session, evaluator, artifacts_dir).await?)
    } else {
        None
    };

    let final_passed = match (&eval_result, eval_mode) {
        (Some(eval), EvaluatorMode::Hybrid) => agent_outcome.passed && eval.passed,
        (Some(eval), EvaluatorMode::Programmatic) => eval.passed,
        _ => agent_outcome.passed,
    };

    print_validation_results(Some(&agent_outcome), eval_result.as_ref());

    Ok(TaskRunResult {
        outcome: AgentOutcome {
            passed: final_passed,
            reasoning: format_evaluation_reasoning(Some(&agent_outcome), eval_result.as_ref()),
            screenshot_count: agent_outcome.screenshot_count,
        },
        eval_result,
        agent_ran: true,
    })
}

/// Print validation results showing which sources passed/failed.
fn print_validation_results(
    agent_outcome: Option<&AgentOutcome>,
    eval_result: Option<&evaluator::EvaluationResult>,
) {
    println!("\n=== Validation Results ===");

    if let Some(outcome) = agent_outcome {
        let verdict = if outcome.passed { "PASSED" } else { "FAILED" };
        println!("  Agent verdict: {verdict}");
        println!("    Reasoning: {}", outcome.reasoning);
        println!("    Steps: {}", outcome.screenshot_count);
    }

    if let Some(result) = eval_result {
        let verdict = if result.passed { "PASSED" } else { "FAILED" };
        println!("  Programmatic evaluation: {verdict}");
        for mr in &result.metric_results {
            let status = if mr.passed { "PASS" } else { "FAIL" };
            println!("    [{status}] {}: {}", mr.metric, mr.detail);
        }
    }

    // Combined result
    let final_passed = match (agent_outcome, eval_result) {
        (Some(a), Some(e)) => a.passed && e.passed,  // hybrid
        (Some(a), None) => a.passed,                   // llm
        (None, Some(e)) => e.passed,                   // programmatic
        (None, None) => true,
    };
    let final_verdict = if final_passed { "PASSED" } else { "FAILED" };
    println!("  Final result: {final_verdict}");
    println!("========================\n");
}

/// Format a combined reasoning string from agent and evaluation results.
fn format_evaluation_reasoning(
    agent_outcome: Option<&AgentOutcome>,
    eval_result: Option<&evaluator::EvaluationResult>,
) -> String {
    let mut parts = Vec::new();

    if let Some(outcome) = agent_outcome {
        let verdict = if outcome.passed { "passed" } else { "failed" };
        parts.push(format!("Agent {verdict}: {}", outcome.reasoning));
    }

    if let Some(result) = eval_result {
        let total = result.metric_results.len();
        let passed = result.metric_results.iter().filter(|m| m.passed).count();
        let failed = total - passed;
        if result.passed {
            parts.push(format!("Programmatic evaluation passed ({passed}/{total} metrics)"));
        } else {
            let failures: Vec<String> = result
                .metric_results
                .iter()
                .filter(|m| !m.passed)
                .map(|m| format!("{}: {}", m.metric, m.detail))
                .collect();
            parts.push(format!(
                "Programmatic evaluation failed ({failed}/{total} metrics failed: {})",
                failures.join("; ")
            ));
        }
    }

    parts.join(". ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use evaluator::{EvaluationResult, MetricResult};

    fn make_agent_outcome(passed: bool, reasoning: &str) -> AgentOutcome {
        AgentOutcome {
            passed,
            reasoning: reasoning.into(),
            screenshot_count: 5,
        }
    }

    fn make_eval_result(passed: bool, metrics: Vec<MetricResult>) -> EvaluationResult {
        EvaluationResult {
            passed,
            mode: if passed { "programmatic" } else { "programmatic" }.into(),
            metric_results: metrics,
        }
    }

    fn make_metric(passed: bool, name: &str, detail: &str) -> MetricResult {
        MetricResult {
            passed,
            metric: name.into(),
            expected: "expected".into(),
            actual: "actual".into(),
            detail: detail.into(),
        }
    }

    // --- format_evaluation_reasoning tests ---

    #[test]
    fn test_format_reasoning_agent_only_passed() {
        let outcome = make_agent_outcome(true, "Task completed");
        let result = format_evaluation_reasoning(Some(&outcome), None);
        assert!(result.contains("Agent passed"));
        assert!(result.contains("Task completed"));
    }

    #[test]
    fn test_format_reasoning_agent_only_failed() {
        let outcome = make_agent_outcome(false, "Could not find button");
        let result = format_evaluation_reasoning(Some(&outcome), None);
        assert!(result.contains("Agent failed"));
        assert!(result.contains("Could not find button"));
    }

    #[test]
    fn test_format_reasoning_eval_only_passed() {
        let metrics = vec![
            make_metric(true, "file_exists", "File exists"),
            make_metric(true, "command_output", "Output matches"),
        ];
        let eval = make_eval_result(true, metrics);
        let result = format_evaluation_reasoning(None, Some(&eval));
        assert!(result.contains("Programmatic evaluation passed"));
        assert!(result.contains("2/2 metrics"));
    }

    #[test]
    fn test_format_reasoning_eval_only_failed() {
        let metrics = vec![
            make_metric(true, "file_exists", "File exists"),
            make_metric(false, "command_output", "Output mismatch"),
        ];
        let eval = make_eval_result(false, metrics);
        let result = format_evaluation_reasoning(None, Some(&eval));
        assert!(result.contains("Programmatic evaluation failed"));
        assert!(result.contains("1/2 metrics failed"));
        assert!(result.contains("command_output: Output mismatch"));
    }

    #[test]
    fn test_format_reasoning_hybrid_both_passed() {
        let outcome = make_agent_outcome(true, "Done");
        let metrics = vec![make_metric(true, "file_exists", "File exists")];
        let eval = make_eval_result(true, metrics);
        let result = format_evaluation_reasoning(Some(&outcome), Some(&eval));
        assert!(result.contains("Agent passed"));
        assert!(result.contains("Programmatic evaluation passed"));
    }

    #[test]
    fn test_format_reasoning_hybrid_agent_passed_eval_failed() {
        let outcome = make_agent_outcome(true, "Done");
        let metrics = vec![make_metric(false, "file_compare", "Files differ")];
        let eval = make_eval_result(false, metrics);
        let result = format_evaluation_reasoning(Some(&outcome), Some(&eval));
        assert!(result.contains("Agent passed"));
        assert!(result.contains("Programmatic evaluation failed"));
    }

    #[test]
    fn test_format_reasoning_hybrid_agent_failed_eval_passed() {
        let outcome = make_agent_outcome(false, "Timed out");
        let metrics = vec![make_metric(true, "file_exists", "File exists")];
        let eval = make_eval_result(true, metrics);
        let result = format_evaluation_reasoning(Some(&outcome), Some(&eval));
        assert!(result.contains("Agent failed"));
        assert!(result.contains("Programmatic evaluation passed"));
    }

    #[test]
    fn test_format_reasoning_no_sources() {
        let result = format_evaluation_reasoning(None, None);
        assert!(result.is_empty());
    }

    // --- print_validation_results does not panic ---

    #[test]
    fn test_print_validation_agent_only() {
        let outcome = make_agent_outcome(true, "Done");
        // Should not panic
        print_validation_results(Some(&outcome), None);
    }

    #[test]
    fn test_print_validation_eval_only() {
        let metrics = vec![make_metric(true, "file_exists", "OK")];
        let eval = make_eval_result(true, metrics);
        print_validation_results(None, Some(&eval));
    }

    #[test]
    fn test_print_validation_hybrid() {
        let outcome = make_agent_outcome(false, "Failed");
        let metrics = vec![
            make_metric(true, "file_exists", "OK"),
            make_metric(false, "command_output", "Mismatch"),
        ];
        let eval = make_eval_result(false, metrics);
        print_validation_results(Some(&outcome), Some(&eval));
    }

    #[test]
    fn test_print_validation_none() {
        print_validation_results(None, None);
    }

    // --- Evaluation mode detection from task ---

    #[test]
    fn test_task_no_evaluator_defaults_to_llm() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        let mode = task
            .evaluator
            .as_ref()
            .map(|e| &e.mode)
            .unwrap_or(&task::EvaluatorMode::Llm);
        assert_eq!(*mode, task::EvaluatorMode::Llm);
    }

    #[test]
    fn test_task_hybrid_mode_detected() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "hybrid",
                "metrics": [{"type": "file_exists", "path": "/tmp/out"}]
            }
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        let mode = &task.evaluator.as_ref().unwrap().mode;
        assert_eq!(*mode, task::EvaluatorMode::Hybrid);
    }

    #[test]
    fn test_task_programmatic_mode_detected() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "metrics": [{"type": "file_exists", "path": "/tmp/out"}]
            }
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        let mode = &task.evaluator.as_ref().unwrap().mode;
        assert_eq!(*mode, task::EvaluatorMode::Programmatic);
    }

    #[test]
    fn test_format_reasoning_eval_all_failed() {
        let metrics = vec![
            make_metric(false, "file_exists", "File not found"),
            make_metric(false, "exit_code", "Exit code 1"),
        ];
        let eval = make_eval_result(false, metrics);
        let result = format_evaluation_reasoning(None, Some(&eval));
        assert!(result.contains("2/2 metrics failed"));
        assert!(result.contains("file_exists"));
        assert!(result.contains("exit_code"));
    }

    #[test]
    fn test_format_reasoning_eval_empty_metrics_passed() {
        let eval = make_eval_result(true, vec![]);
        let result = format_evaluation_reasoning(None, Some(&eval));
        assert!(result.contains("Programmatic evaluation passed (0/0 metrics)"));
    }

    // --- Custom Docker image extraction from task ---

    #[test]
    fn test_custom_image_extracted_from_docker_image_task() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "docker_image", "image": "my-app:latest", "entrypoint_cmd": "myapp"}
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        let custom_image = match &task.app {
            task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
            _ => None,
        };
        assert_eq!(custom_image, Some("my-app:latest"));
    }

    #[test]
    fn test_no_custom_image_for_appimage_task() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        let custom_image = match &task.app {
            task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
            _ => None,
        };
        assert!(custom_image.is_none());
    }

    #[test]
    fn test_no_custom_image_for_folder_task() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "folder", "dir": "/apps/myapp", "entrypoint": "run.sh"}
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        let custom_image = match &task.app {
            task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
            _ => None,
        };
        assert!(custom_image.is_none());
    }

    #[test]
    fn test_docker_image_without_entrypoint_cmd() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "docker_image", "image": "my-app:latest"}
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        match &task.app {
            task::AppConfig::DockerImage { image, entrypoint_cmd } => {
                assert_eq!(image, "my-app:latest");
                assert!(entrypoint_cmd.is_none());
            }
            _ => panic!("Expected DockerImage"),
        }
    }

    #[test]
    fn test_docker_image_is_detected_correctly() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "docker_image", "image": "ubuntu:22.04"}
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        let is_docker_image = matches!(task.app, task::AppConfig::DockerImage { .. });
        assert!(is_docker_image);
    }

    // --- CLI subcommand tests ---

    #[test]
    fn test_load_config_or_defaults_none() {
        let config = load_config_or_defaults(&None);
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_interactive_validate_only_requires_evaluator() {
        // A task without evaluator should fail validate-only
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        assert!(task.evaluator.is_none());
    }

    #[test]
    fn test_interactive_validate_only_with_evaluator() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "hybrid",
                "metrics": [{"type": "file_exists", "path": "/tmp/out"}]
            }
        }"#;
        let task = task::TaskDefinition::parse_and_validate(json).unwrap();
        assert!(task.evaluator.is_some());
        // Verify mode can be changed to programmatic
        let mut task_mut = task;
        if let Some(ref mut eval) = task_mut.evaluator {
            eval.mode = task::EvaluatorMode::Programmatic;
        }
        assert_eq!(task_mut.evaluator.as_ref().unwrap().mode, task::EvaluatorMode::Programmatic);
    }
}
