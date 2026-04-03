use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::error::AppError;
use crate::task::{
    AppConfig, CompareMode, Conjunction, EvaluatorConfig, EvaluatorMode, MatchMode, MetricConfig,
    TaskDefinition,
};

/// Supported app types for `desktest init`.
const APP_TYPES: &[&str] = &[
    "appimage",
    "folder",
    "docker_image",
    "macos_tart",
    "macos_native",
    "windows_vm",
    "windows_native",
];

/// Run the `desktest init` command: scaffold a minimal task JSON.
pub fn run_init(output: &Path, app_type: Option<&str>) -> Result<(), AppError> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    let app_type = match app_type {
        Some(t) => {
            if !APP_TYPES.contains(&t) {
                return Err(AppError::Config(format!(
                    "Unknown app type '{t}'. Valid types: {}",
                    APP_TYPES.join(", ")
                )));
            }
            t.to_string()
        }
        None => prompt_choice(&mut reader, "App type", APP_TYPES, None)?,
    };

    let id = prompt_string(
        &mut reader,
        "Task ID",
        Some(&default_id_from_output(output)),
    )?;
    let instruction = prompt_string(&mut reader, "Instruction (what should the agent do?)", None)?;
    let completion_condition = prompt_optional(
        &mut reader,
        "Completion condition (optional, press Enter to skip)",
    )?;

    let app = build_app_config(&mut reader, &app_type)?;

    let add_evaluator = prompt_yes_no(&mut reader, "Add a programmatic evaluator?", false)?;
    let evaluator = if add_evaluator {
        Some(build_evaluator(&mut reader)?)
    } else {
        None
    };

    let task = TaskDefinition::new_scaffold(id, instruction, completion_condition, app, evaluator);

    let json = serde_json::to_string_pretty(&task)
        .map_err(|e| AppError::Config(format!("Failed to serialize task JSON: {e}")))?;

    if output.exists() {
        let overwrite = prompt_yes_no(
            &mut reader,
            &format!("{} already exists. Overwrite?", output.display()),
            false,
        )?;
        if !overwrite {
            println!("Aborted.");
            return Ok(());
        }
    }

    std::fs::write(output, format!("{json}\n"))
        .map_err(|e| AppError::Config(format!("Failed to write {}: {e}", output.display())))?;

    println!("\nCreated {}", output.display());
    println!("\nNext steps:");
    println!("  desktest validate {}", output.display());
    println!("  desktest run {} --monitor", output.display());

    Ok(())
}

fn default_id_from_output(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("my-test")
        .to_string()
}

fn build_app_config(reader: &mut impl BufRead, app_type: &str) -> Result<AppConfig, AppError> {
    match app_type {
        "appimage" => {
            let path = prompt_string(reader, "Path to AppImage file", None)?;
            let electron = prompt_yes_no(reader, "Is this an Electron app?", false)?;
            Ok(AppConfig::Appimage { path, electron })
        }
        "folder" => {
            let dir = prompt_string(reader, "App directory path", None)?;
            let entrypoint = prompt_string(reader, "Entrypoint script (e.g., start.sh)", None)?;
            let electron = prompt_yes_no(reader, "Is this an Electron app?", false)?;
            Ok(AppConfig::Folder {
                dir,
                entrypoint,
                electron,
            })
        }
        "docker_image" => {
            let image = prompt_string(reader, "Docker image name (e.g., my-app:latest)", None)?;
            let entrypoint_cmd = prompt_optional(
                reader,
                "Custom entrypoint command (optional, press Enter to skip)",
            )?;
            Ok(AppConfig::DockerImage {
                image,
                digest: None,
                entrypoint_cmd,
                needs_fuse: false,
            })
        }
        "macos_tart" => {
            let base_image =
                prompt_string(reader, "Tart base image", Some("desktest-macos:latest"))?;
            let bundle_id = prompt_optional(
                reader,
                "Bundle ID (e.g., com.apple.TextEdit, or press Enter to skip)",
            )?;
            let app_path = prompt_optional(reader, "App path (optional, press Enter to skip)")?;
            let launch_cmd =
                prompt_optional(reader, "Launch command (optional, press Enter to skip)")?;
            let electron = prompt_yes_no(reader, "Is this an Electron app?", false)?;
            Ok(AppConfig::MacosTart {
                base_image,
                bundle_id,
                app_path,
                launch_cmd,
                electron,
            })
        }
        "macos_native" => {
            let bundle_id = prompt_optional(
                reader,
                "Bundle ID (e.g., com.apple.TextEdit, or press Enter to skip)",
            )?;
            let app_path = prompt_optional(reader, "App path (optional, press Enter to skip)")?;
            if bundle_id.is_none() && app_path.is_none() {
                return Err(AppError::Config(
                    "MacosNative app: at least one of 'bundle_id' or 'app_path' must be provided."
                        .into(),
                ));
            }
            Ok(AppConfig::MacosNative {
                bundle_id,
                app_path,
            })
        }
        "windows_vm" => {
            let base_image = prompt_string(
                reader,
                "QCOW2 golden image path",
                Some("desktest-windows.qcow2"),
            )?;
            let app_path =
                prompt_optional(reader, "App path to deploy (optional, press Enter to skip)")?;
            let launch_cmd = prompt_optional(
                reader,
                "Launch command (e.g., calc.exe, or press Enter to skip)",
            )?;
            let installer_cmd =
                prompt_optional(reader, "Installer command (optional, press Enter to skip)")?;
            Ok(AppConfig::WindowsVm {
                base_image,
                app_path,
                launch_cmd,
                installer_cmd,
            })
        }
        "windows_native" => {
            let app_path = prompt_optional(reader, "App path (optional, press Enter to skip)")?;
            let launch_cmd = prompt_optional(
                reader,
                "Launch command (e.g., notepad.exe, or press Enter to skip)",
            )?;
            if app_path.is_none() && launch_cmd.is_none() {
                return Err(AppError::Config(
                    "WindowsNative app: at least one of 'app_path' or 'launch_cmd' must be provided."
                        .into(),
                ));
            }
            Ok(AppConfig::WindowsNative {
                app_path,
                launch_cmd,
            })
        }
        _ => Err(AppError::Config(format!("Unknown app type: {app_type}"))),
    }
}

