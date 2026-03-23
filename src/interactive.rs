use std::time::{Duration, Instant};

use tracing::info;

use crate::agent;
use crate::artifacts;
use crate::config::Config;
use crate::docker;
use crate::error::{AgentOutcome, AppError};
use crate::evaluator;
use crate::orchestration::{
    TaskRunResult, build_agent_loop_config, format_evaluation_reasoning, print_validation_results,
    resolve_image_name, run_task,
};
use crate::provider;
use crate::readiness;
use crate::recording;
use crate::results;
use crate::setup;
use crate::task;

/// Run the interactive subcommand: starts container, runs setup, provides dev access.
pub(crate) async fn run_interactive(
    task_def: task::TaskDefinition,
    config: Config,
    debug: bool,
    verbose: bool,
    bash_enabled: bool,
    no_recording: bool,
    output_dir: std::path::PathBuf,
    step: bool,
    validate_only: bool,
    qa: bool,
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
            debug,
            verbose,
            bash_enabled,
            no_recording,
            output_dir,
            None,
            qa,
        )
        .await;
    }

    if step {
        // --step: run agent one step at a time, pausing after each
        return run_interactive_step(
            task_def,
            config,
            debug,
            verbose,
            bash_enabled,
            no_recording,
            output_dir,
            qa,
        )
        .await;
    }

    // Default interactive: start container, run setup, print VNC info, pause
    run_interactive_pause(task_def, config, debug, no_recording).await
}

/// Interactive mode: start container, run setup steps, print VNC info, pause.
async fn run_interactive_pause(
    task_def: task::TaskDefinition,
    mut config: Config,
    debug: bool,
    no_recording: bool,
) -> Result<AgentOutcome, AppError> {
    config.apply_task_app(&task_def.app);
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    // Determine custom Docker image from task definition
    let custom_image = match &task_def.app {
        task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
        _ => None,
    };

    info!("Creating Docker container...");
    let session = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C) during container setup");
            return Err(AppError::Infra("Interrupted by user".into()));
        }
        r = async {
            let effective_image = resolve_image_name(&config, custom_image).await?;
            docker::DockerSession::create(&config, effective_image).await
        } => r?,
    };

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_interactive_pause_inner(&task_def, &config, &session, timeout, debug, no_recording) => r,
    };

    // Always clean up
    info!("Collecting artifacts...");
    let artifacts_dir = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
        .join("desktest_artifacts");
    let _ = artifacts::collect_artifacts(&session, &artifacts_dir).await;

    info!("Cleaning up container...");
    let _ = session.cleanup().await;

    result
}

async fn run_interactive_pause_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &docker::DockerSession,
    timeout: Duration,
    debug: bool,
    _no_recording: bool,
) -> Result<AgentOutcome, AppError> {
    // 1. Wait for desktop
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 1b. Validate custom image dependencies (after desktop is ready)
    let is_docker_image = matches!(task_def.app, task::AppConfig::DockerImage { .. });
    if is_docker_image {
        session.validate_custom_image().await?;
    }

    // 2. Deploy app (before setup steps, so setup can reference deployed files)
    let app_path = if is_docker_image {
        info!("Custom Docker image: skipping app deployment");
        String::new()
    } else {
        info!("Deploying app...");
        session.deploy_app(config).await?
    };

    // 3. Run setup steps (after deploy, before app launch)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config).await?;
    }

    // 4. Launch app
    if is_docker_image {
        if let task::AppConfig::DockerImage { entrypoint_cmd, .. } = &task_def.app {
            if let Some(cmd) = entrypoint_cmd {
                info!("Launching app via entrypoint_cmd: {cmd}");
                session
                    .exec_detached_with_log(&["bash", "-c", cmd], "/tmp/app.log")
                    .await?;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    } else {
        let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
        info!("Launching app: {app_path}");
        session
            .launch_app(&app_path, is_appimage, config.electron)
            .await?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // 5. Print VNC info and container info
    if let Some(port) = config.vnc_port {
        println!("VNC available at {}:{}", config.vnc_bind_addr, port);
    }

    println!(
        "\nInteractive mode: container is running with task '{}'.",
        task_def.id
    );
    println!("  Instruction: {}", task_def.instruction);
    println!("  Container ID: {}", session.container_id);
    println!("  docker exec -it {} bash", session.container_id);
    println!("\nPress Ctrl+C to stop and clean up.");

    // Wait forever until Ctrl+C
    std::future::pending::<()>().await;
    unreachable!()
}

/// Interactive step mode: run agent one step at a time, pausing after each.
async fn run_interactive_step(
    task_def: task::TaskDefinition,
    mut config: Config,
    debug: bool,
    verbose: bool,
    bash_enabled: bool,
    no_recording: bool,
    output_dir: std::path::PathBuf,
    qa: bool,
) -> Result<AgentOutcome, AppError> {
    let start = Instant::now();
    config.apply_task_app(&task_def.app);

    let artifacts_dir = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot get cwd: {e}")))?
        .join("desktest_artifacts");
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    let custom_image = match &task_def.app {
        task::AppConfig::DockerImage { image, .. } => Some(image.as_str()),
        _ => None,
    };

    info!("Creating Docker container...");
    let session = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C) during container setup");
            return Err(AppError::Infra("Interrupted by user".into()));
        }
        r = async {
            let effective_image = resolve_image_name(&config, custom_image).await?;
            docker::DockerSession::create(&config, effective_image).await
        } => r?,
    };

    let test_id = task_def.id.clone();
    let timeout = Duration::from_secs(config.startup_timeout_seconds);

    let result = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted (Ctrl+C), cleaning up...");
            Err(AppError::Infra("Interrupted by user".into()))
        }
        r = run_interactive_step_inner(&task_def, &config, &session, &artifacts_dir, timeout, debug, verbose, bash_enabled, no_recording, qa) => r,
    };

    // Collect artifacts and clean up
    info!("Collecting artifacts...");
    let _ = artifacts::collect_artifacts(&session, &artifacts_dir).await;
    info!("Cleaning up container...");
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
            qa,
        ),
        Err(e) => results::from_error(&test_id, e, duration_ms),
    };
    if let Err(e) = results::write_results(&test_result, &output_dir) {
        tracing::warn!("Failed to write results.json: {e}");
    }

    result.map(|r| r.outcome)
}

