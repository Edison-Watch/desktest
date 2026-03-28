use std::time::{Duration, Instant};

use tracing::info;

use crate::agent;
use crate::artifacts;
use crate::config::Config;
use crate::docker;
use crate::error::{AgentOutcome, AppError};
use crate::evaluator;
use crate::monitor;
use crate::observation;
use crate::provider;
use crate::readiness;
use crate::recording;
use crate::results;
use crate::session::{Session, SessionKind};
use crate::setup;
use crate::task;

/// Internal result from run_task_inner, preserving evaluation details for results.json.
pub(crate) struct TaskRunResult {
    pub(crate) outcome: AgentOutcome,
    pub(crate) eval_result: Option<evaluator::EvaluationResult>,
    /// True when an agent loop was run (LLM or hybrid mode).
    pub(crate) agent_ran: bool,
}

/// CLI overrides for provider/model/api_key flags.
pub(crate) struct LlmOverrides {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub llm_max_retries: Option<usize>,
}

/// Runtime configuration flags that travel together across orchestration functions.
#[derive(Debug, Clone)]
pub(crate) struct RunConfig {
    pub debug: bool,
    pub verbose: bool,
    pub bash_enabled: bool,
    pub no_recording: bool,
    pub qa: bool,
    pub artifacts_timeout_secs: u64,
    pub no_artifacts: bool,
    pub artifacts_exclude: Vec<String>,
    pub llm_max_retries: usize,
}

/// Groups the core references that every orchestration inner function needs.
struct TaskContext<'a> {
    task_def: &'a task::TaskDefinition,
    config: &'a Config,
    session: &'a SessionKind,
    artifacts_dir: &'a std::path::Path,
}

/// Load config from --config flag path or use task defaults.
pub(crate) fn load_config_or_defaults(
    config_flag: &Option<std::path::PathBuf>,
    resolution: &Option<String>,
    llm: &LlmOverrides,
) -> Config {
    let mut config = if let Some(config_path) = config_flag {
        match Config::load_and_validate(config_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Config error: {e}");
                std::process::exit(e.exit_code());
            }
        }
    } else {
        Config::from_task_defaults()
    };

    // CLI flags override config file values.
    // Note: we intentionally do NOT update config.api_base_url when --provider
    // changes, because create_provider() uses is_known_default_url() to detect
    // stale default URLs and auto-resolves to the correct endpoint for each
    // named provider. The api_base_url field only matters when the user
    // explicitly sets a custom URL.
    if let Some(p) = &llm.provider {
        // When switching providers, clear any config-file API key so the new
        // provider's env var is used instead of sending the wrong key.
        // Only clear when the provider actually changes — if --provider matches
        // the config's provider, the config key is still valid.
        if config.provider != *p && !config.api_key.is_empty() && llm.api_key.is_none() {
            config.api_key = String::new();
        }
        config.provider = p.clone();
    }
    if let Some(m) = &llm.model {
        config.model = m.clone();
    }
    if let Some(k) = &llm.api_key {
        config.api_key = k.clone();
        config.api_key_source = Some("--api-key flag");
    }
    if let Some(max_retries) = llm.llm_max_retries {
        config.llm_max_retries = max_retries;
    }

    // Warn if the provider was switched but the model still looks like it
    // belongs to the old provider (e.g. claude model sent to Gemini).
    if llm.provider.is_some() && llm.model.is_none() {
        let non_anthropic = !matches!(
            config.provider.as_str(),
            "anthropic" | "custom" | "claude-cli" | "codex-cli"
        );
        if non_anthropic
            && (config.model.starts_with("claude") || config.model.starts_with("anthropic/"))
        {
            eprintln!(
                "Warning: --provider {} with default model '{}' — did you mean to also pass --model?",
                config.provider, config.model
            );
        }
    }

    if let Some(res) = resolution {
        match parse_resolution(res) {
            Ok((w, h)) => {
                config.display_width = w;
                config.display_height = h;
            }
            Err(e) => {
                eprintln!("Resolution error: {e}");
                std::process::exit(2);
            }
        }
    }

    config
}

/// Parse a resolution string like "1280x720", "720p", or "1080p" into (width, height).
pub(crate) fn parse_resolution(s: &str) -> Result<(u32, u32), AppError> {
    match s.to_lowercase().as_str() {
        "720p" => Ok((1280, 720)),
        "1080p" => Ok((1920, 1080)),
        other => {
            let parts: Vec<&str> = other.split('x').collect();
            if parts.len() != 2 {
                return Err(AppError::Config(format!(
                    "Invalid resolution '{s}': expected WxH (e.g., 1280x720) or preset (720p, 1080p)"
                )));
            }
            let w = parts[0]
                .parse::<u32>()
                .map_err(|_| AppError::Config(format!("Invalid resolution width in '{s}'")))?;
            let h = parts[1]
                .parse::<u32>()
                .map_err(|_| AppError::Config(format!("Invalid resolution height in '{s}'")))?;
            if w == 0 || h == 0 {
                return Err(AppError::Config(format!(
                    "Invalid resolution '{s}': width and height must be greater than zero"
                )));
            }
            Ok((w, h))
        }
    }
}

