pub mod context;
pub(super) mod llm_retry;
pub mod loop_v2;
pub mod pyautogui;

/// Default per-step execution timeout in seconds.
/// Used by both the agent loop (wall-clock per step) and PyAutoGUI (per code block).
pub const DEFAULT_STEP_TIMEOUT_SECS: u64 = 60;
