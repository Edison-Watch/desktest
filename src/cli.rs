use clap::{
    Parser, Subcommand,
    builder::styling::{AnsiColor, Effects, Style, Styles},
};

use crate::results;

// Brand-aligned clap styles:
//   - Headers (Usage:, Commands:, Options:): cyan + bold + underline
//   - Literals (subcommands, flags): cyan
//   - Placeholders (<VALUE>): graphene grey
//   - Valid values: cyan
const BRAND_STYLES: Styles = Styles::styled()
    .header(
        Style::new()
            .fg_color(Some(clap::builder::styling::Color::Ansi(AnsiColor::Cyan)))
            .effects(Effects::BOLD.insert(Effects::UNDERLINE)),
    )
    .literal(Style::new().fg_color(Some(clap::builder::styling::Color::Ansi(AnsiColor::Cyan))))
    .placeholder(Style::new().effects(Effects::DIMMED))
    .valid(Style::new().fg_color(Some(clap::builder::styling::Color::Ansi(AnsiColor::Cyan))))
    .usage(
        Style::new()
            .fg_color(Some(clap::builder::styling::Color::Ansi(AnsiColor::Cyan)))
            .effects(Effects::BOLD.insert(Effects::UNDERLINE)),
    );

#[derive(Parser, Debug)]
#[command(
    name = "desktest",
    styles = BRAND_STYLES,
    about = "Automated end-to-end testing for Linux desktop apps using LLM-powered agents",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("DESKTEST_GIT_SHA"), ")"),
    after_help = concat!(
        "\x1b[1;4;36mWORKFLOWS:\x1b[0m\n",
        "  Test authoring (explore \u{2192} codify \u{2192} CI):\n",
        "    \x1b[36m\u{25b8}\x1b[0m desktest run task.json --monitor          \x1b[2m# 1. Watch the agent explore your app\x1b[0m\n",
        "    \x1b[36m\u{25b8}\x1b[0m desktest review desktest_artifacts/       \x1b[2m# 2. Inspect the trajectory in a browser\x1b[0m\n",
        "    \x1b[36m\u{25b8}\x1b[0m desktest codify trajectory.jsonl --overwrite task.json  \x1b[2m# 3. Convert + update task JSON\x1b[0m\n",
        "    \x1b[36m\u{25b8}\x1b[0m desktest run task.json --replay           \x1b[2m# 4. Deterministic replay (no LLM, no API costs)\x1b[0m\n",
        "\n",
        "  Live monitoring + agent-assisted debugging:\n",
        "    \x1b[36m\u{25b8}\x1b[0m desktest run task.json --monitor          \x1b[2m# 1. Watch live, spot the failure\x1b[0m\n",
        "    \x1b[36m\u{25b8}\x1b[0m desktest logs desktest_artifacts/         \x1b[2m# 2. Hand off to your coding agent\x1b[0m\n",
        "                                              \x1b[2m#    e.g. \"Claude, look at desktest logs and diagnose\"\x1b[0m\n",
        "\n",
        "\x1b[1;4;36mEXAMPLES:\x1b[0m\n",
        "  desktest run task.json --config config.json --artifacts-dir ./artifacts\n",
        "  desktest run task.json --monitor --with-bash\n",
        "  desktest suite ./tests --filter gedit\n",
        "  desktest interactive task.json\n",
        "  desktest validate task.json"
    )
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
\x1b[1;4;36mEXAMPLES:\x1b[0m
  desktest validate task.json
  desktest validate tests/gedit-save.json")]
    Validate {
        /// Path to the task JSON file to validate
        task: std::path::PathBuf,
    },

    /// Run a single test from a task JSON file
    #[command(after_help = "\
\x1b[1;4;36mEXAMPLES:\x1b[0m
  desktest run task.json                          \x1b[2m# Basic run\x1b[0m
  desktest run task.json --monitor                \x1b[2m# Watch live at http://localhost:7860\x1b[0m
  desktest run task.json --monitor --with-bash    \x1b[2m# Live + let agent use bash for debugging\x1b[0m
  desktest run task.json --config config.json     \x1b[2m# Custom config\x1b[0m
  desktest run task.json --record --verbose       \x1b[2m# Record video + full LLM logs\x1b[0m
  desktest run task.json --resolution 1280x720    \x1b[2m# Custom resolution\x1b[0m")]
    Run {
        /// Path to the task JSON file
        task: std::path::PathBuf,

        /// Use the replay_script from the task JSON for deterministic execution (no LLM, no API costs)
        #[arg(long, default_value_t = false)]
        replay: bool,
    },

    /// Run a suite of tests from a directory of task JSON files
    #[command(after_help = "\
\x1b[1;4;36mEXAMPLES:\x1b[0m
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
\x1b[1;4;36mEXAMPLES:\x1b[0m
  desktest interactive task.json                   \x1b[2m# Start container, run setup, pause\x1b[0m
  desktest interactive task.json --step            \x1b[2m# Run agent one step at a time\x1b[0m
  desktest interactive task.json --validate-only   \x1b[2m# Skip agent, run evaluation only\x1b[0m
  desktest interactive task.json --config c.json   \x1b[2m# Use custom config\x1b[0m")]
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
\x1b[1;4;36mEXAMPLES:\x1b[0m
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
    #[command(after_help = concat!(
        "\x1b[1;4;36mPREREQUISITES:\x1b[0m\n",
        "  Docker daemon must be accessible. Desktest uses the Docker API (via the ",
        "socket) to exec into the target container. The user running desktest needs ",
        "Docker socket permissions \u{2014} typically membership in the `docker` group, or ",
        "`sudo chmod 660 /var/run/docker.sock && sudo chown root:docker /var/run/docker.sock` ",
        "for temporary local dev access.\n\n",
        "\x1b[1;4;36mEXAMPLES:\x1b[0m\n",
        "  desktest attach task.json --container my-container\n",
        "  desktest attach task.json --container abc123 --config config.json\n",
        "  desktest attach task.json --container my-container --resolution 1280x720\n",
        "  desktest attach task.json --container my-container --replay"
    ))]
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
    #[command(after_help = concat!(
        "\x1b[1;4;36mDEPRECATED:\x1b[0m Prefer setting 'replay_script' in your task JSON and using `desktest run --replay`\n",
        "for deterministic execution with no LLM and zero API costs.\n\n",
        "\x1b[1;4;36mEXAMPLES:\x1b[0m\n",
        "  desktest replay task.json --script desktest_replay.py\n",
        "  desktest replay task.json --script desktest_replay.py --screenshots-dir desktest_artifacts/"
    ))]
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
    #[command(after_help = concat!(
        "Prints trajectory steps to stdout as structured text. Designed for CLI ",
        "and agent workflows \u{2014} pipe to grep, jq, or other tools.\n",
        "For an interactive visual viewer, use `desktest review` instead.\n\n",
        "\x1b[1;4;36mEXAMPLES:\x1b[0m\n",
        "  desktest logs desktest_artifacts/\n",
        "  desktest logs desktest_artifacts/ --brief\n",
        "  desktest logs desktest_artifacts/ --step 3\n",
        "  desktest logs desktest_artifacts/ --steps 3-7\n",
        "  desktest logs desktest_artifacts/ --steps 1,3,5-8"
    ))]
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
    #[command(after_help = concat!(
        "Verifies Docker daemon connectivity, API key availability, and displays ",
        "current configuration. Use this to troubleshoot setup issues.\n\n",
        "\x1b[1;4;36mEXAMPLES:\x1b[0m\n",
        "  desktest doctor\n",
        "  desktest doctor --config config.json"
    ))]
    Doctor,

    /// Update desktest to the latest release from GitHub
    #[command(after_help = "\
\x1b[1;4;36mEXAMPLES:\x1b[0m
  desktest update                \x1b[2m# Update to latest if newer\x1b[0m
  desktest update --force        \x1b[2m# Re-download even if already up to date\x1b[0m")]
    Update {
        /// Force update even if already on the latest version
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Start a persistent monitor server that watches artifact directories for multi-phase runs
    #[command(after_help = concat!(
        "Watches an artifacts directory tree for trajectory files from multiple ",
        "desktest attach/run phases. Each subdirectory with a trajectory.jsonl ",
        "is treated as a separate phase, displayed in a single timeline.\n\n",
        "\x1b[1;4;36mEXAMPLES:\x1b[0m\n",
        "  desktest monitor --watch ./artifacts/\n",
        "  desktest monitor --watch ./artifacts/ --monitor-port 8080"
    ))]
    Monitor {
        /// Directory tree to watch for phase subdirectories
        #[arg(long)]
        watch: std::path::PathBuf,
    },

    /// Generate an interactive HTML trajectory viewer (best for human review in a browser)
    #[command(after_help = concat!(
        "For a CLI/agent-friendly text view, use `desktest logs` instead.\n\n",
        "\x1b[1;4;36mEXAMPLES:\x1b[0m\n",
        "  desktest review desktest_artifacts/\n",
        "  desktest review desktest_artifacts/ --output desktest_review.html\n",
        "  desktest review desktest_artifacts/ --no-open"
    ))]
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
