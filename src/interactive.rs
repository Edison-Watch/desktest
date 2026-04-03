use std::time::{Duration, Instant};

use tracing::info;

use crate::agent;
use crate::config::Config;
use crate::docker;
use crate::error::{AgentOutcome, AppError};
use crate::evaluator;
use crate::orchestration::{
    RunConfig, TaskRunResult, build_agent_loop_config, format_evaluation_reasoning,
    maybe_collect_artifacts, print_validation_results, resolve_image_name, run_task,
};
use crate::provider;
use crate::readiness;
use crate::recording;
use crate::results;
use crate::session::{Session, SessionKind};
use crate::setup;
use crate::task;

/// Run the interactive subcommand: starts container, runs setup, provides dev access.
pub(crate) async fn run_interactive(
    task_def: task::TaskDefinition,
    config: Config,
    run: RunConfig,
    output_dir: std::path::PathBuf,
    step: bool,
    validate_only: bool,
    artifacts_dir_override: Option<std::path::PathBuf>,
) -> Result<AgentOutcome, AppError> {
    // Guard: vnc_attach tasks must use `desktest attach`, not `desktest interactive`
    if matches!(task_def.app, task::AppConfig::VncAttach { .. }) {
        return Err(AppError::Config(
            "Task uses 'vnc_attach' app type — use 'desktest attach' instead of 'desktest interactive'.".into()
        ));
    }

    if validate_only {
        // --validate-only: skip agent loop, run programmatic evaluation only
        // Force evaluation mode to programmatic, reusing the existing run_task flow
        // but we need to check that the task has an evaluator config
        if task_def.evaluator.is_none() {
            return Err(AppError::Config(
                "interactive --validate-only requires a task with an evaluator config".into(),
            ));
        }

        // Create a modified task with programmatic mode for the evaluator
        let mut task_def = task_def;
        if let Some(ref mut eval) = task_def.evaluator {
            eval.mode = task::EvaluatorMode::Programmatic;
        }

        return run_task(
            task_def,
            config,
            run,
            output_dir,
            None,
            artifacts_dir_override,
        )
        .await;
    }

    if step {
        // --step: run agent one step at a time, pausing after each
        return run_interactive_step(task_def, config, run, output_dir, artifacts_dir_override)
            .await;
    }

    // Default interactive: start session, run setup, print connection info, pause
    run_interactive_pause(task_def, config, run, artifacts_dir_override).await
}

