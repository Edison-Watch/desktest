use crate::config::Config;
use crate::error::AppError;

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

/// Check that an API key is available for the configured provider.
///
/// This mirrors the resolution logic in `provider::resolve_api_key` but runs
/// before any Docker work so the user gets immediate feedback.
pub fn check_api_key(config: &Config) -> Result<(), AppError> {
    if !config.api_key.is_empty() {
        return Ok(());
    }

    let provider_env = match config.provider.as_str() {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        _ => "",
    };

    if !provider_env.is_empty() {
        if let Ok(key) = std::env::var(provider_env) {
            if !key.is_empty() {
                return Ok(());
            }
        }
    }

    if let Ok(key) = std::env::var("LLM_API_KEY") {
        if !key.is_empty() {
            return Ok(());
        }
    }

    let hint = if provider_env.is_empty() {
        "Set an API key in your config file or the LLM_API_KEY environment variable.".to_string()
    } else {
        format!(
            "Set one of:\n\
             - {provider_env} environment variable\n\
             - LLM_API_KEY environment variable\n\
             - \"api_key\" in your config JSON (--config)"
        )
    };

    Err(AppError::Config(format!("No API key found for provider '{}'.\n\n{hint}", config.provider)))
}

/// Run all preflight checks for commands that need Docker + LLM.
///
/// Skips API key check when `needs_llm` is false (e.g., --replay mode).
pub async fn run_preflight(config: &Config, needs_llm: bool) -> Result<(), AppError> {
    let _client = check_docker().await?;

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

    // API key check
    print!("API key ({}) ... ", config.provider);
    let _ = std::io::stdout().flush();
    match check_api_key(config) {
        Ok(()) => {
            // Show which source the key came from (without revealing it)
            let source = if !config.api_key.is_empty() {
                "config file"
            } else {
                let provider_env = match config.provider.as_str() {
                    "openai" => "OPENAI_API_KEY",
                    "anthropic" => "ANTHROPIC_API_KEY",
                    _ => "",
                };
                if !provider_env.is_empty()
                    && std::env::var(provider_env).map_or(false, |k| !k.is_empty())
                {
                    provider_env
                } else {
                    "LLM_API_KEY"
                }
            };
            println!("ok (from {source})");
        }
        Err(e) => {
            println!("MISSING");
            println!("  {e}");
            all_ok = false;
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