fn build_evaluator(reader: &mut impl BufRead) -> Result<EvaluatorConfig, AppError> {
    let mode_options = &["llm", "programmatic", "hybrid"];
    let mode_str = prompt_choice(reader, "Evaluator mode", mode_options, Some("hybrid"))?;
    let mode = match mode_str.as_str() {
        "llm" => EvaluatorMode::Llm,
        "programmatic" => EvaluatorMode::Programmatic,
        "hybrid" => EvaluatorMode::Hybrid,
        _ => EvaluatorMode::Hybrid,
    };

    let mut metrics = Vec::new();
    if mode != EvaluatorMode::Llm {
        loop {
            let metric_types = &[
                "file_compare",
                "command_output",
                "file_exists",
                "exit_code",
                "done (no more metrics)",
            ];
            let choice = prompt_choice(reader, "Add metric", metric_types, None)?;
            match choice.as_str() {
                "file_compare" => {
                    let actual_path =
                        prompt_string(reader, "Actual file path (in container)", None)?;
                    let expected_path =
                        prompt_string(reader, "Expected file path (on host)", None)?;
                    metrics.push(MetricConfig::FileCompare {
                        actual_path,
                        expected_path,
                        compare_mode: CompareMode::Normalized,
                    });
                }
                "command_output" => {
                    let command = prompt_string(reader, "Command to run", None)?;
                    let expected = prompt_string(reader, "Expected output", None)?;
                    metrics.push(MetricConfig::CommandOutput {
                        command,
                        expected,
                        match_mode: MatchMode::Contains,
                    });
                }
                "file_exists" => {
                    let path = prompt_string(reader, "File path to check", None)?;
                    metrics.push(MetricConfig::FileExists {
                        path,
                        should_not_exist: false,
                    });
                }
                "exit_code" => {
                    let command = prompt_string(reader, "Command to run", None)?;
                    let code_str = prompt_string(reader, "Expected exit code", Some("0"))?;
                    let expected = code_str.parse::<i32>().unwrap_or(0);
                    metrics.push(MetricConfig::ExitCode { command, expected });
                }
                _ => break,
            }
        }
    }

    Ok(EvaluatorConfig {
        mode,
        metrics,
        conjunction: Conjunction::And,
        eval_timeout_secs: None,
    })
}

// ── Prompt helpers ──────────────────────────────────────────────────────────

fn prompt_string(
    reader: &mut impl BufRead,
    label: &str,
    default: Option<&str>,
) -> Result<String, AppError> {
    loop {
        if let Some(d) = default {
            eprint!("  {label} [{d}]: ");
        } else {
            eprint!("  {label}: ");
        }
        io::stderr().flush().ok();

        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|e| AppError::Config(format!("Failed to read input: {e}")))?;
        if bytes == 0 {
            return Err(AppError::Config("Unexpected end of input (EOF)".into()));
        }
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if let Some(d) = default {
                return Ok(d.to_string());
            }
            eprintln!("    (required)");
            continue;
        }
        return Ok(trimmed.to_string());
    }
}

fn prompt_optional(reader: &mut impl BufRead, label: &str) -> Result<Option<String>, AppError> {
    eprint!("  {label}: ");
    io::stderr().flush().ok();

    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|e| AppError::Config(format!("Failed to read input: {e}")))?;
    if bytes == 0 {
        return Ok(None);
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn prompt_yes_no(reader: &mut impl BufRead, label: &str, default: bool) -> Result<bool, AppError> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    eprint!("  {label} {hint}: ");
    io::stderr().flush().ok();

    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|e| AppError::Config(format!("Failed to read input: {e}")))?;
    if bytes == 0 {
        return Ok(default);
    }
    let trimmed = line.trim().to_lowercase();
    if trimmed.is_empty() {
        Ok(default)
    } else {
        Ok(trimmed.starts_with('y'))
    }
}

fn prompt_choice(
    reader: &mut impl BufRead,
    label: &str,
    options: &[&str],
    default: Option<&str>,
) -> Result<String, AppError> {
    eprintln!("  {label}:");
    for (i, opt) in options.iter().enumerate() {
        let marker = match default {
            Some(d) if d == *opt => " (default)",
            _ => "",
        };
        eprintln!("    {}: {opt}{marker}", i + 1);
    }
    loop {
        if let Some(d) = default {
            eprint!("  Choice [{d}]: ");
        } else {
            eprint!("  Choice: ");
        }
        io::stderr().flush().ok();

        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|e| AppError::Config(format!("Failed to read input: {e}")))?;
        if bytes == 0 {
            return Err(AppError::Config("Unexpected end of input (EOF)".into()));
        }
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if let Some(d) = default {
                return Ok(d.to_string());
            }
            eprintln!("    (required)");
            continue;
        }

        // Accept number or name
        if let Ok(n) = trimmed.parse::<usize>() {
            if n >= 1 && n <= options.len() {
                return Ok(options[n - 1].to_string());
            }
        }
        if options.contains(&trimmed) {
            return Ok(trimmed.to_string());
        }
        eprintln!(
            "    Invalid choice. Enter a number (1-{}) or name.",
            options.len()
        );
    }
}
