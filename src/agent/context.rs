//! Sliding window context management for the OSWorld-style agent loop.
//!
//! Reconstructs the LLM message array each call from:
//! - System prompt (with action space definition and display dimensions)
//! - Task instruction
//! - Sliding window of recent trajectory turns
//! - Current observation (screenshot + a11y tree)

use crate::observation::Observation;
use crate::provider::{ChatMessage, system_message, user_image_message, user_message};

/// Platform the agent is running on, used to tailor the system prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

/// Default number of recent trajectory turns to keep.
pub const DEFAULT_MAX_TRAJECTORY_LENGTH: usize = 6;

/// A single turn in the agent trajectory (one observe-think-act cycle).
#[derive(Debug, Clone)]
pub struct TrajectoryTurn {
    /// The observation at this step (screenshot data URL and/or a11y tree).
    pub observation: Observation,
    /// The agent's raw response text (reflection + code).
    pub response_text: String,
    /// Error feedback from action execution (if any).
    pub error_feedback: Option<String>,
    /// Captured bash command output (if any bash blocks were executed).
    pub bash_output: Option<String>,
}

/// Manages sliding window context for the v2 agent loop.
pub struct ContextManager {
    /// Full system prompt (with action space, display dimensions, etc.)
    system_prompt: String,
    /// Task instruction for the agent.
    instruction: String,
    /// Maximum number of recent turns to keep in the trajectory window.
    max_trajectory_length: usize,
    /// The trajectory of previous turns.
    trajectory: Vec<TrajectoryTurn>,
}

impl ContextManager {
    /// Create a new context manager with the given configuration.
    pub fn new(
        display_width: u32,
        display_height: u32,
        instruction: &str,
        max_trajectory_length: usize,
        bash_enabled: bool,
        qa: bool,
        platform: Platform,
    ) -> Self {
        let system_prompt =
            build_system_prompt(display_width, display_height, bash_enabled, qa, platform);
        Self {
            system_prompt,
            instruction: instruction.to_string(),
            max_trajectory_length,
            trajectory: Vec::new(),
        }
    }

    /// Record a completed turn in the trajectory.
    pub fn push_turn(&mut self, turn: TrajectoryTurn) {
        self.trajectory.push(turn);
    }

    /// Build the message array for the next LLM call.
    ///
    /// The array is reconstructed each call:
    /// 1. System prompt (action space + display dimensions)
    /// 2. Task instruction
    /// 3. Recent trajectory turns (sliding window of last N)
    /// 4. Current observation
    pub fn build_messages(&self, current_observation: &Observation) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // 1. System prompt
        messages.push(system_message(&self.system_prompt));

        // 2. Task instruction
        messages.push(user_message(&format!(
            "## Task\n\n{}\n\nPlease complete the task above. \
             Reflect on the current screenshot before taking action.",
            self.instruction
        )));

        // 3. Sliding window of recent trajectory turns
        let window_start = self
            .trajectory
            .len()
            .saturating_sub(self.max_trajectory_length);
        for turn in &self.trajectory[window_start..] {
            // Previous observation
            let obs_msg = observation_to_message(&turn.observation);
            messages.extend(obs_msg);

            // Previous agent response
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: Some(serde_json::Value::String(turn.response_text.clone())),
                tool_calls: None,
                tool_call_id: None,
            });

            // Bash output (if any)
            if let Some(ref output) = turn.bash_output {
                messages.push(user_message(&format!("Bash command output:\n{output}")));
            }

            // Error feedback (if any)
            if let Some(ref feedback) = turn.error_feedback {
                messages.push(user_message(&format!("Action execution error: {feedback}")));
            }
        }

        // 4. Current observation
        let current_obs_msgs = observation_to_message(current_observation);
        messages.extend(current_obs_msgs);

        messages
    }

    /// Build a fallback message array with only the system prompt and current observation.
    ///
    /// Used when a `context_length_exceeded` error occurs — drops the entire
    /// trajectory to fit within the model's context window.
    pub fn build_fallback_messages(&self, current_observation: &Observation) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // System prompt
        messages.push(system_message(&self.system_prompt));

        // Instruction
        messages.push(user_message(&format!(
            "## Task\n\n{}\n\nNote: Previous conversation history was dropped due to \
             context length limits. Please continue from the current observation.\n\n\
             Please complete the task above. Reflect on the current screenshot before taking action.",
            self.instruction
        )));

        // Current observation only (no trajectory)
        let current_obs_msgs = observation_to_message(current_observation);
        messages.extend(current_obs_msgs);

        messages
    }

    /// Get the number of turns in the trajectory.
    #[cfg(test)]
    pub fn trajectory_len(&self) -> usize {
        self.trajectory.len()
    }

    /// Clear the trajectory (e.g., after a context length fallback).
    pub fn clear_trajectory(&mut self) {
        self.trajectory.clear();
    }
}