/// Resolve the Docker image to use, building the electron image if needed.
/// Returns the electron image name when `config.electron` is true and no custom image is set.
pub(crate) async fn resolve_image_name<'a>(
    config: &Config,
    custom_image: Option<&'a str>,
) -> Result<Option<&'a str>, AppError> {
    if config.electron && custom_image.is_none() {
        let client = bollard::Docker::connect_with_local_defaults()
            .map_err(|e| AppError::Infra(format!("Cannot connect to Docker: {e}")))?;
        docker::DockerSession::ensure_electron_image(&client, false).await?;
        // Safety: IMAGE_NAME_ELECTRON is a &'static str, coerce to 'a
        return Ok(Some(docker::IMAGE_NAME_ELECTRON));
    }
    Ok(custom_image)
}

/// Collect artifacts with timeout handling, or skip entirely if `--no-artifacts`.
pub(crate) async fn maybe_collect_artifacts(
    session: &crate::session::SessionKind,
    artifacts_dir: &std::path::Path,
    run: &RunConfig,
) {
    if run.no_artifacts {
        info!("Skipping artifact collection (--no-artifacts)");
        return;
    }

    // 0 means no timeout (unlimited) — just await directly
    if run.artifacts_timeout_secs == 0 {
        info!("Collecting artifacts (no timeout)...");
        let _ = artifacts::collect_artifacts(session, artifacts_dir, &run.artifacts_exclude).await;
        return;
    }

    info!(
        "Collecting artifacts (timeout: {}s)...",
        run.artifacts_timeout_secs
    );
    let timeout = Duration::from_secs(run.artifacts_timeout_secs);
    match tokio::time::timeout(
        timeout,
        artifacts::collect_artifacts(session, artifacts_dir, &run.artifacts_exclude),
    )
    .await
    {
        Ok(_) => {}
        Err(_) => {
            tracing::warn!(
                "Artifact collection timed out after {}s — skipping remaining artifacts. \
                 Use --artifacts-timeout to increase or --no-artifacts to skip.",
                run.artifacts_timeout_secs
            );
        }
    }
}

/// Run a test from a task definition file.
///
/// This is the new task-based flow: load task → create container → wait for desktop →
/// run setup steps → deploy & launch app → run agent loop → cleanup.
pub(crate) async fn run_task(
    mut task_def: task::TaskDefinition,
    mut config: Config,
    run: RunConfig,
    output_dir: std::path::PathBuf,
    monitor: Option<monitor::MonitorHandle>,
    artifacts_dir_override: Option<std::path::PathBuf>,
) -> Result<AgentOutcome, AppError> {
    let start = Instant::now();

    // Guard: vnc_attach tasks must use `desktest attach`, not `desktest run`
    if matches!(task_def.app, task::AppConfig::VncAttach { .. }) {
        return Err(AppError::Config(
            "Task uses 'vnc_attach' app type — use 'desktest attach' instead of 'desktest run'."
                .into(),
        ));
    }

    // Resolve secrets from environment variables and apply substitution
    let resolved_secrets = task_def.resolve_secrets()?;
    task_def.apply_secrets(&resolved_secrets)?;
    let redactor = crate::redact::Redactor::new(resolved_secrets.values().cloned());

    // Populate config app fields from task definition (needed when no --config file)
    config.apply_task_app(&task_def.app);

    // Set up artifacts directory
    let artifacts_dir = match artifacts_dir_override {
        Some(dir) => dir,
        None => std::env::current_dir()
            .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
            .join("desktest_artifacts"),
    };
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    // Determine custom Docker image from task definition
    let custom_image = match &task_def.app {
        task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
        _ => None,
    };

    let extra_env = if resolved_secrets.is_empty() {
        None
    } else {
        Some(&resolved_secrets)
    };

    // Build electron image + create container (inside select! so Ctrl+C works)
    info!("Creating Docker container...");
    let session = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C) during container setup");
            return Err(AppError::Infra("Interrupted by user".into()));
        }
        r = async {
            let effective_image = resolve_image_name(&config, custom_image).await?;
            docker::DockerSession::create(&config, effective_image, extra_env).await
        } => r?,
    };

    let session = SessionKind::Docker(session);

    let ctx = TaskContext {
        task_def: &task_def,
        config: &config,
        session: &session,
        artifacts_dir: &artifacts_dir,
    };

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_task_inner(&ctx, run.clone(), monitor.as_ref(), start, Some(&redactor)) => r,
    };

    // Collect artifacts (with timeout) and clean up
    maybe_collect_artifacts(&session, &artifacts_dir, &run).await;

    info!("Cleaning up container...");
    let _ = session.cleanup().await;

    finalize_run(
        result,
        &task_def,
        &artifacts_dir,
        &output_dir,
        start,
        run.qa,
        Some(&redactor),
    )
}

