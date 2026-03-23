/// Redacts secret values from text to prevent credential leakage in logs and artifacts.
///
/// Secret values shorter than `MIN_SECRET_LENGTH` are skipped to avoid
/// over-redacting common substrings like single characters or empty strings.

const MIN_SECRET_LENGTH: usize = 3;
const REDACTED: &str = "[REDACTED]";

/// Holds secret values and replaces them with `[REDACTED]` in any text.
#[derive(Debug, Clone)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    /// Create a new redactor from an iterator of secret values.
    ///
    /// Values shorter than 3 characters are silently dropped.
    pub fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        let mut secrets: Vec<String> = secrets
            .into_iter()
            .filter(|s| s.len() >= MIN_SECRET_LENGTH)
            .collect();
        // Sort longest-first so overlapping values are replaced greedily.
        secrets.sort_by(|a, b| b.len().cmp(&a.len()));
        Self { secrets }
    }

    /// Replace all known secret values in `text` with `[REDACTED]`.
    pub fn redact(&self, text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut idx = 0;

        while idx < text.len() {
            let remainder = &text[idx..];
            if let Some(secret) = self
                .secrets
                .iter()
                .find(|secret| remainder.starts_with(secret.as_str()))
            {
                result.push_str(REDACTED);
                idx += secret.len();
                continue;
            }

            let ch = remainder
                .chars()
                .next()
                .expect("idx always points to a valid character boundary");
            result.push(ch);
            idx += ch.len_utf8();
        }
        result
    }

    /// Returns true when there are no secrets to redact.
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }
}

/// Recursively redact string values inside a JSON value.
pub fn redact_json_value(value: &mut serde_json::Value, redactor: &Redactor) {
    match value {
        serde_json::Value::String(text) => {
            *text = redactor.redact(text);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_json_value(item, redactor);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                redact_json_value(value, redactor);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_single_value() {
        let r = Redactor::new(vec!["hunter2".to_string()]);
        assert_eq!(
            r.redact("my password is hunter2"),
            "my password is [REDACTED]"
        );
    }

    #[test]
    fn test_redact_multiple_values() {
        let r = Redactor::new(vec!["alice".to_string(), "s3cret".to_string()]);
        let input = "user=alice pass=s3cret";
        let output = r.redact(input);
        assert_eq!(output, "user=[REDACTED] pass=[REDACTED]");
    }

    #[test]
    fn test_redact_overlapping_values() {
        // "supersecret" contains "secret" — the longer match should win.
        let r = Redactor::new(vec!["secret".to_string(), "supersecret".to_string()]);
        assert_eq!(r.redact("val=supersecret"), "val=[REDACTED]");
    }

    #[test]
    fn test_skip_short_values() {
        let r = Redactor::new(vec!["ab".to_string(), "x".to_string(), "".to_string()]);
        assert!(r.is_empty());
        assert_eq!(r.redact("ab x test"), "ab x test");
    }

    #[test]
    fn test_empty_redactor_is_noop() {
        let r = Redactor::new(Vec::<String>::new());
        assert!(r.is_empty());
        assert_eq!(r.redact("nothing to redact"), "nothing to redact");
    }

    #[test]
    fn test_redact_multiple_occurrences() {
        let r = Redactor::new(vec!["token123".to_string()]);
        assert_eq!(
            r.redact("first token123 then token123 again"),
            "first [REDACTED] then [REDACTED] again"
        );
    }

    #[test]
    fn test_redacted_marker_is_not_reprocessed() {
        let r = Redactor::new(vec!["supersecret".to_string(), "ACTED".to_string()]);
        assert_eq!(r.redact("value=supersecret"), "value=[REDACTED]");
    }

    #[test]
    fn test_redact_json_value_redacts_escaped_strings() {
        let redactor = Redactor::new(vec!["p\"ass\\word".to_string()]);
        let mut value = serde_json::json!({
            "instruction": "secret=p\"ass\\word",
            "nested": ["keep", "p\"ass\\word"]
        });

        redact_json_value(&mut value, &redactor);

        assert_eq!(value["instruction"], "secret=[REDACTED]");
        assert_eq!(value["nested"][1], "[REDACTED]");
    }
}