async fn run_interactive_step_inner(
    task_def: &task::TaskDefinition,
    config: &Config,
    session: &docker::DockerSession,
    artifacts_dir: &std::path::Path,
    timeout: Duration,
    debug: bool,
    verbose: bool,
    bash_enabled: bool,
    no_recording: bool,
    qa: bool,
) -> Result<TaskRunResult, AppError> {
    use task::EvaluatorMode;

    // 1. Wait for desktop
    info!("Waiting for desktop to be ready...");
    readiness::wait_for_desktop(session, timeout, debug).await?;

    // 1b. Validate custom image dependencies (after desktop is ready)
    let is_docker_image = matches!(task_def.app, task::AppConfig::DockerImage { .. });
    if is_docker_image {
        session.validate_custom_image().await?;
    }

    // 2. Deploy app (before setup steps, so setup can reference deployed files)
    let app_path = if is_docker_image {
        info!("Custom Docker image: skipping app deployment");
        String::new()
    } else {
        info!("Deploying app...");
        session.deploy_app(config).await?
    };

    // 3. Run setup steps (after deploy, before app launch)
    if !task_def.config.is_empty() {
        info!("Running {} setup steps...", task_def.config.len());
        setup::run_setup_steps(session, &task_def.config).await?;
    }

    // 4. Launch app
    if is_docker_image {
        if let task::AppConfig::DockerImage { entrypoint_cmd, .. } = &task_def.app {
            if let Some(cmd) = entrypoint_cmd {
                let baseline_windows = readiness::get_stable_window_list(session).await?;
                info!("Launching app via entrypoint_cmd: {cmd}");
                session
                    .exec_detached_with_log(&["bash", "-c", cmd], "/tmp/app.log")
                    .await?;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                info!("Waiting for app window...");
                readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
            }
        }
    } else {
        let baseline_windows = readiness::get_stable_window_list(session).await?;
        let is_appimage = matches!(config.app_type, crate::config::AppType::Appimage);
        info!("Launching app: {app_path}");
        session
            .launch_app(&app_path, is_appimage, config.electron)
            .await?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        info!("Waiting for app window...");
        readiness::wait_for_app_window(session, &baseline_windows, timeout, debug).await?;
    }

    // Print VNC info
    if let Some(port) = config.vnc_port {
        println!("VNC available at {}:{}", config.vnc_bind_addr, port);
    }

    // 4. Start video recording (after app is ready so we skip the boot/setup filler)
    let recording = if no_recording {
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

    // 5. Run agent loop in step mode
    info!("Starting agent loop v2 in step mode...");
    let llm_client = provider::create_provider(
        &config.provider,
        &config.api_key,
        &config.model,
        &config.api_base_url,
    )?;

    let mut loop_config =
        build_agent_loop_config(&task_def, session, debug, verbose, bash_enabled).await;
    loop_config.qa = qa;
    let full_instruction = task_def.full_instruction();
    let mut agent_loop = agent::loop_v2::AgentLoopV2::new(
        llm_client,
        session,
        artifacts_dir.to_path_buf(),
        &full_instruction,
        config.display_width,
        config.display_height,
        loop_config,
        recording.as_ref(),
        None, // no monitor in interactive step mode
        task_def.id.clone(),
    );

    let agent_loop_result = agent_loop.run_step_by_step().await;

    // Stop recording unconditionally (before propagating any error)
    if let Some(rec) = &recording {
        rec.stop(session).await;
        rec.collect(session, artifacts_dir).await;
    }

    let agent_outcome = agent_loop_result?;

    // 6. Run evaluation if needed
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