/// Create a session from the task definition, mirroring orchestration.rs session creation.
async fn create_session(
    task_def: &task::TaskDefinition,
    config: &Config,
    resolved_secrets: &std::collections::HashMap<String, String>,
    run: &RunConfig,
) -> Result<SessionKind, AppError> {
    let is_macos_tart = matches!(task_def.app, task::AppConfig::MacosTart { .. });
    let is_macos_native = matches!(task_def.app, task::AppConfig::MacosNative { .. });
    let is_windows_vm = matches!(task_def.app, task::AppConfig::WindowsVm { .. });
    let is_windows_native = matches!(task_def.app, task::AppConfig::WindowsNative { .. });

    let extra_env = if resolved_secrets.is_empty() {
        None
    } else {
        Some(resolved_secrets)
    };

    // Resource usage warnings (suppressed by --quiet)
    if !run.quiet {
        if is_macos_tart {
            crate::warnings::warn_tart_resources();
        } else if !is_macos_native && !is_windows_vm && !is_windows_native {
            crate::warnings::warn_docker_resources(config);
        }
    }

    if is_macos_tart {
        let base_image = match &task_def.app {
            task::AppConfig::MacosTart { base_image, .. } => base_image.as_str(),
            _ => unreachable!(),
        };
        info!("Creating Tart VM from '{base_image}'...");
        let tart_session = tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nInterrupted (Ctrl+C) during VM setup");
                return Err(AppError::Infra("Interrupted by user".into()));
            }
            r = crate::tart::TartSession::create(base_image) => r?,
        };
        Ok(SessionKind::Tart(tart_session))
    } else if is_macos_native {
        info!("Using native macOS session (no VM, no isolation)");
        Ok(SessionKind::Native(crate::session::NativeSession::create()))
    } else if is_windows_vm {
        let base_image = match &task_def.app {
            task::AppConfig::WindowsVm { base_image, .. } => base_image.as_str(),
            _ => unreachable!(),
        };
        info!("Creating Windows VM from '{base_image}'...");
        let win_session = tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nInterrupted (Ctrl+C) during Windows VM setup");
                return Err(AppError::Infra("Interrupted by user".into()));
            }
            r = crate::windows::WindowsVmSession::create(base_image) => r?,
        };
        Ok(SessionKind::WindowsVm(win_session))
    } else if is_windows_native {
        info!("Using native Windows session (no VM, no isolation)");
        Ok(SessionKind::WindowsNative(
            crate::session::WindowsNativeSession::create(),
        ))
    } else {
        let (custom_image, expected_digest, needs_fuse) = match &task_def.app {
            task::AppConfig::DockerImage {
                image,
                digest,
                needs_fuse,
                ..
            } => (Some(image.as_str()), digest.as_deref(), *needs_fuse),
            _ => (None, None, false),
        };

        info!("Creating Docker container...");
        let docker_session = tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nInterrupted (Ctrl+C) during container setup");
                return Err(AppError::Infra("Interrupted by user".into()));
            }
            r = async {
                let effective_image = resolve_image_name(config, custom_image).await?;
                docker::DockerSession::create(config, effective_image, extra_env, run.no_network, needs_fuse, expected_digest).await
            } => r?,
        };
        Ok(SessionKind::Docker(docker_session))
    }
}

/// Wait for desktop readiness, dispatching to the correct platform implementation.
async fn wait_for_desktop(
    task_def: &task::TaskDefinition,
    session: &SessionKind,
    timeout: Duration,
    debug: bool,
) -> Result<(), AppError> {
    info!("Waiting for desktop to be ready...");
    match &task_def.app {
        task::AppConfig::MacosTart { .. } => {
            crate::tart::readiness::wait_for_desktop_macos(session, timeout, debug).await
        }
        task::AppConfig::MacosNative { .. } => {
            info!("Native macOS session: desktop is ready");
            Ok(())
        }
        task::AppConfig::WindowsVm { .. } => {
            let win = session
                .as_windows_vm()
                .expect("Windows VM session required for Windows desktop readiness");
            crate::windows::readiness::wait_for_desktop(win).await
        }
        task::AppConfig::WindowsNative { .. } => {
            info!("Native Windows session: desktop is ready");
            Ok(())
        }
        _ => readiness::wait_for_desktop(session, timeout, debug).await,
    }
}

/// Deploy app into the session, dispatching to the correct platform implementation.
async fn deploy_app(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &SessionKind,
) -> Result<String, AppError> {
    match &task_def.app {
        task::AppConfig::MacosTart { .. } => {
            info!("Deploying app to macOS VM...");
            let tart = session
                .as_tart()
                .expect("Tart session required for macOS app deployment");
            tart.deploy_app(&task_def.app).await
        }
        task::AppConfig::MacosNative { .. } => {
            info!("Preparing native macOS app...");
            let native = session
                .as_native()
                .expect("Native session required for MacosNative app deployment");
            native.deploy_app(&task_def.app).await
        }
        task::AppConfig::WindowsVm { .. } => {
            info!("Deploying app to Windows VM...");
            let win = session
                .as_windows_vm()
                .expect("Windows VM session required for Windows app deployment");
            crate::windows::deploy::deploy_app(win, &task_def.app).await
        }
        task::AppConfig::WindowsNative { .. } => {
            info!("Preparing native Windows app...");
            let win_native = session
                .as_windows_native()
                .expect("Windows native session required for WindowsNative app deployment");
            win_native.deploy_app(&task_def.app).await
        }
        task::AppConfig::DockerImage { .. } => {
            info!("Custom Docker image: skipping app deployment");
            Ok(String::new())
        }
        _ => {
            info!("Deploying app...");
            let docker = session
                .as_docker()
                .expect("Docker session required for app deployment");
            docker.deploy_app(config).await
        }
    }
}

