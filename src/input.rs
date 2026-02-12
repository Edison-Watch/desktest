/// Pure functions that build xdotool command arrays.
/// These are executed inside the container via `DockerSession::exec`.

pub fn build_move_mouse(x: i32, y: i32) -> Vec<String> {
    vec![
        "xdotool".into(),
        "mousemove".into(),
        x.to_string(),
        y.to_string(),
    ]
}

pub fn build_left_click() -> Vec<String> {
    vec!["xdotool".into(), "click".into(), "1".into()]
}

pub fn build_double_click() -> Vec<String> {
    vec![
        "xdotool".into(),
        "click".into(),
        "--repeat".into(),
        "2".into(),
        "1".into(),
    ]
}

pub fn build_right_click() -> Vec<String> {
    vec!["xdotool".into(), "click".into(), "3".into()]
}

pub fn build_middle_click() -> Vec<String> {
    vec!["xdotool".into(), "click".into(), "2".into()]
}

pub fn build_scroll_up(ticks: i32) -> Vec<String> {
    vec![
        "xdotool".into(),
        "click".into(),
        "--repeat".into(),
        ticks.to_string(),
        "4".into(),
    ]
}

pub fn build_scroll_down(ticks: i32) -> Vec<String> {
    vec![
        "xdotool".into(),
        "click".into(),
        "--repeat".into(),
        ticks.to_string(),
        "5".into(),
    ]
}

pub fn build_drag(start_x: i32, start_y: i32, end_x: i32, end_y: i32) -> Vec<String> {
    // xdotool mousemove X Y mousedown 1 mousemove X2 Y2 mouseup 1
    vec![
        "xdotool".into(),
        "mousemove".into(),
        start_x.to_string(),
        start_y.to_string(),
        "mousedown".into(),
        "1".into(),
        "mousemove".into(),
        end_x.to_string(),
        end_y.to_string(),
        "mouseup".into(),
        "1".into(),
    ]
}

pub fn build_type(text: &str) -> Vec<String> {
    vec![
        "xdotool".into(),
        "type".into(),
        "--clearmodifiers".into(),
        text.into(),
    ]
}

/// Build a key press command with optional modifiers and hold duration.
///
/// - `key`: key name (e.g. "Return", "Tab", "a", "space", etc.)
/// - `hold_ms`: how long to hold the key in milliseconds (0 = tap)
/// - `modifiers`: optional modifier keys like "ctrl", "alt", "shift", "super"
pub fn build_key_press(key: &str, hold_ms: i32, modifiers: Option<&[&str]>) -> Vec<String> {
    let key_combo = match modifiers {
        Some(mods) if !mods.is_empty() => {
            let mut combo = mods.join("+");
            combo.push('+');
            combo.push_str(key);
            combo
        }
        _ => key.into(),
    };

    if hold_ms <= 0 {
        // Simple key tap
        vec!["xdotool".into(), "key".into(), key_combo]
    } else {
        // Hold: keydown, sleep, keyup
        // xdotool doesn't have a native hold duration, so we chain commands via bash
        let sleep_secs = hold_ms as f64 / 1000.0;
        vec![
            "bash".into(),
            "-c".into(),
            format!(
                "xdotool keydown {combo} && sleep {sleep:.3} && xdotool keyup {combo}",
                combo = key_combo,
                sleep = sleep_secs
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_mouse() {
        assert_eq!(
            build_move_mouse(100, 200),
            vec!["xdotool", "mousemove", "100", "200"]
        );
    }

    #[test]
    fn test_left_click() {
        assert_eq!(build_left_click(), vec!["xdotool", "click", "1"]);
    }

    #[test]
    fn test_double_click() {
        assert_eq!(
            build_double_click(),
            vec!["xdotool", "click", "--repeat", "2", "1"]
        );
    }

    #[test]
    fn test_right_click() {
        assert_eq!(build_right_click(), vec!["xdotool", "click", "3"]);
    }

    #[test]
    fn test_middle_click() {
        assert_eq!(build_middle_click(), vec!["xdotool", "click", "2"]);
    }

    #[test]
    fn test_scroll_up() {
        assert_eq!(
            build_scroll_up(3),
            vec!["xdotool", "click", "--repeat", "3", "4"]
        );
    }

    #[test]
    fn test_scroll_down() {
        assert_eq!(
            build_scroll_down(5),
            vec!["xdotool", "click", "--repeat", "5", "5"]
        );
    }

    #[test]
    fn test_drag() {
        assert_eq!(
            build_drag(10, 20, 300, 400),
            vec![
                "xdotool",
                "mousemove",
                "10",
                "20",
                "mousedown",
                "1",
                "mousemove",
                "300",
                "400",
                "mouseup",
                "1"
            ]
        );
    }

    #[test]
    fn test_type_text() {
        assert_eq!(
            build_type("hello world"),
            vec!["xdotool", "type", "--clearmodifiers", "hello world"]
        );
    }

    #[test]
    fn test_key_press_simple() {
        assert_eq!(
            build_key_press("Return", 0, None),
            vec!["xdotool", "key", "Return"]
        );
    }

    #[test]
    fn test_key_press_with_modifier() {
        assert_eq!(
            build_key_press("c", 0, Some(&["ctrl"])),
            vec!["xdotool", "key", "ctrl+c"]
        );
    }

    #[test]
    fn test_key_press_with_multiple_modifiers() {
        let cmd = build_key_press("Delete", 0, Some(&["ctrl", "alt"]));
        assert_eq!(cmd, vec!["xdotool", "key", "ctrl+alt+Delete"]);
    }

    #[test]
    fn test_key_press_with_hold() {
        let cmd = build_key_press("Return", 100, None);
        assert_eq!(cmd[0], "bash");
        assert_eq!(cmd[1], "-c");
        assert!(cmd[2].contains("keydown Return"));
        assert!(cmd[2].contains("sleep 0.100"));
        assert!(cmd[2].contains("keyup Return"));
    }

    #[test]
    fn test_key_press_with_modifier_and_hold() {
        let cmd = build_key_press("a", 500, Some(&["ctrl"]));
        assert_eq!(cmd[0], "bash");
        assert!(cmd[2].contains("keydown ctrl+a"));
        assert!(cmd[2].contains("keyup ctrl+a"));
    }
}