/// Shared post-inner logic: save task.json, write results.json, map to outcome.
///
/// Used by both `run_task` and `run_attach` to avoid duplication.
fn finalize_run(
    result: Result<TaskRunResult, AppError>,
    task_def: &task::TaskDefinition,
    artifacts_dir: &std::path::Path,
    output_dir: &std::path::Path,
    start: Instant,
    qa: bool,
    redactor: Option<&crate::redact::Redactor>,
) -> Result<AgentOutcome, AppError> {
    let test_id = &task_def.id;
    let duration_ms = start.elapsed().as_millis() as u64;

    // Save task definition to artifacts for review HTML
    let task_json_path = artifacts_dir.join("task.json");
    match serde_json::to_value(task_def) {
        Ok(mut value) => {
            if let Some(redactor) = redactor {
                crate::redact::redact_json_value(&mut value, redactor);
            }
            match serde_json::to_string_pretty(&value) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&task_json_path, &json) {
                        tracing::warn!("Failed to write task.json to artifacts: {e}");
                    }
                }
                Err(e) => tracing::warn!("Failed to serialize task definition: {e}"),
            }
        }
        Err(e) => tracing::warn!("Failed to convert task definition to JSON value: {e}"),
    }

    // Write results.json
    let test_result = match &result {
        Ok(run_result) if !run_result.agent_ran => results::from_evaluation(
            test_id,
            run_result
                .eval_result
                .as_ref()
                .expect("programmatic mode has eval_result"),
            duration_ms,
        ),
        Ok(run_result) => results::from_outcome(
            test_id,
            &run_result.outcome,
            run_result.eval_result.as_ref(),
            duration_ms,
            qa,
        ),
        Err(e) => results::from_error(test_id, e, duration_ms),
    };
    if let Err(e) = results::write_results(&test_result, output_dir, redactor) {
        tracing::warn!("Failed to write results.json: {e}");
    }

    result.map(|r| r.outcome)
}

async fn run_task_inner(
    ctx: &TaskContext<'_>,
    run: RunConfig,
    monitor: Option<&monitor::MonitorHandle>,
    start_time: Instant,
    redactor: Option<&crate::redact::Redactor>,
) -> Result<TaskRunResult, AppError> {
    use task::EvaluatorMode;

    let TaskContext {
        task_def,
        config,
        session,
        ..
    } = ctx;
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    // Determine evaluation mode (default to LLM if no evaluator configured)
    let eval_mode = task_def
        .evaluator
        .as_ref()
        .map(|e| &e.mode)
        .unwrap_or(&EvaluatorMode::Llm);

    info!(
        "Evaluation mode: {}",
        match eval_mode {
            EvaluatorMode::Llm => "llm",
            EvaluatorMode::Programmatic => "programmatic",
            EvaluatorMode::Hybrid => "hybrid",
        }
    );

    // 1. Wait for desktop to be ready
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, run.debug).await?;

    // 1b. Validate custom image dependencies (after desktop is ready so X11-dependent imports work)
    let is_docker_image = matches!(task_def.app, task::AppConfig::DockerImage { .. });
    if is_docker_image {
        let docker = session
            .as_docker()
            .expect("Docker session required for custom image validation");
        docker.validate_custom_image().await?;
    }

    // 2. Deploy app (before setup steps, so setup can reference deployed files)
    let app_path = if is_docker_image {
        info!("Custom Docker image: skipping app deployment");
        String::new()
    } else {
        info!("Deploying app...");
        let docker = session
            .as_docker()
            .expect("Docker session required for app deployment");
        docker.deploy_app(config).await?
    };

    // 3. Run setup steps from task definition (after deploy, before app launch)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config, redactor).await?;
    }

    // 4. Launch app
    if is_docker_image {
        if let task::AppConfig::DockerImage { entrypoint_cmd, .. } = &task_def.app {
            if let Some(cmd) = entrypoint_cmd {
                info!("Waiting for stable window baseline...");
                let baseline_windows = readiness::get_stable_window_list(session).await?;

                info!("Launching app via entrypoint_cmd: {cmd}");
                session
                    .exec_detached_with_log(&["bash", "-c", cmd], "/tmp/app.log")
                    .await?;

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                let pgrep_cmd = format!(
                    "pgrep -f {} || true",
                    shell_escape::escape(cmd.as_str().into())
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
                readiness::wait_for_app_window(session, &baseline_windows, timeout, run.debug)
                    .await?;
            } else {
                info!(
                    "No entrypoint_cmd specified, assuming app starts automatically in custom image"
                );
            }
        }
    } else {
        info!("Waiting for stable window baseline...");
        let baseline_windows = readiness::get_stable_window_list(session).await?;

        let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
        info!("Launching app: {app_path}");
        let docker = session
            .as_docker()
            .expect("Docker session required for app launch");
        docker
            .launch_app(&app_path, is_appimage, config.electron)
            .await?;

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
        readiness::wait_for_app_window(session, &baseline_windows, timeout, run.debug).await?;
    }

    // 6. Print VNC info
    let vnc_url = if let Some(port) = config.vnc_port {
        let url = crate::config::format_host_port(&config.vnc_bind_addr, port);
        println!("VNC available at {url}");
        url
    } else {
        String::new()
    };

    // Publish TestStart for live monitoring
    if let Some(m) = monitor {
        let instruction = match redactor {
            Some(redactor) => redactor.redact(&task_def.instruction),
            None => task_def.instruction.clone(),
        };
        let completion_condition =
            task_def
                .completion_condition
                .as_ref()
                .map(|condition| match redactor {
                    Some(redactor) => redactor.redact(condition),
                    None => condition.clone(),
                });
        m.send(monitor::MonitorEvent::TestStart {
            test_id: task_def.id.clone(),
            instruction,
            completion_condition,
            vnc_url,
            max_steps: task_def.max_steps as usize,
        });
    }

    // 7-8. Start recording and run agent loop / evaluation
    run_eval_loop(ctx, eval_mode, run, monitor, start_time, redactor).await
}

