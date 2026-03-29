use clap::{Parser, Subcommand};

use crate::results;

#[derive(Parser, Debug)]
#[command(
    name = "desktest",
    about = "Automated end-to-end testing for Linux desktop apps using LLM-powered agents",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("DESKTEST_GIT_SHA"), ")"),
    after_help = "\
WORKFLOWS:
  Test authoring (explore → codify → CI):
    desktest run task.json --monitor          # 1. Watch the agent explore your app
    desktest review desktest_artifacts/       # 2. Inspect the trajectory in a browser
    desktest codify trajectory.jsonl --overwrite task.json  # 3. Convert + update task JSON
    desktest run task.json --replay           # 4. Deterministic replay (no LLM, no API costs)

  Live monitoring + agent-assisted debugging:
    desktest run task.json --monitor          # 1. Watch live, spot the failure
    desktest logs desktest_artifacts/         # 2. Hand off to your coding agent
                                              #    e.g. \"Claude, look at desktest logs and diagnose\"

EXAMPLES:
  desktest run task.json --config config.json --artifacts-dir ./artifacts
  desktest run task.json --monitor --with-bash
  desktest suite ./tests --filter gedit
  desktest interactive task.json
  desktest validate task.json"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Path to config JSON file (API key, provider, display settings)
    #[arg(long = "config", global = true)]
    pub config_flag: Option<std::path::PathBuf>,

    /// Output directory for test result JSON files (default: ./test-results/)
    #[arg(long, global = true, default_value = results::DEFAULT_OUTPUT_DIR)]
    pub output: std::path::PathBuf,

    /// Enable debug mode (verbose logging)
    #[arg(long, default_value_t = false, global = true)]
    pub debug: bool,

    /// Enable verbose trajectory logging (includes full LLM responses in trajectory.jsonl)
    #[arg(long, default_value_t = false, global = true)]
    pub verbose: bool,

    /// Enable video recording of test sessions
    #[arg(long, default_value_t = false, global = true)]
    pub record: bool,

    /// Display resolution as WxH (e.g., 1280x720, 1920x1080) or preset (720p, 1080p)
    #[arg(long, global = true)]
    pub resolution: Option<String>,

    /// Enable live monitoring web dashboard (open http://localhost:7860 to watch)
    #[arg(long, default_value_t = false, global = true)]
    pub monitor: bool,

    /// Port for the live monitoring dashboard
    #[arg(long, default_value_t = 7860, global = true)]
    pub monitor_port: u16,

    /// Bind address for the monitoring dashboard (default: 127.0.0.1, use 0.0.0.0 for remote access)
    #[arg(long, default_value = "127.0.0.1", global = true,
          value_parser = |s: &str| s.parse::<std::net::IpAddr>().map(|a| a.to_string()))]
    pub monitor_bind_addr: String,

    /// Directory for trajectory logs, screenshots, and accessibility tree snapshots (default: ./desktest_artifacts/)
    #[arg(long, global = true)]
    pub artifacts_dir: Option<std::path::PathBuf>,

    /// Allow the agent to run bash commands inside the container for debugging (disabled by default — the agent can "cheat" by using bash instead of the GUI)
    #[arg(long = "with-bash", default_value_t = false, global = true)]
    pub with_bash: bool,

    /// Enable QA mode: agent reports app bugs it encounters during testing
    #[arg(long, default_value_t = false, global = true)]
    pub qa: bool,

    /// LLM provider (overrides config file)
    #[arg(long, global = true, value_parser = clap::builder::PossibleValuesParser::new([
        "anthropic", "openai", "openrouter", "cerebras", "gemini", "claude-cli", "codex-cli", "custom"
    ]))]
    pub provider: Option<String>,

    /// LLM model name (overrides config file)
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// API key for the LLM provider (overrides config file and env vars).
    /// Note: prefer env vars (ANTHROPIC_API_KEY, OPENROUTER_API_KEY, etc.) to avoid exposing secrets in shell history and process listings
    #[arg(long, global = true)]
    pub api_key: Option<String>,

    /// Timeout in seconds for artifact collection (default: 120, 0 = no limit). If collection exceeds this, a warning is logged and the process exits with the evaluation result.
    #[arg(long, global = true, default_value_t = 120)]
    pub artifacts_timeout: u64,

    /// Skip artifact collection entirely (exit immediately after evaluation)
    #[arg(long, default_value_t = false, global = true)]
    pub no_artifacts: bool,

    /// Glob patterns to exclude from home directory artifact collection (repeatable).
    /// Defaults: node_modules, .cache, .npm, .electron, .nvm, GPU Cache, GPUCache, ShaderCache.
    /// Use --artifacts-exclude=none to disable all default excludes.
    #[arg(long, global = true)]
    pub artifacts_exclude: Vec<String>,

    /// Maximum number of retry attempts for retryable LLM API failures
    #[arg(long, global = true)]
    pub llm_max_retries: Option<usize>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Validate a task JSON file against the schema without running anything
    #[command(after_help = "\
