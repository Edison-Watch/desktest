use crate::config::Config;
use crate::error::AppError;
use crate::provider;
use crate::task::AppConfig;

/// Check that the Docker daemon is reachable via the API socket.
/// Returns the connected client on success for reuse.
pub async fn check_docker() -> Result<bollard::Docker, AppError> {
    let client = bollard::Docker::connect_with_local_defaults()
        .map_err(|e| AppError::Infra(format!("Cannot connect to Docker: {e}")))?;

    client.ping().await.map_err(|e| {
        AppError::Infra(format!(
            "Docker daemon is not responding: {e}\n\
             \n\
             Make sure Docker is running:\n\
             - macOS: open Docker Desktop, OrbStack, or start Colima (`colima start`)\n\
             - Linux: `sudo systemctl start docker`\n\
             \n\
             If Docker is running, check socket permissions:\n\
             - Linux: `sudo usermod -aG docker $USER` (then log out and back in)"
        ))
    })?;

    Ok(client)
}

/// Check that Tart is installed and accessible.
pub fn check_tart() -> Result<(), AppError> {
    match std::process::Command::new("tart")
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(AppError::Config(
            "Tart is not installed or not in PATH.\n\
             Install it with: brew install cirruslabs/cli/tart\n\
             Requires Apple Silicon (M1+) running macOS 13+."
                .into(),
        )),
    }
}

/// Check that an API key is available for the configured provider.
///
/// Delegates to `provider::resolve_api_key` so the resolution logic lives
/// in one place and can't drift between preflight and runtime.
/// Skips the check for providers that don't need an API key (e.g., claude-cli).
pub fn check_api_key(config: &Config) -> Result<(), AppError> {
    if config.provider == "claude-cli" || config.provider == "codex-cli" {
        return Ok(());
    }
    provider::resolve_api_key(&config.api_key, &config.provider).map(|_| ())
}

/// Run all preflight checks for commands that need Docker + LLM.
///
/// Skips API key check when `needs_llm` is false (e.g., --replay mode).
/// Checks Docker or Tart based on the app config.
pub async fn run_preflight(
    config: &Config,
    needs_llm: bool,
    app: Option<&AppConfig>,
) -> Result<(), AppError> {
    let is_macos = matches!(
        app,
        Some(AppConfig::MacosTart { .. } | AppConfig::MacosNative { .. })
    );

    if is_macos {
        check_tart()?;
    } else {
        let _client = check_docker().await?;
    }

    if needs_llm {
        check_api_key(config)?;
    }

    Ok(())
}

/// Run preflight checks and print results in a human-friendly format.
/// Returns true if all checks pass.
pub async fn run_doctor(config: &Config) -> bool {
    use std::io::Write;

    let mut all_ok = true;

    // Docker check
    print!("Docker daemon ... ");
    let _ = std::io::stdout().flush();
    match check_docker().await {
        Ok(client) => {
            println!("ok");
            if let Ok(version) = client.version().await {
                if let Some(v) = version.version {
                    println!("  Docker Engine {v}");
                }
                if let Some(os) = version.os {
                    let arch_str = version.arch.as_deref().unwrap_or("unknown");
                    println!("  Platform: {os}/{arch_str}");
                }
            }
        }
        Err(e) => {
            println!("FAILED");
            println!("  {e}");
            all_ok = false;
        }
    }

    // Tart check (informational — only relevant on macOS Apple Silicon)
    if cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64" {
        print!("Tart VM ......... ");
        let _ = std::io::stdout().flush();
        match check_tart() {
            Ok(()) => {
                println!("ok");
                // Try to get version info
                if let Ok(output) = std::process::Command::new("tart").arg("--version").output() {
                    if output.status.success() {
                        let version = String::from_utf8_lossy(&output.stdout);
                        let version = version.trim();
                        if !version.is_empty() {
                            println!("  {version}");
                        }
                    }
                }
            }
            Err(_) => {
                println!("not installed (optional — needed for macOS testing)");
            }
        }
    } else {
        println!("Tart VM ......... skipped (requires macOS on Apple Silicon)");
    }

    // CLI binary check (replaces API key check for CLI-based providers)
    if config.provider == "claude-cli" {
        print!("Claude CLI ..... ");
        let _ = std::io::stdout().flush();
        let cli_ok = std::process::Command::new("claude")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if cli_ok {
            println!("ok");
        } else {
            println!("NOT FOUND");
            println!("  Install Claude Code from https://claude.ai/code");
            all_ok = false;
        }
    }

    if config.provider == "codex-cli" {
        print!("Codex CLI ...... ");
        let _ = std::io::stdout().flush();
        let cli_ok = std::process::Command::new("codex")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if cli_ok {
            println!("ok");
        } else {
            println!("NOT FOUND");
            println!("  Install Codex CLI: npm install -g @openai/codex");
            all_ok = false;
        }
    }

    // API key check
    print!("API key ({}) ... ", config.provider);
    let _ = std::io::stdout().flush();
    if config.provider == "claude-cli" {
        println!("ok (not needed — uses Claude Code CLI auth)");
    } else if config.provider == "codex-cli" {
        println!("ok (not needed — uses Codex CLI auth / CODEX_API_KEY)");
    } else {
        match provider::resolve_api_key_with_source(
            &config.api_key,
            &config.provider,
            config.api_key_source,
        ) {
            Ok((_key, source)) => {
                println!("ok (from {source})");
            }
            Err(e) => {
                println!("MISSING");
                println!("  {e}");
                all_ok = false;
            }
        }
    }

    // Model info
    println!("Model ........... {}", config.model);
    println!("Provider ........ {}", config.provider);
    println!(
        "Resolution ...... {}x{}",
        config.display_width, config.display_height
    );

    all_ok
}
