mod agent;
mod artifacts;
mod config;
mod docker;
mod error;
mod input;
mod readiness;
mod screenshot;

use std::time::Duration;

use clap::Parser;
use config::Config;
use error::{AgentOutcome, AppError};
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "llm-desktop-app-tester", about = "LLM-powered desktop app tester")]
pub struct Cli {
    /// Path to the JSON config file
    pub config: std::path::PathBuf,

    /// Path to the instructions Markdown file
    pub instructions: std::path::PathBuf,

    /// Enable debug mode (verbose logging)
    #[arg(long, default_value_t = false)]
    pub debug: bool,
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

    let result = run(cli).await;

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

async fn run(cli: Cli) -> Result<AgentOutcome, AppError> {
    // 1. Validate config
    let config = Config::load_and_validate(&cli.config)?;

    // 2. Read instructions
    let instructions = std::fs::read_to_string(&cli.instructions)
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
        r = run_inner(&config, &session, &artifacts_dir, &instructions, cli.debug) => r,
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
) -> Result<AgentOutcome, AppError> {
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    // 5. Wait for desktop to be ready
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 6. Deploy app into container
    info!("Deploying app...");
    let app_path = session.deploy_app(config).await?;

    // 7. Get baseline windows, launch app, wait for app window
    let baseline_windows = readiness::get_window_list(session).await?;

    info!("Launching app: {app_path}");
    session.launch_app(&app_path).await?;

    info!("Waiting for app window...");
    readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;

    // 8. Print VNC info
    if let Some(vnc_port) = config.vnc_port {
        println!("VNC available at {}:{}", config.vnc_bind_addr, vnc_port);
    }

    // 9. Run agent loop
    info!("Starting agent loop...");
    let client = agent::openai::OpenAiClient::new(&config.openai_api_key, &config.openai_model);
    let mut agent_loop = agent::AgentLoop::new(
        client,
        session,
        artifacts_dir.to_path_buf(),
        instructions.to_string(),
        debug,
    );
    agent_loop.run().await
}