/// Shared logic for recording, agent loop, and evaluation.
///
/// Used by both `run_task_inner` and `run_attach_inner` to avoid duplication.
async fn run_eval_loop(
    ctx: &TaskContext<'_>,
    eval_mode: &task::EvaluatorMode,
    run: RunConfig,
    monitor: Option<&monitor::MonitorHandle>,
    start_time: Instant,
    redactor: Option<&crate::redact::Redactor>,
) -> Result<TaskRunResult, AppError> {
    use task::EvaluatorMode;

    let TaskContext {
        task_def,
        config,
        session,
        artifacts_dir,
    } = ctx;

    // Start video recording
    let recording = if run.no_recording {
        None
    } else {
        match recording::Recording::start(session, config.display_width, config.display_height)
            .await
        {
            Ok(rec) => Some(rec),
            Err(e) => {
                tracing::warn!("Failed to start recording: {e}");
                None
            }
        }
    };

    match eval_mode {
        EvaluatorMode::Programmatic => {
            info!("Programmatic mode: skipping agent loop, running evaluation...");

            let evaluator = task_def.evaluator.as_ref().expect(
                "Programmatic mode requires evaluator config (validated at task load time)",
            );

            // ScriptReplay metrics write their own trajectory with per-step screenshots,
            // so skip the generic pre/post trajectory for replay runs.
            // NOTE: uses `.any()` so mixed evaluators (ScriptReplay + other metrics)
            // also skip the generic trajectory. This is acceptable because
            // apply_replay_override always places ScriptReplay first and replay mode
            // doesn't normally mix with other metric types.
            let has_replay = evaluator
                .metrics
                .iter()
                .any(|m| matches!(m, task::MetricConfig::ScriptReplay { .. }));

            let mut screenshot_count = 0usize;
            let mut trajectory_logger: Option<crate::trajectory::TrajectoryLogger> = if !has_replay
            {
                match crate::trajectory::TrajectoryLogger::new(
                    artifacts_dir,
                    run.verbose,
                    redactor.cloned(),
                ) {
                    Ok(tl) => Some(tl),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to create trajectory logger, review HTML may be empty: {e}"
                        );
                        None
                    }
                }
            } else {
                None
            };

            // Capture pre-evaluation screenshot (non-replay only)
            if !has_replay {
                if let Ok((path, _)) =
                    observation::capture_screenshot_with_retry(session, artifacts_dir, 1).await
                {
                    screenshot_count += 1;
                    if let Some(ref mut tl) = trajectory_logger {
                        let entry = crate::trajectory::TrajectoryEntry {
                            step: 1,
                            timestamp: crate::trajectory::chrono_iso8601_now(),
                            action_code: String::new(),
                            thought: Some("Pre-evaluation screenshot".into()),
                            screenshot_path: path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string()),
                            a11y_tree_path: None,
                            result: "success".into(),
                            llm_raw_response: None,
                            bash_output: None,
                            error_feedback: None,
                            action_type: None,
                        };
                        tl.log_entry(&entry);
                    }
                }
            }

            let eval_result = evaluator::run_evaluation(session, evaluator, artifacts_dir).await;

            // Capture post-evaluation screenshot (non-replay only)
            if !has_replay {
                if let Ok((path, _)) =
                    observation::capture_screenshot_with_retry(session, artifacts_dir, 2).await
                {
                    screenshot_count += 1;
                    if let Some(ref mut tl) = trajectory_logger {
                        let (result_str, thought, action_code) = match &eval_result {
                            Ok(r) => {
                                let status = if r.passed { "PASSED" } else { "FAILED" };
                                let passed_count =
                                    r.metric_results.iter().filter(|m| m.passed).count();
                                let total_count = r.metric_results.len();
                                let thought = format!(
                                    "Evaluation {status}: {passed_count}/{total_count} metrics passed (mode: {mode})",
                                    mode = r.mode,
                                );
                                let mut code_lines = Vec::new();
                                for (i, m) in r.metric_results.iter().enumerate() {
                                    let m_status = if m.passed { "PASSED" } else { "FAILED" };
                                    code_lines.push(format!(
                                        "# Metric {}: {} — {}",
                                        i + 1,
                                        m.metric,
                                        m_status
                                    ));
                                    if !m.expected.is_empty() {
                                        code_lines.push(format!("#   Expected: {}", m.expected));
                                    }
                                    if !m.actual.is_empty() {
                                        code_lines.push(format!("#   Actual:   {}", m.actual));
                                    }
                                    if !m.detail.is_empty() {
                                        code_lines.push(format!("#   Detail:   {}", m.detail));
                                    }
                                }
                                let result_str = if r.passed {
                                    "evaluation_passed"
                                } else {
                                    "evaluation_failed"
                                };
                                (result_str, thought, code_lines.join("\n"))
                            }
                            Err(e) => (
                                "evaluation_error",
                                "Evaluation error".to_string(),
                                format!("# Error: {e}"),
                            ),
                        };
                        let entry = crate::trajectory::TrajectoryEntry {
                            step: 2,
                            timestamp: crate::trajectory::chrono_iso8601_now(),
                            action_code,
                            thought: Some(thought),
                            screenshot_path: path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string()),
                            a11y_tree_path: None,
                            result: result_str.into(),
                            llm_raw_response: None,
                            bash_output: None,
                            error_feedback: None,
                            action_type: None,
                        };
                        tl.log_entry(&entry);
                    }
                }
            }

            if let Some(rec) = &recording {
                rec.stop(session).await;
                rec.collect(session, artifacts_dir).await;
            }

            // For replay runs, derive screenshot_count from trajectory entries that have screenshots
            if has_replay {
                let trajectory_path = artifacts_dir.join("trajectory.jsonl");
                if let Ok(content) = tokio::fs::read_to_string(&trajectory_path).await {
                    screenshot_count = content
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                        .filter(|v| v.get("screenshot_path").is_some_and(|p| !p.is_null()))
                        .count();
                }
            }

            let eval_result = eval_result?;
            print_validation_results(None, Some(&eval_result));

            if let Some(m) = monitor {
                m.send(monitor::MonitorEvent::TestComplete {
                    test_id: task_def.id.clone(),
                    passed: eval_result.passed,
                    reasoning: format_evaluation_reasoning(None, Some(&eval_result)),
                    duration_ms: start_time.elapsed().as_millis() as u64,
                });
            }

            Ok(TaskRunResult {
                outcome: AgentOutcome {
                    passed: eval_result.passed,
                    reasoning: format_evaluation_reasoning(None, Some(&eval_result)),
                    screenshot_count,
                    bugs_found: 0,
                },
                eval_result: Some(eval_result),
                agent_ran: false,
            })
        }
        EvaluatorMode::Llm => {
            info!("Starting agent loop v2 (LLM-only evaluation)...");
            let agent_loop_result =
                run_agent_loop(ctx, run, recording.as_ref(), monitor, redactor).await;

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
            info!("Starting agent loop v2 (hybrid evaluation)...");
            let agent_loop_result =
                run_agent_loop(ctx, run, recording.as_ref(), monitor, redactor).await;

            if let Some(rec) = &recording {
                rec.stop(session).await;
                rec.collect(session, artifacts_dir).await;
            }

            let agent_outcome = agent_loop_result?;

            info!("Agent loop complete, running programmatic evaluation...");
            let evaluator = task_def
                .evaluator
                .as_ref()
                .expect("Hybrid mode requires evaluator config (validated at task load time)");
            let eval_result = evaluator::run_evaluation(session, evaluator, artifacts_dir).await?;

            let both_passed = agent_outcome.passed && eval_result.passed;
            print_validation_results(Some(&agent_outcome), Some(&eval_result));

            if let Some(m) = monitor {
                m.send(monitor::MonitorEvent::TestComplete {
                    test_id: task_def.id.clone(),
                    passed: both_passed,
                    reasoning: format_evaluation_reasoning(
                        Some(&agent_outcome),
                        Some(&eval_result),
                    ),
                    duration_ms: start_time.elapsed().as_millis() as u64,
                });
            }

            Ok(TaskRunResult {
                outcome: AgentOutcome {
                    passed: both_passed,
                    reasoning: format_evaluation_reasoning(
                        Some(&agent_outcome),
                        Some(&eval_result),
                    ),
                    screenshot_count: agent_outcome.screenshot_count,
                    bugs_found: agent_outcome.bugs_found,
                },
                eval_result: Some(eval_result),
                agent_ran: true,
            })
        }
    }
}