/// Convert an Observation into one or more user messages for the LLM.
///
/// Loads the screenshot data URL on demand from disk. Returns a Vec because
/// we may need separate messages for image and text content, or a combined
/// message with both.
fn observation_to_message(observation: &Observation) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    // Lazy-load: read the screenshot from disk only when building LLM messages
    let data_url = observation.load_screenshot_data_url().and_then(|r| {
        r.map_err(|e| {
            tracing::warn!("Failed to load screenshot for LLM message: {e}");
        })
        .ok()
    });

    match (data_url.as_deref(), &observation.a11y_tree_text) {
        (Some(data_url), Some(a11y_text)) => {
            // Combined: screenshot image + a11y tree text in a single user message
            messages.push(ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!([
                    {
                        "type": "image_url",
                        "image_url": { "url": data_url }
                    },
                    {
                        "type": "text",
                        "text": format!(
                            "Here is the current accessibility tree of the desktop:\n\n```\n{}\n```",
                            a11y_text
                        )
                    }
                ])),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        (Some(data_url), None) => {
            // Screenshot only
            messages.push(user_image_message(data_url));
        }
        (None, Some(a11y_text)) => {
            // A11y tree only
            messages.push(user_message(&format!(
                "Here is the current accessibility tree of the desktop:\n\n```\n{}\n```",
                a11y_text
            )));
        }
        (None, None) => {
            // No observation data (shouldn't normally happen)
            messages.push(user_message(
                "[No observation available — screenshot and accessibility tree both unavailable]",
            ));
        }
    }

    messages
}

