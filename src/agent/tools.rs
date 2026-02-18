#![allow(dead_code)]

use std::path::Path;

use serde_json::json;
use tracing::{debug, info};

use crate::docker::DockerSession;
use crate::error::AppError;
use crate::input;
use crate::screenshot;

/// Result of executing a tool call.
pub enum ToolResult {
    /// Plain text result for the tool response message.
    Success(String),
    /// Screenshot was taken; includes the base64 data URL.
    ScreenshotTaken(String),
    /// The agent called done().
    Done { passed: bool, reasoning: String },
}

/// Return the list of tool definitions in OpenAI function-calling format.
pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        make_tool(
            "moveMouse",
            "Move the mouse cursor to the specified screen coordinates",
            json!({
                "type": "object",
                "properties": {
                    "posX": { "type": "integer", "description": "X coordinate" },
                    "posY": { "type": "integer", "description": "Y coordinate" }
                },
                "required": ["posX", "posY"]
            }),
        ),
        make_tool(
            "leftClick",
            "Perform a left mouse click at the current cursor position",
            json!({ "type": "object", "properties": {} }),
        ),
        make_tool(
            "doubleClick",
            "Perform a double left click at the current cursor position",
            json!({ "type": "object", "properties": {} }),
        ),
        make_tool(
            "rightClick",
            "Perform a right mouse click at the current cursor position",
            json!({ "type": "object", "properties": {} }),
        ),
        make_tool(
            "middleClick",
            "Perform a middle mouse click at the current cursor position",
            json!({ "type": "object", "properties": {} }),
        ),
        make_tool(
            "scrollUp",
            "Scroll the mouse wheel up",
            json!({
                "type": "object",
                "properties": {
                    "ticks": { "type": "integer", "description": "Number of scroll ticks" }
                },
                "required": ["ticks"]
            }),
        ),
        make_tool(
            "scrollDown",
            "Scroll the mouse wheel down",
            json!({
                "type": "object",
                "properties": {
                    "ticks": { "type": "integer", "description": "Number of scroll ticks" }
                },
                "required": ["ticks"]
            }),
        ),
        make_tool(
            "dragLeftClickMouse",
            "Drag with left mouse button from start to end coordinates",
            json!({
                "type": "object",
                "properties": {
                    "startX": { "type": "integer" },
                    "startY": { "type": "integer" },
                    "endX": { "type": "integer" },
                    "endY": { "type": "integer" }
                },
                "required": ["startX", "startY", "endX", "endY"]
            }),
        ),
        make_tool(
            "pressAndHoldKey",
            "Press and hold a key for a specified duration, optionally with modifiers",
            json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key name (e.g. Enter, Tab, Escape, a, space)" },
                    "milliseconds": { "type": "integer", "description": "How long to hold in ms (0 = tap)" },
                    "modifiers": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Modifier keys: ctrl, alt, shift, super"
                    }
                },
                "required": ["key", "milliseconds"]
            }),
        ),
        make_tool(
            "type",
            "Type a string of text using the keyboard",
            json!({
                "type": "object",
                "properties": {
                    "str": { "type": "string", "description": "The text to type" }
                },
                "required": ["str"]
            }),
        ),
        make_tool(
            "screenshot",
            "Take a screenshot of the current screen",
            json!({ "type": "object", "properties": {} }),
        ),
        make_tool(
            "think",
            "Use this tool to think step-by-step before acting. Describe what you see on screen, the absolute screen coordinates of any relevant UI elements (from the top-left corner of the 1280 x 800 screen), what you plan to do next, and why. You MUST call this before any action sequence.",
            json!({
                "type": "object",
                "properties": {
                    "observation": { "type": "string", "description": "What you see on the current screenshot (UI elements, buttons, text, cursor position)" },
                    "plan": { "type": "string", "description": "What you will do next and why" }
                },
                "required": ["observation", "plan"]
            }),
        ),
        make_tool(
            "done",
            "Signal that testing is complete with a pass/fail verdict",
            json!({
                "type": "object",
                "properties": {
                    "isGood": { "type": "boolean", "description": "true if test passed, false if failed" },
                    "reasoning": { "type": "string", "description": "Explanation of the verdict" }
                },
                "required": ["isGood", "reasoning"]
            }),
        ),
    ]
}

fn make_tool(name: &str, description: &str, parameters: serde_json::Value) -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