/// Build an AgentLoopV2Config from a task definition, probing the a11y tree
/// timing if no explicit override is set.
pub(crate) async fn build_agent_loop_config(
    task_def: &task::TaskDefinition,
    session: &SessionKind,
    config: &Config,
    run: RunConfig,
) -> agent::loop_v2::AgentLoopV2Config {
    let max_a11y_nodes = task_def.max_a11y_nodes.unwrap_or(10_000);
    let max_steps = task_def.max_steps as usize;

    let mut obs_config = observation::ObservationConfig {
        max_a11y_nodes,
        ..observation::ObservationConfig::default()
    };

    // Determine a11y timeout: explicit override or probe
    let a11y_timeout = if let Some(secs) = task_def.a11y_timeout_secs {
        info!("Using explicit a11y timeout: {secs}s");
        Duration::from_secs(secs)
    } else {
        match observation::probe_a11y_timing(session, max_a11y_nodes, obs_config.max_a11y_tokens)
            .await
        {
            Ok(measured) => {
                let timeout = measured
                    .mul_f64(1.5)
                    .max(Duration::from_secs(15))
                    .min(Duration::from_secs(60));
                info!(
                    "A11y probe: extraction took {:.1}s, setting timeout to {:.1}s",
                    measured.as_secs_f64(),
                    timeout.as_secs_f64()
                );
                timeout
            }
            Err(e) => {
                // Use the cap (60s) as fallback — if the probe timed out, 15s would
                // guarantee every subsequent extraction also times out
                tracing::warn!("A11y probe failed ({e}), using maximum 60s timeout as fallback");
                Duration::from_secs(60)
            }
        }
    };
    obs_config.a11y_timeout = a11y_timeout;

    // Compute adjusted total timeout to account for per-step observation overhead.
    // Uses the a11y timeout ceiling (not measured time) intentionally — this is the
    // worst-case wait per step, so total_timeout must budget for it.
    // 1.0s accounts for fixed per-step overhead (screenshot capture).
    let per_step_overhead = obs_config.sleep_after_action + 1.0 + a11y_timeout.as_secs_f64();
    let base_timeout = task_def.timeout;
    let adjusted_total = base_timeout as f64 + (per_step_overhead * max_steps as f64);
    let total_timeout = Duration::from_secs_f64(adjusted_total);
    info!(
        "Total timeout: {:.0}s (base {base_timeout}s + {:.1}s overhead/step × {max_steps} steps)",
        adjusted_total, per_step_overhead
    );

    agent::loop_v2::AgentLoopV2Config {
        max_steps,
        total_timeout,
        observation_config: obs_config,
        llm_max_retries: run.llm_max_retries,
        debug: run.debug,
        verbose: run.verbose,
        bash_enabled: run.bash_enabled,
        qa: run.qa,
        display_width: config.display_width,
        display_height: config.display_height,
        early_exit: task_def.early_exit.clone(),
        ..Default::default()
    }
}

