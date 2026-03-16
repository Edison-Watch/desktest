//! Generate a self-contained HTML trajectory viewer.
//!
//! Reads trajectory.jsonl and screenshots from an artifacts directory,
//! embeds everything into a single HTML file with inline CSS and vanilla JS.

use std::path::Path;

use tracing::info;

use crate::error::AppError;
use crate::codify::{load_trajectory, TrajectoryRecord};

/// Generate a self-contained HTML review file from test artifacts.
pub fn generate_review_html(
    artifacts_dir: &Path,
    output_path: &Path,
) -> Result<(), AppError> {
    let trajectory_path = artifacts_dir.join("trajectory.jsonl");
    if !trajectory_path.exists() {
        return Err(AppError::Config(format!(
            "No trajectory.jsonl found in '{}'",
            artifacts_dir.display()
        )));
    }

    let entries = load_trajectory(&trajectory_path)?;

    // Load screenshots as base64
    let steps_json = build_steps_json(&entries, artifacts_dir);

    // Check for recording
    let has_recording = artifacts_dir.join("recording.mp4").exists();

    let html = build_html(&steps_json, has_recording);

    std::fs::write(output_path, &html)
        .map_err(|e| AppError::Infra(format!("Cannot write review HTML: {e}")))?;

    info!("Review HTML written to {}", output_path.display());
    Ok(())
}

/// Build JSON array of step data with embedded screenshots.
fn build_steps_json(entries: &[TrajectoryRecord], artifacts_dir: &Path) -> String {
    let steps: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            let screenshot_b64 = entry.screenshot_path.as_ref().and_then(|p| {
                let full_path = artifacts_dir.join(p);
                std::fs::read(&full_path).ok().map(|bytes| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                })
            });

            serde_json::json!({
                "step": entry.step,
                "timestamp": entry.timestamp,
                "thought": entry.thought,
                "action_code": entry.action_code,
                "result": entry.result,
                "screenshot": screenshot_b64,
            })
        })
        .collect();

    serde_json::to_string(&steps)
        .unwrap_or_else(|_| "[]".to_string())
        .replace("</", "<\\/")
}