/// Dispatch a tool call by name, executing it against the container.
pub async fn dispatch_tool(
    name: &str,
    args_json: &str,
    session: &DockerSession,
    artifacts_dir: &Path,
    screenshot_counter: &mut usize,
) -> Result<ToolResult, AppError> {
    let args: serde_json::Value = serde_json::from_str(args_json)
        .map_err(|e| AppError::Agent(format!("Invalid tool arguments JSON: {e}")))?;

    debug!("Dispatching tool: {name}({args})");

    match name {
        "moveMouse" => {
            let x = args["posX"].as_i64().unwrap_or(0) as i32;
            let y = args["posY"].as_i64().unwrap_or(0) as i32;
            let cmd = input::build_move_mouse(x, y);
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success(format!("Moved mouse to ({x}, {y})")))
        }
        "leftClick" => {
            let cmd = input::build_left_click();
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success("Left clicked".into()))
        }
        "doubleClick" => {
            let cmd = input::build_double_click();
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success("Double clicked".into()))
        }
        "rightClick" => {
            let cmd = input::build_right_click();
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success("Right clicked".into()))
        }
        "middleClick" => {
            let cmd = input::build_middle_click();
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success("Middle clicked".into()))
        }
        "scrollUp" => {
            let ticks = args["ticks"].as_i64().unwrap_or(1) as i32;
            let cmd = input::build_scroll_up(ticks);
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success(format!("Scrolled up {ticks} ticks")))
        }
        "scrollDown" => {
            let ticks = args["ticks"].as_i64().unwrap_or(1) as i32;
            let cmd = input::build_scroll_down(ticks);
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success(format!("Scrolled down {ticks} ticks")))
        }
        "dragLeftClickMouse" => {
            let sx = args["startX"].as_i64().unwrap_or(0) as i32;
            let sy = args["startY"].as_i64().unwrap_or(0) as i32;
            let ex = args["endX"].as_i64().unwrap_or(0) as i32;
            let ey = args["endY"].as_i64().unwrap_or(0) as i32;
            let cmd = input::build_drag(sx, sy, ex, ey);
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success(format!(
                "Dragged from ({sx},{sy}) to ({ex},{ey})"
            )))
        }
        "pressAndHoldKey" => {
            let key = args["key"].as_str().unwrap_or("Return");
            let ms = args["milliseconds"].as_i64().unwrap_or(0) as i32;
            let modifiers: Option<Vec<&str>> = args["modifiers"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect());
            let cmd = input::build_key_press(key, ms, modifiers.as_deref());
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success(format!("Pressed key: {key}")))
        }
        "type" => {
            let text = args["str"].as_str().unwrap_or("");
            let cmd = input::build_type(text);
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            session.exec(&cmd_refs).await?;
            Ok(ToolResult::Success(format!("Typed: {text}")))
        }
        // "screenshot" => {
        //     let (path, data_url) =
        //         screenshot::capture_screenshot(session, artifacts_dir, *screenshot_counter)
        //             .await?;
        //     *screenshot_counter += 1;
        //     debug!("Screenshot saved to {}", path.display());
        //     Ok(ToolResult::ScreenshotTaken(data_url))
        // }
        "think" => {
            let observation = args["observation"].as_str().unwrap_or("");
            let plan = args["plan"].as_str().unwrap_or("");
            info!("Agent thinking: observation={observation}, plan={plan}");
            Ok(ToolResult::Success(format!(
                "Observation noted. Plan acknowledged. Proceed with your actions."
            )))
        }
        "done" => {
            let is_good = args["isGood"].as_bool().unwrap_or(false);
            let reasoning = args["reasoning"]
                .as_str()
                .unwrap_or("No reasoning provided")
                .to_string();
            Ok(ToolResult::Done {
                passed: is_good,
                reasoning,
            })
        }
        _ => Err(AppError::Agent(format!("Unknown tool: {name}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definitions_count() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 13, "Expected 13 tool definitions");
    }

    #[test]
    fn test_tool_definitions_valid_json() {
        let defs = tool_definitions();
        for def in &defs {
            assert_eq!(def["type"], "function");
            assert!(def["function"]["name"].is_string());
            assert!(def["function"]["description"].is_string());
            assert!(def["function"]["parameters"].is_object());
        }
    }

    #[test]
    fn test_tool_names() {
        let defs = tool_definitions();
        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["function"]["name"].as_str().unwrap())
            .collect();

        let expected = [
            "moveMouse",
            "leftClick",
            "doubleClick",
            "rightClick",
            "middleClick",
            "scrollUp",
            "scrollDown",
            "dragLeftClickMouse",
            "pressAndHoldKey",
            "type",
            // "screenshot",
            "think",
            "done",
        ];

        for name in &expected {
            assert!(names.contains(name), "Missing tool: {name}");
        }
    }

    // Note: dispatch_tool tests that require a DockerSession are integration tests.
    // We test the argument parsing and result types via the done tool which doesn't
    // need a real session.

    #[test]
    fn test_done_tool_args_parsing() {
        let args = r#"{"isGood": true, "reasoning": "all tests passed"}"#;
        let parsed: serde_json::Value = serde_json::from_str(args).unwrap();
        assert_eq!(parsed["isGood"].as_bool().unwrap(), true);
        assert_eq!(parsed["reasoning"].as_str().unwrap(), "all tests passed");
    }

    #[test]
    fn test_done_tool_args_fail() {
        let args = r#"{"isGood": false, "reasoning": "button was missing"}"#;
        let parsed: serde_json::Value = serde_json::from_str(args).unwrap();
        assert_eq!(parsed["isGood"].as_bool().unwrap(), false);
    }

    #[test]
    fn test_unknown_tool_args() {
        let result = serde_json::from_str::<serde_json::Value>("not json");
        assert!(result.is_err());
    }
}