/// Launch the app inside the session, dispatching to the correct platform implementation.
async fn launch_app(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &SessionKind,
    app_path: &str,
    timeout: Duration,
    debug: bool,
) -> Result<(), AppError> {
    match &task_def.app {
        task::AppConfig::MacosTart { .. } => {
            info!("Waiting for stable process baseline...");
            let baseline_procs =
                crate::tart::readiness::get_stable_gui_process_list(session).await?;
            let tart = session
                .as_tart()
                .expect("Tart session required for macOS app launch");
            tart.launch_app(&task_def.app, app_path).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
            info!("Waiting for app window...");
            crate::tart::readiness::wait_for_app_window_macos(
                session,
                &baseline_procs,
                timeout,
                debug,
            )
            .await?;
        }
        task::AppConfig::MacosNative { .. } => {
            info!("Waiting for stable process baseline...");
            let baseline_procs =
                crate::tart::readiness::get_stable_gui_process_list(session).await?;
            let native = session
                .as_native()
                .expect("Native session required for MacosNative app launch");
            native.launch_app(&task_def.app, app_path).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
            info!("Waiting for app window...");
            crate::tart::readiness::wait_for_app_window_macos(
                session,
                &baseline_procs,
                timeout,
                debug,
            )
            .await?;
        }
        task::AppConfig::WindowsVm { .. } => {
            info!("Launching Windows app...");
            let win = session
                .as_windows_vm()
                .expect("Windows VM session required for Windows app launch");
            crate::windows::deploy::launch_app(win, &task_def.app, app_path).await?;
            tokio::time::sleep(Duration::from_secs(3)).await;
            info!("Windows app launched");
        }
        task::AppConfig::WindowsNative { .. } => {
            info!("Launching native Windows app...");
            let win_native = session
                .as_windows_native()
                .expect("Windows native session required for WindowsNative app launch");
            win_native.launch_app(&task_def.app, app_path).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
            info!("Windows app launched");
        }
        task::AppConfig::DockerImage {
            entrypoint_cmd: Some(cmd),
            ..
        } => {
            let baseline_windows = readiness::get_stable_window_list(session).await?;
            info!("Launching app via entrypoint_cmd: {cmd}");
            session
                .exec_detached_with_log(&["bash", "-c", cmd], "/tmp/app.log")
                .await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
            info!("Waiting for app window...");
            readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
        }
        task::AppConfig::DockerImage { .. } => {
            // No entrypoint_cmd — nothing to launch
        }
        _ => {
            let baseline_windows = readiness::get_stable_window_list(session).await?;
            let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
            info!("Launching app: {app_path}");
            let docker = session
                .as_docker()
                .expect("Docker session required for app launch");
            docker
                .launch_app(app_path, is_appimage, config.electron)
                .await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
            info!("Waiting for app window...");
            readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
        }
    }
    Ok(())
}

/// Validate custom Docker image dependencies (only applies to DockerImage tasks).
async fn validate_custom_image(
    task_def: &task::TaskDefinition,
    session: &SessionKind,
) -> Result<(), AppError> {
    if matches!(task_def.app, task::AppConfig::DockerImage { .. }) {
        let docker = session
            .as_docker()
            .expect("Docker session required for custom image validation");
        docker.validate_custom_image().await?;
    }
    Ok(())
}

