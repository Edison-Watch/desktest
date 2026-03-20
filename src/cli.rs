use clap::{Parser, Subcommand};

use crate::results;

#[derive(Parser, Debug)]
#[command(
    name = "desktest",
    about = "LLM-powered desktop app tester",
    after_help = "\
EXAMPLES:
  Legacy mode (backward compatible):
    desktest config.json instructions.md
    desktest --interactive config.json instructions.md

  Subcommand mode:
    desktest run task.json
    desktest run task.json --config config.json --output ./results
    desktest suite ./tests --filter gedit
    desktest interactive task.json
    desktest validate task.json"
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

    /// Enable video recording of test sessions
    #[arg(long, default_value_t = false, global = true)]
    pub record: bool,

    /// Display resolution as WxH (e.g., 1280x720, 1920x1080) or preset (720p, 1080p)
    #[arg(long, global = true)]
    pub resolution: Option<String>,

    /// Enable live monitoring web dashboard
    #[arg(long, default_value_t = false, global = true)]
    pub monitor: bool,

    /// Port for the live monitoring dashboard
    #[arg(long, default_value_t = 7860, global = true)]
    pub monitor_port: u16,

    /// Interactive mode: start container and app, then wait for Ctrl+C (no agent) [legacy flag]
    #[arg(long, default_value_t = false, hide = true)]
    pub interactive: bool,
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
  desktest run task.json
  desktest run task.json --config config.json
  desktest run task.json --output ./my-results --verbose
  desktest run task.json --record --debug
  desktest run task.json --resolution 1280x720")]
    Run {
        /// Path to the task JSON file
        task: std::path::PathBuf,
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
EXAMPLES:
  desktest attach task.json --container my-container
  desktest attach task.json --container abc123 --config config.json
  desktest attach task.json --container my-container --resolution 1280x720")]
    Attach {
        /// Path to the task JSON file
        task: std::path::PathBuf,

        /// Docker container ID or name to attach to (must be running)
        #[arg(long)]
        container: String,
    },

    /// Replay a codified Python script inside a container
    #[command(after_help = "\
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

    /// Generate a web-based trajectory review viewer
    #[command(after_help = "\
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