/// Build the OSWorld-style system prompt with full action space definition.
///
/// Includes:
/// - Role description
/// - Action space (PyAutoGUI API reference)
/// - Output format (reflection + code block)
/// - Special commands (DONE, FAIL, WAIT)
/// - Display dimensions
/// - Coordinate system
pub fn build_system_prompt(
    display_width: u32,
    display_height: u32,
    bash_enabled: bool,
    qa: bool,
    platform: Platform,
) -> String {
    let pt = PlatformText::for_platform(platform);
    let bash_section = build_bash_section(bash_enabled);
    let qa_section = build_qa_section(qa, platform);
    let bug_command_line = if qa {
        "\n- **BUG** — (QA mode) Report an application bug you discovered. Describe the issue on the following lines, then continue your task. Can co-exist with DONE/FAIL in the same response."
    } else {
        ""
    };

    let max_x = display_width.saturating_sub(1);
    let max_y = display_height.saturating_sub(1);

    format!(
        r#"You are a professional software tester controlling {platform_desc}. Your task is to interact with the desktop GUI to complete a given objective.

## Display Information

- Screen resolution: {display_width}x{display_height} pixels
- Coordinate system: (0, 0) is the top-left corner; ({max_x}, {max_y}) is the bottom-right corner
- {display_desc}

## Action Space

You interact with the desktop using PyAutoGUI Python code. The following modules and functions are pre-imported and available:
- `pyautogui` — GUI automation (mouse, keyboard, screenshots)
- `time` — time utilities (sleep, etc.)
- `pyperclip` — clipboard access (copy/paste){type_text_import}

### Mouse Actions
- `pyautogui.click(x, y)` — left click at coordinates
- `pyautogui.rightClick(x, y)` — right click at coordinates
- `pyautogui.doubleClick(x, y)` — double click at coordinates
- `pyautogui.moveTo(x, y)` — move mouse to coordinates
- `pyautogui.scroll(clicks, x=None, y=None)` — scroll (positive=up, negative=down)
- `pyautogui.mouseDown(x, y, button='left')` — press mouse button down
- `pyautogui.mouseUp(x, y, button='left')` — release mouse button
- `pyautogui.drag(dx, dy, duration=0.5)` — drag from current position

### Keyboard Actions
- `pyautogui.typewrite('text', interval=0.05)` — type text (ASCII only, one char at a time). **WARNING: `typewrite` cannot handle backslashes (`\`) — it will error out. For any text containing `\`, use the clipboard method below instead.**
- `pyautogui.write('text')` — alias for typewrite (same backslash limitation)
- `pyautogui.press('key')` — press a single key (enter, tab, escape, backspace, delete, space, etc.)
- `pyautogui.hotkey({hotkey_examples})` — press key combination{hotkey_note}
- `pyautogui.keyDown('key')` — hold a key down
- `pyautogui.keyUp('key')` — release a key

### Reliable Text Input (for special characters, passwords, Unicode){type_text_section}
- `pyperclip.copy('text')` followed by `pyautogui.hotkey('{clipboard_paste_key}', 'v')` — clipboard paste. {clipboard_caveat}

### Timing
- `time.sleep(seconds)` — wait for the specified duration
{bash_section}
## Output Format

For each step, you MUST respond with:

1. **Reflection**: A brief analysis of what you see on screen and what you plan to do next.
2. **Action**: A fenced Python code block with the PyAutoGUI commands to execute.

Example response format:
```
I can see the text editor is open with an empty document. I need to type "Hello World" into the editor. I'll click on the text area first to make sure it's focused, then type the text.

```python
pyautogui.click(640, 400)
time.sleep(0.3)
{example_type_text}
```
```

### Important Guidelines

- Always reflect on what you see BEFORE taking action
- Use precise coordinates based on the screenshot — examine button positions carefully
- After clicking a menu or button, wait briefly (`time.sleep(0.5)`) for the UI to update
- If an action doesn't produce the expected result, try a different approach rather than repeating the same action
{typewrite_guidance}
- Multiple actions can be in a single code block (they execute sequentially)
- Do NOT use `pyautogui.locateOnScreen()` or image-based location — use coordinates from the screenshot{xdotool_tip}

## Special Commands

Instead of (or in addition to) a code block, you can emit these special commands on a line by themselves:

- **DONE** — The task is complete. Emit this when you have finished the objective.
- **FAIL** — The task is infeasible or cannot be completed. Emit this if you determine the task cannot be done.
- **WAIT** — You need more time to observe. Emit this to pause and get a fresh observation without taking any action.{bug_command_line}

## Observation

After each action, you will receive:
- A screenshot of the current desktop state
- An accessibility tree showing UI elements with their names, roles, and positions (when available)

Use BOTH the screenshot and accessibility tree to understand the current state. The accessibility tree is especially useful for:
- Finding the exact names of buttons and menu items
- Determining which element has focus
- Reading text content that might be hard to see in the screenshot
{qa_section}"#,
        platform_desc = pt.platform_desc,
        display_desc = pt.display_desc,
        clipboard_paste_key = pt.clipboard_paste_key,
        type_text_import = pt.type_text_import,
        hotkey_examples = pt.hotkey_examples,
        hotkey_note = pt.hotkey_note,
        xdotool_tip = pt.xdotool_tip,
        type_text_section = pt.type_text_section,
        clipboard_caveat = pt.clipboard_caveat,
        example_type_text = pt.example_type_text,
        typewrite_guidance = pt.typewrite_guidance,
        display_width = display_width,
        display_height = display_height,
        max_x = max_x,
        max_y = max_y,
        bash_section = bash_section,
        bug_command_line = bug_command_line,
        qa_section = qa_section,
    )
}

/// Platform-specific text fragments for the system prompt.
struct PlatformText {
    platform_desc: &'static str,
    display_desc: &'static str,
    clipboard_paste_key: &'static str,
    hotkey_examples: &'static str,
    hotkey_note: &'static str,
    xdotool_tip: &'static str,
    type_text_import: &'static str,
    type_text_section: &'static str,
    clipboard_caveat: &'static str,
    example_type_text: &'static str,
    typewrite_guidance: &'static str,
}

impl PlatformText {
    fn for_platform(platform: Platform) -> Self {
        match platform {
            Platform::Linux => Self {
                platform_desc: "a Linux desktop environment",
                display_desc: "The display is a virtual X11 framebuffer (Xvfb) running XFCE desktop",
                clipboard_paste_key: "ctrl",
                hotkey_examples: "'ctrl', 'a'",
                hotkey_note: " (ctrl+a, alt+f4, ctrl+shift+s, etc.)",
                xdotool_tip: "\n- When using xdotool, prefer `windowfocus` over `windowactivate` if the target container has no window manager (`windowactivate` requires `_NET_ACTIVE_WINDOW` support and will silently fail without a WM).",
                type_text_import: "\n- `type_text(text, delay_ms=12)` — reliable text input via xdotool (handles special characters, Unicode, passwords)",
                type_text_section: "\n- `type_text('text')` — types text character-by-character using xdotool. Handles the full UTF-8 range including special characters (`@`, `(`, `)`, `\\`, `#`, `!`, etc.). **This is the most reliable way to type text containing special characters.** Works in all input fields including Electron app password fields. Optional `delay_ms` parameter (default 12) controls inter-keystroke delay.",
                clipboard_caveat: "Alternative for bulk text, but may not work in all input fields (e.g., some Electron password fields block paste).",
                example_type_text: "type_text('Hello World')",
                typewrite_guidance: "- `pyautogui.typewrite()` is only appropriate for simple ASCII text without special characters; prefer `type_text()` when in doubt\n- Use `type_text('text')` for passwords, non-ASCII text, or any text containing special characters (`@`, `\\`, `(`, `)`, `#`, `!`, etc.). Example: `type_text('P@ssw0rd!#1')`\n- Use `pyperclip.copy()` + `pyautogui.hotkey('ctrl', 'v')` as a fallback for bulk text if `type_text()` is too slow for very long strings",
            },
            Platform::Macos => Self {
                platform_desc: "a macOS desktop environment",
                display_desc: "The display is a macOS desktop",
                clipboard_paste_key: "command",
                hotkey_examples: "'command', 'a'",
                hotkey_note: " (command+a, command+q, command+shift+s, etc.). **On macOS, use `command` instead of `ctrl` for most shortcuts.**",
                xdotool_tip: "",
                type_text_import: "",
                type_text_section: "",
                clipboard_caveat: "Supports Unicode, backslashes, and all special characters. **Always use this method when the text contains special characters.**",
                example_type_text: "pyperclip.copy('Hello World')\npyautogui.hotkey('command', 'v')",
                typewrite_guidance: "- `pyautogui.typewrite()` is only appropriate for simple backslash-free ASCII text; prefer the clipboard method when in doubt\n- Use `pyperclip.copy()` + `pyautogui.hotkey('command', 'v')` for non-ASCII text, long strings, or any text containing backslashes (`\\`). Example for typing a password with a backslash: `pyperclip.copy('my\\\\pass'); pyautogui.hotkey('command', 'v')`",
            },
        }
    }
}