/// Print session-specific connection info for interactive pause mode.
fn print_connection_info(task_def: &task::TaskDefinition, config: &Config, session: &SessionKind) {
    // VNC info for Docker sessions
    if let Some(port) = config.vnc_port {
        println!(
            "VNC available at {}",
            crate::config::format_host_port(&config.vnc_bind_addr, port)
        );
    }

    println!(
        "\nInteractive mode: session is running with task '{}'.",
        task_def.id
    );
    println!("  Instruction: {}", task_def.instruction);

    match session {
        SessionKind::Docker(docker) => {
            println!("  Container ID: {}", docker.container_id);
            println!("  docker exec -it {} bash", docker.container_id);
        }
        SessionKind::Tart(tart) => {
            println!("  VM name: {}", tart.vm_name());
            println!("  View VM: tart view {}", tart.vm_name());
        }
        SessionKind::Native(_) => {
            println!("  Running on the host desktop (no isolation)");
        }
        SessionKind::WindowsVm(win) => {
            println!("  VM name: {}", win.vm_name());
            println!("  Windows VM running via QEMU");
        }
        SessionKind::WindowsNative(_) => {
            println!("  Running on the Windows host desktop (no isolation)");
        }
    }

    println!("\nPress Ctrl+C to stop and clean up.");
}

/// Interactive mode: start session, run setup steps, print connection info, pause.
async fn run_interactive_pause(
    mut task_def: task::TaskDefinition,
    mut config: Config,
    run: RunConfig,
    artifacts_dir_override: Option<std::path::PathBuf>,
) -> Result<AgentOutcome, AppError> {
    let resolved_secrets = task_def.resolve_secrets()?;
    task_def.apply_secrets(&resolved_secrets)?;
    let redactor = crate::redact::Redactor::new(resolved_secrets.values().cloned());
    config.apply_task_app(&task_def.app);
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    let session = create_session(&task_def, &config, &resolved_secrets, &run).await?;

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_interactive_pause_inner(&task_def, &config, &session, timeout, run.debug, run.no_recording, Some(&redactor)) => r,
    };

    // Always clean up
    let artifacts_dir = match artifacts_dir_override {
        Some(dir) => dir,
        None => std::env::current_dir()
            .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
            .join("desktest_artifacts"),
    };
    maybe_collect_artifacts(&session, &artifacts_dir, &run).await;

    info!("Cleaning up session...");
    let _ = session.cleanup().await;

    result
}

async fn run_interactive_pause_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &SessionKind,
    timeout: Duration,
    debug: bool,
    _no_recording: bool,
    redactor: Option<&crate::redact::Redactor>,
) -> Result<AgentOutcome, AppError> {
    // 1. Wait for desktop
    wait_for_desktop(task_def, session, timeout, debug).await?;

    // 1b. Validate custom image dependencies (after desktop is ready)
    validate_custom_image(task_def, session).await?;

    // 2. Deploy app (before setup steps, so setup can reference deployed files)
    let app_path = deploy_app(task_def, config, session).await?;

    // 3. Run setup steps (after deploy, before app launch)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config, redactor).await?;
    }

    // 4. Launch app
    launch_app(task_def, config, session, &app_path, timeout, debug).await?;

    // 5. Print connection info
    print_connection_info(task_def, config, session);

    // Wait forever until Ctrl+C
    std::future::pending::<()>().await;
    unreachable!()
}

/// Interactive step mode: run agent one step at a time, pausing after each.
async fn run_interactive_step(
    mut task_def: task::TaskDefinition,
    mut config: Config,
    run: RunConfig,
    output_dir: std::path::PathBuf,
    artifacts_dir_override: Option<std::path::PathBuf>,
) -> Result<AgentOutcome, AppError> {
    let start = Instant::now();
    let resolved_secrets = task_def.resolve_secrets()?;
    task_def.apply_secrets(&resolved_secrets)?;
    let redactor = crate::redact::Redactor::new(resolved_secrets.values().cloned());
    config.apply_task_app(&task_def.app);

    let artifacts_dir = match artifacts_dir_override {
        Some(dir) => dir,
        None => std::env::current_dir()
            .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
            .join("desktest_artifacts"),
    };
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    let session = create_session(&task_def, &config, &resolved_secrets, &run).await?;

    let test_id = task_def.id.clone();
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_interactive_step_inner(&task_def, &config, &session, &artifacts_dir, timeout, run.clone(), Some(&redactor)) => r,
    };

    // Collect artifacts and clean up
    maybe_collect_artifacts(&session, &artifacts_dir, &run).await;
    info!("Cleaning up session...");
    let _ = session.cleanup().await;

    let duration_ms = start.elapsed().as_millis() as u64;

    // Write results
    let test_result = match &result {
        Ok(run_result) if !run_result.agent_ran => results::from_evaluation(
            &test_id,
            run_result.eval_result.as_ref().expect("eval_result"),
            duration_ms,
        ),
        Ok(run_result) => results::from_outcome(
            &test_id,
            &run_result.outcome,
            run_result.eval_result.as_ref(),
            duration_ms,
            run.qa,
        ),
        Err(e) => results::from_error(&test_id, e, duration_ms),
    };
    if let Err(e) = results::write_results(&test_result, &output_dir, Some(&redactor)) {
        tracing::warn!("Failed to write results.json: {e}");
    }

    result.map(|r| r.outcome)
}