/// Build the complete HTML document.
fn build_html(steps_json: &str, has_recording: bool) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>eyetest - Trajectory Review</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif; background: #0f1117; color: #e1e4e8; display: flex; height: 100vh; }}
.sidebar {{ width: 350px; background: #161b22; border-right: 1px solid #30363d; overflow-y: auto; flex-shrink: 0; }}
.sidebar h2 {{ padding: 16px; font-size: 14px; color: #8b949e; text-transform: uppercase; letter-spacing: 0.5px; border-bottom: 1px solid #30363d; }}
.step-item {{ padding: 12px 16px; border-bottom: 1px solid #21262d; cursor: pointer; transition: background 0.15s; }}
.step-item:hover {{ background: #1c2128; }}
.step-item.active {{ background: #1f6feb22; border-left: 3px solid #1f6feb; }}
.step-num {{ font-weight: 600; font-size: 13px; color: #58a6ff; }}
.step-thought {{ font-size: 12px; color: #8b949e; margin-top: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
.badge {{ display: inline-block; padding: 2px 8px; border-radius: 12px; font-size: 11px; font-weight: 600; margin-left: 8px; }}
.badge.success {{ background: #23882344; color: #3fb950; }}
.badge.done {{ background: #1f6feb22; color: #58a6ff; }}
.badge.error {{ background: #f8514944; color: #f85149; }}
.badge.fail {{ background: #f8514944; color: #f85149; }}
.main {{ flex: 1; overflow-y: auto; padding: 24px; }}
.detail-header {{ display: flex; align-items: center; gap: 12px; margin-bottom: 20px; }}
.detail-header h2 {{ font-size: 20px; }}
.screenshot {{ max-width: 100%; border-radius: 8px; border: 1px solid #30363d; margin-bottom: 20px; }}
.section {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px; margin-bottom: 16px; }}
.section h3 {{ font-size: 13px; color: #8b949e; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 12px; }}
.section pre {{ font-family: 'SF Mono', 'Fira Code', monospace; font-size: 13px; white-space: pre-wrap; word-break: break-all; color: #c9d1d9; }}
.thought-text {{ font-size: 14px; line-height: 1.6; color: #c9d1d9; }}
.codify-bar {{ padding: 12px 16px; border-top: 1px solid #30363d; background: #161b22; }}
.codify-bar label {{ font-size: 12px; color: #8b949e; cursor: pointer; }}
.codify-bar input[type=checkbox] {{ margin-right: 6px; }}
.codify-btn {{ display: block; width: 100%; margin-top: 12px; padding: 8px; background: #238636; color: white; border: none; border-radius: 6px; font-size: 13px; cursor: pointer; font-weight: 600; }}
.codify-btn:hover {{ background: #2ea043; }}
.empty {{ display: flex; align-items: center; justify-content: center; height: 100%; color: #484f58; font-size: 16px; }}
.recording-note {{ margin-bottom: 16px; padding: 12px; background: #161b22; border: 1px solid #30363d; border-radius: 8px; color: #8b949e; font-size: 13px; }}
</style>
</head>
<body>
<div class="sidebar">
  <h2>Steps</h2>
  <div id="step-list"></div>
  <div class="codify-bar">
    <div id="checkbox-list"></div>
    <button class="codify-btn" onclick="copyCodifyCommand()">Copy codify command</button>
  </div>
</div>
<div class="main" id="main-panel">
  <div class="empty">Select a step to view details</div>
</div>

<script>
const STEPS = {steps_json};
const HAS_RECORDING = {has_recording};

const stepList = document.getElementById('step-list');
const checkboxList = document.getElementById('checkbox-list');
const mainPanel = document.getElementById('main-panel');

function badgeClass(result) {{
  if (result === 'success') return 'success';
  if (result === 'done') return 'done';
  if (result.startsWith('error') || result === 'fail') return 'error';
  return 'fail';
}}

STEPS.forEach((s, i) => {{
  // Step list item
  const div = document.createElement('div');
  div.className = 'step-item';
  div.dataset.index = i;
  div.innerHTML = `<span class="step-num">Step ${{s.step}}</span><span class="badge ${{badgeClass(s.result)}}">${{escapeHtml(s.result)}}</span><div class="step-thought">${{escapeHtml(s.thought || '(no thought)')}}</div>`;
  div.addEventListener('click', () => selectStep(i));
  stepList.appendChild(div);

  // Checkbox
  const label = document.createElement('label');
  label.style.display = 'block';
  label.style.marginBottom = '4px';
  label.innerHTML = `<input type="checkbox" value="${{s.step}}" ${{s.result === 'success' ? 'checked' : ''}}> Step ${{s.step}}`;
  checkboxList.appendChild(label);
}});

function selectStep(index) {{
  document.querySelectorAll('.step-item').forEach(el => el.classList.remove('active'));
  document.querySelector(`.step-item[data-index="${{index}}"]`).classList.add('active');

  const s = STEPS[index];
  let html = '';
  if (HAS_RECORDING) {{
    html += '<div class="recording-note">A session recording (recording.mp4) is available in the artifacts directory.</div>';
  }}
  html += `<div class="detail-header"><h2>Step ${{s.step}}</h2><span class="badge ${{badgeClass(s.result)}}">${{escapeHtml(s.result)}}</span><span style="color:#484f58;font-size:13px">${{escapeHtml(s.timestamp)}}</span></div>`;

  if (s.screenshot) {{
    html += `<img class="screenshot" src="data:image/png;base64,${{s.screenshot}}" alt="Step ${{s.step}} screenshot">`;
  }}

  if (s.thought) {{
    html += `<div class="section"><h3>Thought</h3><div class="thought-text">${{escapeHtml(s.thought)}}</div></div>`;
  }}

  if (s.action_code) {{
    html += `<div class="section"><h3>Action Code</h3><pre>${{escapeHtml(s.action_code)}}</pre></div>`;
  }}

  mainPanel.innerHTML = html;
}}

function escapeHtml(text) {{
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}}

function copyCodifyCommand() {{
  const checked = [...document.querySelectorAll('#checkbox-list input:checked')].map(cb => cb.value);
  if (checked.length === 0) {{
    alert('Select at least one step to include.');
    return;
  }}
  const cmd = `eyetest codify trajectory.jsonl --steps ${{checked.join(',')}}`;
  if (navigator.clipboard) {{
    navigator.clipboard.writeText(cmd).then(() => {{
      const btn = document.querySelector('.codify-btn');
      btn.textContent = 'Copied!';
      setTimeout(() => btn.textContent = 'Copy codify command', 2000);
    }}).catch(() => {{
      prompt('Copy this command:', cmd);
    }});
  }} else {{
    prompt('Copy this command:', cmd);
  }}
}}

if (STEPS.length > 0) selectStep(0);
</script>
</body>
</html>"##, steps_json = steps_json, has_recording = if has_recording { "true" } else { "false" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_html_contains_structure() {
        let html = build_html("[]", false);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("eyetest"));
        assert!(html.contains("Trajectory Review"));
        assert!(html.contains("STEPS"));
    }

    #[test]
    fn test_build_steps_json_empty() {
        let dir = tempfile::tempdir().unwrap();
        let json = build_steps_json(&[], dir.path());
        assert_eq!(json, "[]");
    }

    #[test]
    fn test_build_steps_json_with_entry() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![TrajectoryRecord {
            step: 1,
            timestamp: "2026-01-01T00:00:00Z".into(),
            action_code: "pyautogui.click(100, 200)".into(),
            thought: Some("Click button".into()),
            screenshot_path: None,
            result: "success".into(),
        }];
        let json = build_steps_json(&entries, dir.path());
        assert!(json.contains("Click button"));
        assert!(json.contains("pyautogui.click"));
    }

    #[test]
    fn test_generate_review_html() {
        let dir = tempfile::tempdir().unwrap();
        let trajectory = dir.path().join("trajectory.jsonl");
        std::fs::write(&trajectory, "{\"step\":1,\"timestamp\":\"2026-01-01T00:00:00Z\",\"action_code\":\"pyautogui.click(100,200)\",\"result\":\"success\"}\n").unwrap();

        let output = dir.path().join("review.html");
        generate_review_html(dir.path(), &output).unwrap();

        let html = std::fs::read_to_string(&output).unwrap();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("pyautogui.click"));
    }

    #[test]
    fn test_generate_review_no_trajectory() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("review.html");
        let err = generate_review_html(dir.path(), &output).unwrap_err();
        assert!(err.to_string().contains("trajectory.jsonl"));
    }
}