/// Run the v2 agent loop (used by LLM and hybrid modes).
async fn run_agent_loop(
    ctx: &TaskContext<'_>,
    run: RunConfig,
    recording: Option<&recording::Recording>,
    monitor: Option<&monitor::MonitorHandle>,
    redactor: Option<&crate::redact::Redactor>,
) -> Result<AgentOutcome, AppError> {
    let TaskContext {
        task_def,
        config,
        session,
        artifacts_dir,
    } = ctx;
    let llm_client = provider::create_provider(
        &config.provider,
        &config.api_key,
        &config.model,
        &config.api_base_url,
    )?;

    let is_qa = run.qa;
    let mut loop_config = build_agent_loop_config(task_def, session, config, run).await;
    loop_config.test_id = task_def.id.clone();
    loop_config.redactor = redactor.cloned();
    let full_instruction = task_def.full_instruction();
    let notifier = if is_qa {
        let pipeline = crate::notify::build_pipeline(config);
        if pipeline.is_empty() {
            None
        } else {
            Some(pipeline)
        }
    } else {
        None
    };

    let mut agent_loop = agent::loop_v2::AgentLoopV2::new(
        llm_client,
        session,
        artifacts_dir.to_path_buf(),
        &full_instruction,
        loop_config,
        recording,
        monitor.cloned(),
        notifier,
    );
    agent_loop.run().await
}

