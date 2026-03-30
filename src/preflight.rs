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

/// Check that we're running on macOS (required for native sessions).
///
/// Performs best-effort TCC permission detection by probing screencapture
/// and osascript (Automation).
pub fn check_native_macos() -> Result<(), AppError> {
    if cfg!(not(target_os = "macos")) {
        return Err(AppError::Config(
            "MacosNative app type requires macOS. This platform is not macOS.".into(),
        ));
    }

    check_screen_recording()?;
    check_automation()?;

    Ok(())
}

/// Probe screencapture to detect Screen Recording permission and display availability.
fn check_screen_recording() -> Result<(), AppError> {
    let probe_path =
        std::env::temp_dir().join(format!("desktest-preflight-{}.png", std::process::id()));
    let probe_str = probe_path.to_string_lossy().to_string();

    match std::process::Command::new("screencapture")
        .args(["-x", &probe_str])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => {
            let _ = std::fs::remove_file(&probe_path);
            Ok(())
        }
        Ok(output) => {
            let _ = std::fs::remove_file(&probe_path);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let combined = format!("{stderr}{stdout}");

            if combined.contains("permission") || combined.contains("TCC") {
                Err(AppError::Config(
                    "Screen Recording permission is not granted.\n\
                     Go to System Settings → Privacy & Security → Screen Recording\n\
                     and enable your terminal application."
                        .into(),
                ))
            } else if combined.contains("could not create image") || combined.contains("no image") {
                Err(AppError::Config(
                    "screencapture failed: no display available.\n\
                     This typically happens in SSH sessions or headless environments.\n\
                     Native macOS testing requires a local desktop session (GUI login, VNC, or \
                     Screen Sharing)."
                        .into(),
                ))
            } else {
                // Unknown screencapture failure — report it
                Err(AppError::Config(format!(
                    "screencapture failed (exit {}): {}\n\
                     Native macOS testing requires a working display and Screen Recording \
                     permission.",
                    output.status.code().unwrap_or(-1),
                    combined.trim()
                )))
            }
        }
        Err(_) => {
            // screencapture not found — unexpected on macOS, but not fatal
            Ok(())
        }
    }
}

/// Probe osascript to detect Automation (System Events) permission.
///
/// Uses a short timeout because osascript hangs indefinitely when macOS
/// shows a TCC permission dialog (common in SSH sessions where nobody
/// can click "Allow").
fn check_automation() -> Result<(), AppError> {
    use std::process::Stdio;
    use std::time::Duration;

    let timeout = Duration::from_secs(5);

    let child = std::process::Command::new("osascript")
        .args(["-e", "tell application \"System Events\" to return 1"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(_) => return Ok(()), // osascript not found — not fatal
    };

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        use std::io::Read;
                        let mut buf = String::new();
                        s.read_to_string(&mut buf).ok().map(|_| buf)
                    })
                    .unwrap_or_default();
                return Err(AppError::Config(format!(
                    "Automation permission check failed (exit {}).\n\
                     {}\n\
                     Go to System Settings → Privacy & Security → Automation\n\
                     and enable your terminal application for System Events.",
                    status.code().unwrap_or(-1),
                    stderr.trim()
                )));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Config(
                        "Automation permission check timed out.\n\
                         osascript hung for 5s — this usually means macOS is showing a \
                         permission dialog that cannot be dismissed (e.g., in an SSH session).\n\
                         \n\
                         Fix: grant Automation permission to your terminal app:\n\
                         System Settings → Privacy & Security → Automation → enable System Events\n\
                         \n\
                         Or connect via Screen Sharing / VNC to dismiss the dialog."
                            .into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(AppError::Infra(format!(
                    "Failed to check Automation permission: {e}"
                )));
            }
        }
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
    let is_macos_tart = matches!(app, Some(AppConfig::MacosTart { .. }));
    let is_macos_native = matches!(app, Some(AppConfig::MacosNative { .. }));

    if is_macos_tart {
        check_tart()?;
    } else if is_macos_native {
        check_native_macos()?;
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

    // Native macOS check (informational — only relevant on macOS)
    if cfg!(target_os = "macos") {
        print!("Native macOS .... ");
        let _ = std::io::stdout().flush();

        let screen_result = check_screen_recording();
        let automation_result = check_automation();

        if screen_result.is_ok() && automation_result.is_ok() {
            println!("ok (Screen Recording + Automation permissions available)");
        } else {
            println!("limited");
            if let Err(e) = screen_result {
                println!("  Screen Recording: {e}");
            }
            if let Err(e) = automation_result {
                println!("  Automation: {e}");
            }
        }
    } else {
        println!("Native macOS .... skipped (requires macOS)");
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
                println!("not configured (set via config, --api-key, or env var)");
                println!("  {e}");
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