/// Build the bash debugging tool section of the system prompt.
fn build_bash_section(enabled: bool) -> &'static str {
    if enabled {
        r#"

## Bash Debugging Tool

You also have access to a bash shell for **debugging purposes only**. Use this when something is going wrong and you need to investigate — for example, to check if a process is running, inspect file contents, examine logs, check environment variables, or diagnose why the GUI is not behaving as expected.

**IMPORTANT: Your primary interface is PyAutoGUI. Always prefer PyAutoGUI for interacting with the desktop. Only use bash when you need to debug or investigate an issue.**

To run a bash command, use a fenced bash code block:

```bash
# Example: check if the application process is running
ps aux | grep myapp

# Example: inspect a log file
cat /tmp/app.log

# Example: check file system state
ls -la /home/tester/Documents/
```

The command runs inside the container as the `tester` user. You will receive the stdout/stderr output of the command. After running a bash command, you will still receive a fresh screenshot and accessibility tree observation.

### When to use bash vs PyAutoGUI
- **PyAutoGUI** (primary): clicking, typing, keyboard shortcuts, all GUI interaction
- **Bash** (debugging only): checking process state, reading files/logs, inspecting environment, verifying file changes, diagnosing issues when the GUI is unresponsive or behaving unexpectedly

Do NOT use bash to launch GUI applications or perform actions that should be done through the GUI. The bash tool is strictly for observation and debugging."#
    } else {
        ""
    }
}