/// Run a task against an already-running container (attach mode).
///
/// Unlike `run_task`, this does not create, start, or clean up a container.
/// It connects to the given container by ID/name and runs the agent loop
/// and evaluation against it.
pub(crate) async fn run_attach(
    mut task_def: task::TaskDefinition,
    mut config: Config,
    container: &str,
    run: RunConfig,
    output_dir: std::path::PathBuf,
    monitor: Option<monitor::MonitorHandle>,
    artifacts_dir_override: Option<std::path::PathBuf>,
) -> Result<AgentOutcome, AppError> {
    let start = Instant::now();

    // Resolve secrets from environment variables and apply substitution
    let resolved_secrets = task_def.resolve_secrets()?;
    task_def.apply_secrets(&resolved_secrets)?;
    let redactor = crate::redact::Redactor::new(resolved_secrets.values().cloned());

    // Warn if the task isn't designed for attach mode
    if !matches!(task_def.app, task::AppConfig::VncAttach { .. }) {
        let app_type = match &task_def.app {
            task::AppConfig::Appimage { .. } => "appimage",
            task::AppConfig::Folder { .. } => "folder",
            task::AppConfig::DockerImage { .. } => "docker_image",
            task::AppConfig::VncAttach { .. } => unreachable!(),
        };
        tracing::warn!(
            "Task '{}' uses app type '{}', but 'desktest attach' skips app deployment. \
             Consider using app type 'vnc_attach' for attach-mode tasks.",
            task_def.id,
            app_type
        );
    }

    // Populate config app fields from task definition (needed when no --config file)
    config.apply_task_app(&task_def.app);

    // Set up artifacts directory
    let artifacts_dir = match artifacts_dir_override {
        Some(dir) => dir,
        None => std::env::current_dir()
            .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
            .join("desktest_artifacts"),
    };
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    // Attach to existing container (no lifecycle management)
    info!("Attaching to container '{container}'...");
    let session = docker::DockerSession::attach(container).await?;
    let session = SessionKind::Docker(session);

    let ctx = TaskContext {
        task_def: &task_def,
        config: &config,
        session: &session,
        artifacts_dir: &artifacts_dir,
    };

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C)");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_attach_inner(&ctx, run.clone(), monitor.as_ref(), start, Some(&redactor)) => r,
    };

    // Collect artifacts but do NOT clean up the container (we don't own it)
    maybe_collect_artifacts(&session, &artifacts_dir, &run).await;

    finalize_run(
        result,
        &task_def,
        &artifacts_dir,
        &output_dir,
        start,
        run.qa,
        Some(&redactor),
    )
}

/// Inner logic for attach mode: run setup steps, agent loop, and evaluation.
///
/// Skips container creation, desktop readiness wait, image validation,
/// and app deployment/launch — all of which are handled by the external
/// orchestration script.
async fn run_attach_inner(
    ctx: &TaskContext<'_>,
    run: RunConfig,
    monitor: Option<&monitor::MonitorHandle>,
    start_time: Instant,
    redactor: Option<&crate::redact::Redactor>,
) -> Result<TaskRunResult, AppError> {
    let task_def = ctx.task_def;
    let session = ctx.session;
    let eval_mode = task_def
        .evaluator
        .as_ref()
        .map(|e| &e.mode)
        .unwrap_or(&task::EvaluatorMode::Llm);

    info!(
        "Attach mode — evaluation mode: {}",
        match eval_mode {
            task::EvaluatorMode::Llm => "llm",
            task::EvaluatorMode::Programmatic => "programmatic",
            task::EvaluatorMode::Hybrid => "hybrid",
        }
    );

    // Run setup steps if any (execute, copy, sleep are useful in attach mode)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config, redactor).await?;
    }

    // Publish TestStart for live monitoring
    if let Some(m) = monitor {
        let instruction = match redactor {
            Some(redactor) => redactor.redact(&task_def.instruction),
            None => task_def.instruction.clone(),
        };
        let completion_condition =
            task_def
                .completion_condition
                .as_ref()
                .map(|condition| match redactor {
                    Some(redactor) => redactor.redact(condition),
                    None => condition.clone(),
                });
        m.send(monitor::MonitorEvent::TestStart {
            test_id: task_def.id.clone(),
            instruction,
            completion_condition,
            vnc_url: String::new(),
            max_steps: task_def.max_steps as usize,
        });
    }

    // Recording, agent loop, and evaluation (shared with run_task_inner)
    run_eval_loop(ctx, eval_mode, run, monitor, start_time, redactor).await
}

/// Print validation results showing which sources passed/failed.
pub(crate) fn print_validation_results(
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
        (Some(a), Some(e)) => a.passed && e.passed, // hybrid
        (Some(a), None) => a.passed,                // llm
        (None, Some(e)) => e.passed,                // programmatic
        (None, None) => true,
    };
    let final_verdict = if final_passed { "PASSED" } else { "FAILED" };
    println!("  Final result: {final_verdict}");
    println!("========================\n");
}