async fn run_interactive_step_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &SessionKind,
    artifacts_dir: &std::path::Path,
    timeout: Duration,
    run: RunConfig,
    redactor: Option<&crate::redact::Redactor>,
) -> Result<TaskRunResult, AppError> {
    use task::EvaluatorMode;

    // 1. Wait for desktop
    wait_for_desktop(task_def, session, timeout, run.debug).await?;

    // 1b. Validate custom image dependencies (after desktop is ready)
    validate_custom_image(task_def, session).await?;

    // 2. Deploy app (before setup steps, so setup can reference deployed files)
    let app_path = deploy_app(task_def, config, session).await?;

    // 3. Run setup steps (after deploy, before app launch)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config, redactor).await?;
    }

    // 4. Launch app
    launch_app(task_def, config, session, &app_path, timeout, run.debug).await?;

    // Print VNC info
    if let Some(port) = config.vnc_port {
        println!(
            "VNC available at {}",
            crate::config::format_host_port(&config.vnc_bind_addr, port)
        );
    }

    // 5. Start video recording (after app is ready so we skip the boot/setup filler)
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

    // 6. Deploy sandbox script and run agent loop in step mode
    if session.as_native().is_none() {
        agent::pyautogui::deploy_sandbox_script(session).await?;
    }

    info!("Starting agent loop v2 in step mode...");
    let llm_client = provider::create_provider(
        &config.provider,
        &config.api_key,
        &config.model,
        &config.api_base_url,
        config.tls_ca_bundle.as_deref(),
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
        recording.as_ref(),
        None, // no monitor in interactive step mode
        notifier,
    );

    let agent_loop_result = agent_loop.run_step_by_step().await;

    // Stop recording unconditionally (before propagating any error)
    if let Some(rec) = &recording {
        rec.stop(session).await;
        rec.collect(session, artifacts_dir).await;
    }

    let agent_outcome = agent_loop_result?;

    // 7. Run evaluation if needed
    let eval_mode = task_def
        .evaluator
        .as_ref()
        .map(|e| &e.mode)
        .unwrap_or(&EvaluatorMode::Llm);

    let eval_result = if matches!(
        eval_mode,
        EvaluatorMode::Hybrid | EvaluatorMode::Programmatic
    ) {
        info!("Running programmatic evaluation...");
        let evaluator = task_def.evaluator.as_ref().expect("evaluator config");
        Some(evaluator::run_evaluation(session, evaluator, artifacts_dir).await?)
    } else {
        None
    };

    let final_passed = match (&eval_result, eval_mode) {
        (Some(eval), EvaluatorMode::Hybrid) => agent_outcome.passed && eval.passed,
        (Some(eval), EvaluatorMode::Programmatic) => eval.passed,
        _ => agent_outcome.passed,
    };

    print_validation_results(Some(&agent_outcome), eval_result.as_ref());

    Ok(TaskRunResult {
        outcome: AgentOutcome {
            passed: final_passed,
            reasoning: format_evaluation_reasoning(Some(&agent_outcome), eval_result.as_ref()),
            screenshot_count: agent_outcome.screenshot_count,
            bugs_found: agent_outcome.bugs_found,
        },
        eval_result,
        agent_ran: true,
    })
}