/// Build the QA bug reporting section of the system prompt.
fn build_qa_section(qa: bool, platform: Platform) -> String {
    if !qa {
        return String::new();
    }

    let qa_diagnostic_commands = match platform {
        Platform::Linux => {
            r#"- `cat /tmp/app.log` — check application log for errors, stack traces, warnings
- `ps aux | grep <app_name>` — verify process state (crashed? zombie? high CPU/memory?)
- `dmesg | tail -20` — check for kernel-level issues (segfaults, OOM kills)
- `ls -la <relevant_paths>` — verify file state (missing files, wrong permissions, corrupted output)
- `cat /proc/$(pgrep <app_name>)/status` — check process memory and resource usage
- `journalctl --no-pager -n 50 2>/dev/null || true` — check system logs for D-Bus errors, GTK warnings
- `xdotool getactivewindow getwindowname 2>/dev/null || true` — verify current window state"#
        }
        Platform::Macos => {
            r#"- `cat /tmp/app.log` — check application log for errors, stack traces, warnings
- `ps aux | grep <app_name>` — verify process state (crashed? zombie? high CPU/memory?)
- `ls -la <relevant_paths>` — verify file state (missing files, wrong permissions, corrupted output)
- `log show --predicate 'process == "<app_name>"' --last 5m 2>/dev/null | tail -50` — check system logs
- `osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'` — verify current frontmost app"#
        }
    };

    format!(
        r##"

## QA Bug Reporting Mode (ACTIVE)

You are also acting as a QA tester. While completing your task, watch for **application bugs** — unexpected behavior, UI glitches, crashes, incorrect data, broken workflows, missing features, or accessibility issues in the application under test.

**IMPORTANT: Only report bugs in the application itself.** Do NOT report:
- PyAutoGUI execution errors (these are your tooling, not app bugs)
- Screenshot or accessibility tree capture issues
- Network or Docker infrastructure problems
- Issues caused by your own incorrect coordinates or actions

### Diagnosing Bugs

Before reporting a bug, use bash commands to gather diagnostic evidence. You have bash access — use it! Run these as appropriate:

{qa_diagnostic_commands}

Gather this evidence **before** emitting the BUG command so your report includes concrete data, not just visual observations.

### Reporting a Bug

When you find an app bug, emit the `BUG` command on its own line, followed by a detailed description:

```
BUG
<One-line summary of the bug>
<Detailed description: what you observed, what you expected, relevant log output or evidence>
<Steps that led to this state>
```

After reporting a bug, **continue your task normally**. You can include code blocks in the same response as a BUG report. The bug will be logged and you should proceed with your objective.

You may report multiple bugs throughout the test run. Each will receive a unique ID."##,
        qa_diagnostic_commands = qa_diagnostic_commands,
    )
}