/// Format a combined reasoning string from agent and evaluation results.
pub(crate) fn format_evaluation_reasoning(
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
            parts.push(format!(
                "Programmatic evaluation passed ({passed}/{total} metrics)"
            ));
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
            bugs_found: 0,
        }
    }

    fn make_eval_result(passed: bool, metrics: Vec<MetricResult>) -> EvaluationResult {
        EvaluationResult {
            passed,
            // mode is the evaluation strategy name (e.g. "programmatic", "llm", "hybrid"), not pass/fail
            mode: "programmatic".into(),
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
            task::AppConfig::DockerImage {
                image,
                entrypoint_cmd,
            } => {
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

    fn no_overrides() -> LlmOverrides {
        LlmOverrides {
            provider: None,
            model: None,
            api_key: None,
            llm_max_retries: None,
        }
    }

    #[test]
    fn test_load_config_or_defaults_none() {
        let config = load_config_or_defaults(&None, &None, &no_overrides());
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_parse_resolution_preset_720p() {
        assert_eq!(parse_resolution("720p").unwrap(), (1280, 720));
    }

    #[test]
    fn test_parse_resolution_preset_1080p() {
        assert_eq!(parse_resolution("1080p").unwrap(), (1920, 1080));
    }

    #[test]
    fn test_parse_resolution_wxh() {
        assert_eq!(parse_resolution("1280x720").unwrap(), (1280, 720));
        assert_eq!(parse_resolution("800x600").unwrap(), (800, 600));
    }

    #[test]
    fn test_parse_resolution_invalid() {
        assert!(parse_resolution("abc").is_err());
        assert!(parse_resolution("1280").is_err());
        assert!(parse_resolution("1280x").is_err());
        assert!(parse_resolution("0x720").is_err());
        assert!(parse_resolution("1280x0").is_err());
        assert!(parse_resolution("0x0").is_err());
    }

    #[test]
    fn test_load_config_with_resolution_override() {
        let config = load_config_or_defaults(&None, &Some("1280x720".into()), &no_overrides());
        assert_eq!(config.display_width, 1280);
        assert_eq!(config.display_height, 720);
    }

    #[test]
    fn test_load_config_with_llm_overrides() {
        let overrides = LlmOverrides {
            provider: Some("openrouter".into()),
            model: Some("anthropic/claude-sonnet-4".into()),
            api_key: Some("sk-or-test".into()),
            llm_max_retries: Some(7),
        };
        let config = load_config_or_defaults(&None, &None, &overrides);
        assert_eq!(config.provider, "openrouter");
        assert_eq!(config.model, "anthropic/claude-sonnet-4");
        assert_eq!(config.api_key, "sk-or-test");
        assert_eq!(config.llm_max_retries, 7);
    }

    #[test]
    fn test_load_config_partial_llm_overrides() {
        let overrides = LlmOverrides {
            provider: Some("gemini".into()),
            model: None,
            api_key: None,
            llm_max_retries: None,
        };
        let config = load_config_or_defaults(&None, &None, &overrides);
        assert_eq!(config.provider, "gemini");
        // model and api_key keep defaults
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_provider_override_clears_stale_api_key() {
        // Config file has an api_key for anthropic. Switching to openrouter
        // via --provider (without --api-key) must clear the stale key.
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(
            &config_path,
            r#"{
                "provider": "anthropic",
                "api_key": "sk-ant-stale",
                "model": "claude-sonnet-4-20250514",
                "app_type": "docker_image"
            }"#,
        )
        .unwrap();
        let overrides = LlmOverrides {
            provider: Some("openrouter".into()),
            model: None,
            api_key: None,
            llm_max_retries: None,
        };
        let config = load_config_or_defaults(&Some(config_path), &None, &overrides);
        // Stale anthropic key must be cleared when switching to openrouter
        assert!(config.api_key.is_empty(), "stale api_key should be cleared");
        assert_eq!(config.provider, "openrouter");
    }

    #[test]
    fn test_provider_override_same_provider_keeps_key() {
        // Config file has an api_key for anthropic. Passing --provider anthropic
        // (same provider) must NOT clear the key.
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(
            &config_path,
            r#"{"api_key": "sk-ant-valid", "provider": "anthropic", "app_type": "docker_image"}"#,
        )
        .unwrap();

        let overrides = LlmOverrides {
            provider: Some("anthropic".into()),
            model: None,
            api_key: None,
            llm_max_retries: None,
        };
        let config = load_config_or_defaults(&Some(config_path), &None, &overrides);
        assert_eq!(
            config.api_key, "sk-ant-valid",
            "api_key should be preserved when provider matches"
        );
        assert_eq!(config.provider, "anthropic");
    }

    #[test]
    fn test_provider_override_keeps_explicit_api_key() {
        // When both --provider and --api-key are given, the key is kept.
        let overrides = LlmOverrides {
            provider: Some("openrouter".into()),
            model: None,
            api_key: Some("sk-or-explicit".into()),
            llm_max_retries: None,
        };
        let config = load_config_or_defaults(&None, &None, &overrides);
        assert_eq!(config.api_key, "sk-or-explicit");
        assert_eq!(config.api_key_source, Some("--api-key flag"));
    }

    #[test]
    fn test_interactive_validate_only_requires_evaluator() {
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
        let mut task_mut = task;
        if let Some(ref mut eval) = task_mut.evaluator {
            eval.mode = task::EvaluatorMode::Programmatic;
        }
        assert_eq!(
            task_mut.evaluator.as_ref().unwrap().mode,
            task::EvaluatorMode::Programmatic
        );
    }
}