EXAMPLES:
  desktest validate task.json
  desktest validate tests/gedit-save.json")]
    Validate {
        /// Path to the task JSON file to validate
        task: std::path::PathBuf,
    },

    /// Run a single test from a task JSON file
    #[command(after_help = "\
EXAMPLES:
  desktest run task.json                          # Basic run
  desktest run task.json --monitor                # Watch live at http://localhost:7860
  desktest run task.json --monitor --with-bash    # Live + let agent use bash for debugging
  desktest run task.json --config config.json     # Custom config
  desktest run task.json --record --verbose       # Record video + full LLM logs
  desktest run task.json --resolution 1280x720    # Custom resolution")]
    Run {
        /// Path to the task JSON file
        task: std::path::PathBuf,

        /// Use the replay_script from the task JSON for deterministic execution (no LLM, no API costs)
        #[arg(long, default_value_t = false)]
        replay: bool,
    },

    /// Run a suite of tests from a directory of task JSON files
    #[command(after_help = "\
EXAMPLES:
  desktest suite ./tests
  desktest suite ./tests --filter gedit
  desktest suite ./tests --config config.json --output ./results")]
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
  desktest interactive task.json                   # Start container, run setup, pause
  desktest interactive task.json --step            # Run agent one step at a time
  desktest interactive task.json --validate-only   # Skip agent, run evaluation only
  desktest interactive task.json --config c.json   # Use custom config")]
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

    /// Convert a trajectory into a deterministic Python replay script
    #[command(after_help = "\
EXAMPLES:
  desktest codify desktest_artifacts/trajectory.jsonl
  desktest codify desktest_artifacts/trajectory.jsonl --output desktest_replay.py
  desktest codify desktest_artifacts/trajectory.jsonl --steps 1,2,5,6
  desktest codify desktest_artifacts/trajectory.jsonl --with-screenshots --threshold 0.95")]
    Codify {
        /// Path to trajectory.jsonl file
        trajectory: std::path::PathBuf,

        /// Output Python script path (default: desktest_replay.py)
        #[arg(long, default_value = "desktest_replay.py")]
        output: std::path::PathBuf,

        /// Path to a task JSON file to update with the replay_script path
        #[arg(long)]
        overwrite: Option<std::path::PathBuf>,

        /// Only include these step numbers (comma-separated, 1-indexed)
        #[arg(long)]
        steps: Option<String>,

        /// Add screenshot comparison assertions
        #[arg(long, default_value_t = false)]
        with_screenshots: bool,

        /// Pixel similarity threshold for screenshot comparison (MAE-based, 0.0-1.0)
        #[arg(long, default_value_t = 0.95)]
        threshold: f64,

        /// Delay in seconds between replay steps
        #[arg(long, default_value_t = 0.5)]
        delay: f64,
    },

    /// Attach to an existing running container and run a task against it
    #[command(after_help = "\
PREREQUISITES:
  Docker daemon must be accessible. Desktest uses the Docker API (via the \
socket) to exec into the target container. The user running desktest needs \
Docker socket permissions — typically membership in the `docker` group, or \
`sudo chmod 660 /var/run/docker.sock && sudo chown root:docker /var/run/docker.sock` \
for temporary local dev access.\n\n\
EXAMPLES:
  desktest attach task.json --container my-container
  desktest attach task.json --container abc123 --config config.json
  desktest attach task.json --container my-container --resolution 1280x720
  desktest attach task.json --container my-container --replay")]
    Attach {
        /// Path to the task JSON file
        task: std::path::PathBuf,

        /// Docker container ID or name to attach to (must be running)
        #[arg(long)]
        container: String,

        /// Use the replay_script from the task JSON for deterministic execution (no LLM, no API costs)
        #[arg(long, default_value_t = false)]
        replay: bool,
    },

    /// Replay a codified Python script inside a container (deprecated: use `desktest run --replay` instead)
    #[command(after_help = "\