/// Check if an error message indicates a context length exceeded error.
///
/// Different LLM providers may use different error formats:
/// - OpenAI: "context_length_exceeded" or "maximum context length"
/// - Anthropic: "prompt is too long" or "maximum number of tokens"
pub fn is_context_length_error(error_msg: &str) -> bool {
    let lower = error_msg.to_lowercase();
    lower.contains("context_length_exceeded")
        || lower.contains("maximum context length")
        || lower.contains("prompt is too long")
        || lower.contains("maximum number of tokens")
        || lower.contains("context window")
        || lower.contains("token limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_screenshot_observation() -> (Observation, tempfile::NamedTempFile) {
        // Each test gets its own temp file to avoid parallel-test races
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), b"fake-png").unwrap();
        let obs = Observation {
            screenshot_path: Some(f.path().to_path_buf()),
            a11y_tree_text: None,
        };
        (obs, f)
    }

    fn make_full_observation() -> (Observation, tempfile::NamedTempFile) {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), b"fake-png").unwrap();
        let obs = Observation {
            screenshot_path: Some(f.path().to_path_buf()),
            a11y_tree_text: Some("button\tOK\t\tGtkButton".into()),
        };
        (obs, f)
    }

    fn make_a11y_only_observation() -> Observation {
        Observation {
            screenshot_path: None,
            a11y_tree_text: Some("panel\troot\t\tGtkWindow".into()),
        }
    }

    fn make_empty_observation() -> Observation {
        Observation {
            screenshot_path: None,
            a11y_tree_text: None,
        }
    }

    // --- build_system_prompt tests ---

    #[test]
    fn test_system_prompt_contains_display_dimensions() {
        let prompt = build_system_prompt(1920, 1080, false, false, Platform::Linux);
        assert!(prompt.contains("1920x1080"));
        assert!(prompt.contains("1919"));
        assert!(prompt.contains("1079"));
    }

    #[test]
    fn test_system_prompt_contains_action_space() {
        let prompt = build_system_prompt(1280, 800, false, false, Platform::Linux);
        assert!(prompt.contains("pyautogui.click"));
        assert!(prompt.contains("pyautogui.typewrite"));
        assert!(prompt.contains("pyautogui.hotkey"));
        assert!(prompt.contains("pyautogui.scroll"));
        assert!(prompt.contains("pyperclip.copy"));
    }

    #[test]
    fn test_system_prompt_warns_about_windowactivate() {
        let prompt = build_system_prompt(1280, 800, false, false, Platform::Linux);
        assert!(prompt.contains("windowfocus"));
        assert!(prompt.contains("windowactivate"));
        assert!(prompt.contains("no window manager"));
    }

    #[test]
    fn test_system_prompt_platform_macos() {
        let prompt = build_system_prompt(1920, 1080, false, false, Platform::Macos);
        assert!(prompt.contains("macOS desktop environment"));
        assert!(prompt.contains("'command', 'v'"));
        assert!(prompt.contains("'command', 'a'"));
        assert!(!prompt.contains("Xvfb"));
        assert!(!prompt.contains("xdotool"));
        assert!(!prompt.contains("type_text"));
        assert!(!prompt.contains("'ctrl', 'v'"));
        assert!(!prompt.contains("'ctrl', 'a'"));
    }

    #[test]
    fn test_system_prompt_contains_special_commands() {
        let prompt = build_system_prompt(1280, 800, false, false, Platform::Linux);
        assert!(prompt.contains("DONE"));
        assert!(prompt.contains("FAIL"));
        assert!(prompt.contains("WAIT"));
    }

    #[test]
    fn test_system_prompt_contains_output_format() {
        let prompt = build_system_prompt(1280, 800, false, false, Platform::Linux);
        assert!(prompt.contains("Reflection"));
        assert!(prompt.contains("Action"));
        assert!(prompt.contains("```python"));
    }

    #[test]
    fn test_system_prompt_contains_coordinate_system() {
        let prompt = build_system_prompt(1280, 800, false, false, Platform::Linux);
        assert!(prompt.contains("(0, 0)"));
        assert!(prompt.contains("top-left"));
    }

    // --- ContextManager tests ---

    #[test]
    fn test_context_manager_new() {
        let ctx = ContextManager::new(
            1920,
            1080,
            "Click the button",
            DEFAULT_MAX_TRAJECTORY_LENGTH,
            false,
            false,
            Platform::Linux,
        );
        assert_eq!(ctx.trajectory_len(), 0);
        assert!(ctx.system_prompt.contains("1920x1080"));
        assert_eq!(ctx.instruction, "Click the button");
        assert_eq!(ctx.max_trajectory_length, DEFAULT_MAX_TRAJECTORY_LENGTH);
    }

    #[test]
    fn test_build_messages_no_trajectory() {
        let ctx = ContextManager::new(
            1920,
            1080,
            "Click the button",
            3,
            false,
            false,
            Platform::Linux,
        );
        let (obs, _f) = make_screenshot_observation();
        let messages = ctx.build_messages(&obs);

        // Should have: system + instruction + current observation
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert!(
            messages[1]
                .content
                .as_ref()
                .unwrap()
                .as_str()
                .unwrap()
                .contains("Click the button")
        );
        assert_eq!(messages[2].role, "user"); // observation
    }

    #[test]
    fn test_build_messages_with_trajectory() {
        let mut ctx = ContextManager::new(
            1920,
            1080,
            "Click the button",
            3,
            false,
            false,
            Platform::Linux,
        );
        let (obs1, _f1) = make_screenshot_observation();
        ctx.push_turn(TrajectoryTurn {
            observation: obs1,
            response_text: "I see a button. I'll click it.".into(),
            error_feedback: None,
            bash_output: None,
        });

        let (obs, _f2) = make_full_observation();
        let messages = ctx.build_messages(&obs);

        // system + instruction + (prev obs + prev response) + current obs
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user"); // instruction
        assert_eq!(messages[2].role, "user"); // prev observation
        assert_eq!(messages[3].role, "assistant"); // prev response
        assert_eq!(messages[4].role, "user"); // current observation
    }

    #[test]
    fn test_build_messages_with_error_feedback() {
        let mut ctx = ContextManager::new(
            1920,
            1080,
            "Click the button",
            3,
            false,
            false,
            Platform::Linux,
        );
        let (obs1, _f1) = make_screenshot_observation();
        ctx.push_turn(TrajectoryTurn {
            observation: obs1,
            response_text: "I'll click at (100, 200)".into(),
            error_feedback: Some("NameError: name 'foo' is not defined".into()),
            bash_output: None,
        });

        let (obs, _f2) = make_screenshot_observation();
        let messages = ctx.build_messages(&obs);

        // system + instruction + (prev obs + prev response + error feedback) + current obs
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[4].role, "user"); // error feedback
        let feedback = messages[4].content.as_ref().unwrap().as_str().unwrap();
        assert!(feedback.contains("NameError"));
    }

    #[test]
    fn test_sliding_window_truncation() {
        let mut ctx = ContextManager::new(
            1920,
            1080,
            "Click the button",
            2,
            false,
            false,
            Platform::Linux,
        );

        // Push 4 turns — keep temp files alive for the sliding window
        let mut _files = Vec::new();
        for i in 0..4 {
            let (obs, f) = make_screenshot_observation();
            _files.push(f);
            ctx.push_turn(TrajectoryTurn {
                observation: obs,
                response_text: format!("Turn {i}"),
                error_feedback: None,
                bash_output: None,
            });
        }

        assert_eq!(ctx.trajectory_len(), 4);

        let (obs, _f) = make_screenshot_observation();
        let messages = ctx.build_messages(&obs);

        // system + instruction + 2 turns * (obs + response) + current obs = 2 + 4 + 1 = 7
        assert_eq!(messages.len(), 7);

        // Verify only the last 2 turns are included
        // messages[3] = "Turn 2" assistant response, messages[5] = "Turn 3" assistant response
        let turn2_text = messages[3].content.as_ref().unwrap().as_str().unwrap();
        assert_eq!(turn2_text, "Turn 2");

        let turn3_text = messages[5].content.as_ref().unwrap().as_str().unwrap();
        assert_eq!(turn3_text, "Turn 3");
    }

    #[test]
    fn test_sliding_window_exact_fit() {
        let mut ctx = ContextManager::new(1920, 1080, "test", 3, false, false, Platform::Linux);

        // Push exactly 3 turns
        let mut _files = Vec::new();
        for i in 0..3 {
            let (obs, f) = make_screenshot_observation();
            _files.push(f);
            ctx.push_turn(TrajectoryTurn {
                observation: obs,
                response_text: format!("Turn {i}"),
                error_feedback: None,
                bash_output: None,
            });
        }

        let (obs, _f) = make_screenshot_observation();
        let messages = ctx.build_messages(&obs);

        // system + instruction + 3 turns * (obs + response) + current obs = 2 + 6 + 1 = 9
        assert_eq!(messages.len(), 9);
    }

    #[test]
    fn test_fallback_messages_no_trajectory() {
        let mut ctx = ContextManager::new(
            1920,
            1080,
            "Click the button",
            3,
            false,
            false,
            Platform::Linux,
        );
        let (obs1, _f1) = make_screenshot_observation();
        ctx.push_turn(TrajectoryTurn {
            observation: obs1,
            response_text: "Turn 0".into(),
            error_feedback: None,
            bash_output: None,
        });

        let (obs, _f2) = make_full_observation();
        let messages = ctx.build_fallback_messages(&obs);

        // system + instruction (with context drop note) + current observation
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        let instruction_text = messages[1].content.as_ref().unwrap().as_str().unwrap();
        assert!(instruction_text.contains("context length limits"));
        assert!(instruction_text.contains("Click the button"));
    }

    #[test]
    fn test_clear_trajectory() {
        let mut ctx = ContextManager::new(1920, 1080, "test", 3, false, false, Platform::Linux);
        let (obs1, _f1) = make_screenshot_observation();
        ctx.push_turn(TrajectoryTurn {
            observation: obs1,
            response_text: "Turn 0".into(),
            error_feedback: None,
            bash_output: None,
        });
        assert_eq!(ctx.trajectory_len(), 1);

        ctx.clear_trajectory();
        assert_eq!(ctx.trajectory_len(), 0);
    }

    // --- observation_to_message tests ---

    #[test]
    fn test_observation_to_message_screenshot_only() {
        let (obs, _f) = make_screenshot_observation();
        let msgs = observation_to_message(&obs);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        // Should be an image_url content array
        let content = msgs[0].content.as_ref().unwrap();
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "image_url");
    }

    #[test]
    fn test_observation_to_message_full() {
        let (obs, _f) = make_full_observation();
        let msgs = observation_to_message(&obs);
        assert_eq!(msgs.len(), 1);
        // Should be a combined message with image + text
        let content = msgs[0].content.as_ref().unwrap();
        let arr = content.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "image_url");
        assert_eq!(arr[1]["type"], "text");
        let text = arr[1]["text"].as_str().unwrap();
        assert!(text.contains("accessibility tree"));
        assert!(text.contains("GtkButton"));
    }

    #[test]
    fn test_observation_to_message_a11y_only() {
        let obs = make_a11y_only_observation();
        let msgs = observation_to_message(&obs);
        assert_eq!(msgs.len(), 1);
        let content = msgs[0].content.as_ref().unwrap().as_str().unwrap();
        assert!(content.contains("accessibility tree"));
        assert!(content.contains("GtkWindow"));
    }

    #[test]
    fn test_observation_to_message_empty() {
        let obs = make_empty_observation();
        let msgs = observation_to_message(&obs);
        assert_eq!(msgs.len(), 1);
        let content = msgs[0].content.as_ref().unwrap().as_str().unwrap();
        assert!(content.contains("No observation available"));
    }

    // --- is_context_length_error tests ---

    #[test]
    fn test_context_length_error_openai() {
        assert!(is_context_length_error("context_length_exceeded"));
        assert!(is_context_length_error(
            "This model's maximum context length is 128000 tokens"
        ));
    }

    #[test]
    fn test_context_length_error_anthropic() {
        assert!(is_context_length_error("prompt is too long: 200000 tokens"));
        assert!(is_context_length_error("maximum number of tokens exceeded"));
    }

    #[test]
    fn test_context_length_error_generic() {
        assert!(is_context_length_error("exceeded the token limit"));
        assert!(is_context_length_error("context window exceeded"));
    }

    #[test]
    fn test_not_context_length_error() {
        assert!(!is_context_length_error("rate limit exceeded"));
        assert!(!is_context_length_error("authentication failed"));
        assert!(!is_context_length_error("internal server error"));
        assert!(!is_context_length_error(
            "max_tokens must be less than 128000"
        ));
    }

    // --- Multiple trajectory with mixed observation types ---

    #[test]
    fn test_trajectory_with_mixed_observations() {
        let mut ctx = ContextManager::new(1920, 1080, "test", 3, false, false, Platform::Linux);

        // Turn 1: screenshot only
        let (obs1, _f1) = make_screenshot_observation();
        ctx.push_turn(TrajectoryTurn {
            observation: obs1,
            response_text: "Clicked button".into(),
            error_feedback: None,
            bash_output: None,
        });

        // Turn 2: full observation
        let (obs2, _f2) = make_full_observation();
        ctx.push_turn(TrajectoryTurn {
            observation: obs2,
            response_text: "Typed text".into(),
            error_feedback: None,
            bash_output: None,
        });

        // Turn 3: a11y only
        ctx.push_turn(TrajectoryTurn {
            observation: make_a11y_only_observation(),
            response_text: "Checked state".into(),
            error_feedback: Some("timeout".into()),
            bash_output: None,
        });

        let (obs, _f3) = make_full_observation();
        let messages = ctx.build_messages(&obs);

        // system + instruction + 3 turns * (obs + response) + error_feedback + current obs
        // = 2 + 6 + 1 + 1 = 10
        assert_eq!(messages.len(), 10);
    }

    #[test]
    fn test_zero_trajectory_length() {
        let mut ctx = ContextManager::new(1920, 1080, "test", 0, false, false, Platform::Linux);
        let (obs1, _f1) = make_screenshot_observation();
        ctx.push_turn(TrajectoryTurn {
            observation: obs1,
            response_text: "Turn 0".into(),
            error_feedback: None,
            bash_output: None,
        });

        let (obs, _f2) = make_screenshot_observation();
        let messages = ctx.build_messages(&obs);

        // system + instruction + current obs only (no trajectory)
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_system_prompt_contains_qa_section_when_enabled() {
        let prompt = build_system_prompt(1920, 1080, false, true, Platform::Linux);
        assert!(prompt.contains("QA Bug Reporting Mode"));
        assert!(prompt.contains("BUG"));
        assert!(prompt.contains("cat /tmp/app.log"));
        assert!(prompt.contains("continue your task normally"));
        // BUG should appear in the Special Commands section alongside DONE/FAIL/WAIT
        let special_cmds_idx = prompt.find("## Special Commands").unwrap();
        let observation_idx = prompt.find("## Observation").unwrap();
        let special_section = &prompt[special_cmds_idx..observation_idx];
        assert!(
            special_section.contains("**BUG**"),
            "BUG should be listed in Special Commands when QA is enabled"
        );
    }

    #[test]
    fn test_system_prompt_no_qa_section_when_disabled() {
        let prompt = build_system_prompt(1920, 1080, false, false, Platform::Linux);
        assert!(!prompt.contains("QA Bug Reporting Mode"));
        // BUG should NOT appear in Special Commands when QA is disabled
        let special_cmds_idx = prompt.find("## Special Commands").unwrap();
        let observation_idx = prompt.find("## Observation").unwrap();
        let special_section = &prompt[special_cmds_idx..observation_idx];
        assert!(!special_section.contains("**BUG**"));
    }
}
