mod agent;
mod artifacts;
mod config;
mod docker;
mod error;
mod input;
mod observation;
mod provider;
mod readiness;
mod screenshot;
mod setup;
mod task;

use std::time::Duration;

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
            Command::Run { task, config } => {
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

                let result = run_task(task_def, run_config, cli.debug).await;
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
    let session = docker::DockerSession::create(&config).await?;

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

/// Run a test from a task definition file.
///
/// This is the new task-based flow: load task → create container → wait for desktop →
/// run setup steps → deploy & launch app → run agent loop → cleanup.
async fn run_task(
    task_def: task::TaskDefinition,
    config: Config,
    debug: bool,
) -> Result<AgentOutcome, AppError> {
    // Set up artifacts directory
    let artifacts_dir = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    // Create and start Docker container
    info!("Creating Docker container...");
    let session = docker::DockerSession::create(&config).await?;

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_task_inner(&task_def, &config, &session, &artifacts_dir, debug) => r,
    };

    // Always collect artifacts and clean up
    info!("Collecting artifacts...");
    let _ = artifacts::collect_artifacts(&session, &artifacts_dir).await;

    info!("Cleaning up container...");
    let _ = session.cleanup().await;

    result
}

async fn run_task_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &docker::DockerSession,
    artifacts_dir: &std::path::Path,
    debug: bool,
) -> Result<AgentOutcome, AppError> {
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    // 1. Wait for desktop to be ready
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 2. Run setup steps from task definition (after desktop readiness, before app launch)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config).await?;
    }

    // 3. Deploy app into container
    info!("Deploying app...");
    let app_path = session.deploy_app(config).await?;

    // 4. Get stable baseline windows, launch app, wait for app window
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

    // 5. Print VNC info
    if let Some(vnc_port) = config.vnc_port {
        println!("VNC available at {}:{}", config.vnc_bind_addr, vnc_port);
    }

    // 6. Run agent loop (v2 — OSWorld-style with PyAutoGUI + observation + sliding window)
    info!("Starting agent loop v2...");
    let llm_client = provider::create_provider(
        &config.provider,
        &config.api_key,
        &config.model,
        &config.api_base_url,
    )?;

    let loop_config = agent::loop_v2::AgentLoopV2Config {
        debug,
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