DEPRECATED: Prefer setting 'replay_script' in your task JSON and using `desktest run --replay`\n\
for deterministic execution with no LLM and zero API costs.\n\n\
EXAMPLES:
  desktest replay task.json --script desktest_replay.py
  desktest replay task.json --script desktest_replay.py --screenshots-dir desktest_artifacts/")]
    Replay {
        /// Path to the task JSON file (for container/app/setup config)
        task: std::path::PathBuf,

        /// Path to the Python replay script
        #[arg(long)]
        script: std::path::PathBuf,

        /// Optional directory containing expected screenshots for visual assertions
        #[arg(long)]
        screenshots_dir: Option<std::path::PathBuf>,
    },

    /// View trajectory logs in the terminal (machine-readable, suitable for piping and agent consumption)
    #[command(after_help = "\
Prints trajectory steps to stdout as structured text. Designed for CLI \
and agent workflows — pipe to grep, jq, or other tools.\n\
For an interactive visual viewer, use `desktest review` instead.\n\n\
EXAMPLES:
  desktest logs desktest_artifacts/
  desktest logs desktest_artifacts/ --brief
  desktest logs desktest_artifacts/ --step 3
  desktest logs desktest_artifacts/ --steps 3-7
  desktest logs desktest_artifacts/ --steps 1,3,5-8")]
    Logs {
        /// Path to artifacts directory containing trajectory.jsonl
        artifacts_dir: std::path::PathBuf,

        /// Show compact summary table only
        #[arg(long, default_value_t = false)]
        brief: bool,

        /// Show only a specific step number
        #[arg(long, conflicts_with = "steps")]
        step: Option<usize>,

        /// Show specific step numbers and ranges (comma-separated, e.g. "1,3,5-8")
        #[arg(long, conflicts_with = "step")]
        steps: Option<String>,
    },

    /// Check that all prerequisites are installed and configured
    #[command(after_help = "\
Verifies Docker daemon connectivity, API key availability, and displays \
current configuration. Use this to troubleshoot setup issues.\n\n\
EXAMPLES:
  desktest doctor
  desktest doctor --config config.json")]
    Doctor,

    /// Update desktest to the latest release from GitHub
    #[command(after_help = "\
EXAMPLES:
  desktest update                # Update to latest if newer
  desktest update --force        # Re-download even if already up to date")]
    Update {
        /// Force update even if already on the latest version
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Start a persistent monitor server that watches artifact directories for multi-phase runs
    #[command(after_help = "\
Watches an artifacts directory tree for trajectory files from multiple \
desktest attach/run phases. Each subdirectory with a trajectory.jsonl \
is treated as a separate phase, displayed in a single timeline.\n\n\
EXAMPLES:
  desktest monitor --watch ./artifacts/
  desktest monitor --watch ./artifacts/ --monitor-port 8080")]
    Monitor {
        /// Directory tree to watch for phase subdirectories
        #[arg(long)]
        watch: std::path::PathBuf,
    },

    /// Prepare a macOS golden image for Tart-based testing
    #[command(after_help = "\
Pulls a Tart base image, clones it, installs the desktest VM agent, \
accessibility helper, Python, PyAutoGUI, and saves the result as \
'desktest-macos:latest'. Requires Apple Silicon Mac with Tart installed.\n\n\
EXAMPLES:
  desktest init-macos
  desktest init-macos --base-image ghcr.io/cirruslabs/macos-sequoia-base:latest
  desktest init-macos --with-electron
  desktest init-macos --output-image desktest-macos:custom")]
    InitMacos {
        /// Base Tart image to start from (default: ghcr.io/cirruslabs/macos-sequoia-base:latest)
        #[arg(long, default_value = "ghcr.io/cirruslabs/macos-sequoia-base:latest")]
        base_image: String,

        /// Name for the output golden image
        #[arg(long, default_value = "desktest-macos:latest")]
        output_image: String,

        /// Also install Node.js for Electron app testing
        #[arg(long, default_value_t = false)]
        with_electron: bool,
    },

    /// Generate an interactive HTML trajectory viewer (best for human review in a browser)
    #[command(after_help = "\
For a CLI/agent-friendly text view, use `desktest logs` instead.\n\n\
EXAMPLES:
  desktest review desktest_artifacts/
  desktest review desktest_artifacts/ --output desktest_review.html
  desktest review desktest_artifacts/ --no-open")]
    Review {
        /// Path to artifacts directory containing trajectory.jsonl
        artifacts_dir: std::path::PathBuf,

        /// Output HTML file path (default: desktest_review.html)
        #[arg(long, default_value = "desktest_review.html")]
        output: std::path::PathBuf,

        /// Do not open the generated HTML file in the default browser
        #[arg(long, default_value_t = false)]
        no_open: bool,
    },
}
