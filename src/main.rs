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
#[command(name = "tent", about = "LLM-powered desktop app tester")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Path to the JSON config file (legacy positional arg)
    pub config: Option<std::path::PathBuf>,

    /// Path to the instructions Markdown file (legacy positional arg)
    pub instructions: Option<std::path::PathBuf>,

    /// Enable debug mode (verbose logging)
    #[arg(long, default_value_t = false, global = true)]
    pub debug: bool,

    /// Enable verbose trajectory logging (includes full LLM responses in trajectory.jsonl)
    #[arg(long, default_value_t = false, global = true)]
    pub verbose: bool,

    /// Disable video recording of test sessions
    #[arg(long, default_value_t = false, global = true)]
    pub no_recording: bool,

    /// Interactive mode: start container and app, then wait for Ctrl+C (no agent)
    #[arg(long, default_value_t = false)]
    pub interactive: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Validate a task JSON file against the schema without running anything
    Validate {
        /// Path to the task JSON file to validate
        task: std::path::PathBuf,
    },

    /// Run a single test from a task JSON file
    Run {
        /// Path to the task JSON file
        task: std::path::PathBuf,

        /// Path to an optional config JSON file (for API key, provider, display settings)
        #[arg(long)]
        config: Option<std::path::PathBuf>,

        /// Output directory for results.json (default: ./test-results/)
        #[arg(long, default_value = results::DEFAULT_OUTPUT_DIR)]
        output: std::path::PathBuf,
    },

    /// Run a suite of tests from a directory of task JSON files
    Suite {
        /// Path to the directory containing task JSON files
        dir: std::path::PathBuf,

        /// Path to an optional config JSON file (for API key, provider, display settings)
        #[arg(long)]
        config: Option<std::path::PathBuf>,

        /// Output directory for suite-results.json and per-test results (default: ./test-results/)
        #[arg(long, default_value = results::DEFAULT_OUTPUT_DIR)]
        output: std::path::PathBuf,

        /// Run only tests matching this name pattern
        #[arg(long)]
        filter: Option<String>,
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
            Command::Run { task, config, output } => {
                let task_def = match task::TaskDefinition::load(task) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Task load error: {e}");
                        std::process::exit(e.exit_code());
                    }
                };

                let run_config = if let Some(config_path) = config {
                    match config::Config::load_and_validate(config_path) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Config error: {e}");
                            std::process::exit(e.exit_code());
                        }
                    }
                } else {
                    // Build a minimal config from task definition + env vars
                    config::Config::from_task_defaults()
                };

                let result = run_task(task_def, run_config, cli.debug, cli.verbose, cli.no_recording, output.clone()).await;
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
            Command::Suite { dir, config, output, filter } => {
                let result = suite::run_suite(
                    dir,
                    config.as_deref(),
                    filter.as_deref(),
                    output,
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
            if interactive {
                std::process::exit(0);
            }
            eprintln!("Error: {e}");
            std::process::exit(e.exit_code());
        }
    }
}

async fn run_legacy(cli: Cli) -> Result<AgentOutcome, AppError> {
    // 1. Validate config
    let config_path = cli.config.ok_or_else(|| {
        AppError::Config("Missing config file argument. Usage: tent <config.json> <instructions.md>".into())
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
    if let Some(vnc_port) = config.vnc_port {
        println!("VNC available at {}:{}", config.vnc_bind_addr, vnc_port);
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
    config: Config,
    debug: bool,
    verbose: bool,
    no_recording: bool,
    output_dir: std::path::PathBuf,
) -> Result<AgentOutcome, AppError> {
    let start = Instant::now();

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

    // Create and start Docker container
    info!("Creating Docker container...");
    let session = docker::DockerSession::create(&config, custom_image).await?;

    // Validate custom image has required dependencies
    if custom_image.is_some() {
        session.validate_custom_image().await?;
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

    // 3. Start video recording (before app launch so we capture the full session)
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

    // 4. Deploy and launch app
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
    if let Some(vnc_port) = config.vnc_port {
        println!("VNC available at {}:{}", config.vnc_bind_addr, vnc_port);
    }

    // 7. Run agent loop and/or evaluation based on mode
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
            let agent_outcome = run_agent_loop(task_def, config, session, artifacts_dir, debug, verbose).await?;

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
            let agent_outcome = run_agent_loop(task_def, config, session, artifacts_dir, debug, verbose).await?;

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

    // 8. Stop recording and collect the video file (regardless of test outcome)
    if let Some(rec) = &recording {
        rec.stop(session).await;
        rec.collect(session, artifacts_dir).await;
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
    );
    agent_loop.run().await
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
}
