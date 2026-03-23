use std::path::Path;
use std::time::Duration;

use super::MetricResult;
use crate::docker::DockerSession;
use crate::error::AppError;

/// file_compare: Copy file from container, compare against expected file.
pub(super) async fn evaluate_file_compare(
    session: &DockerSession,
    actual_path: &str,
    expected_path: &str,
    compare_mode: &crate::task::CompareMode,
    artifacts_dir: &Path,
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    // Copy the actual file from the container to a temp location
    let temp_actual = artifacts_dir.join("eval_actual_file");
    tokio::time::timeout(eval_timeout, session.copy_from(actual_path, &temp_actual))
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "Evaluation copy_from timed out after {}s: {actual_path}",
                eval_timeout.as_secs()
            ))
        })?
        .map_err(|e| {
            AppError::Infra(format!(
                "Failed to copy file '{actual_path}' from container: {e}"
            ))
        })?;

    let actual_bytes = std::fs::read(&temp_actual)
        .map_err(|e| AppError::Infra(format!("Failed to read copied file: {e}")))?;
    let expected_bytes = std::fs::read(expected_path).map_err(|e| {
        AppError::Infra(format!(
            "Failed to read expected file '{expected_path}': {e}"
        ))
    })?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_actual);

    let (passed, detail) = match compare_mode {
        crate::task::CompareMode::Exact => {
            if actual_bytes == expected_bytes {
                (true, "Files match exactly".to_string())
            } else {
                let actual_len = actual_bytes.len();
                let expected_len = expected_bytes.len();
                (
                    false,
                    format!(
                        "Files differ (actual: {actual_len} bytes, expected: {expected_len} bytes)"
                    ),
                )
            }
        }
        crate::task::CompareMode::Normalized => {
            let actual_text = normalize_text(&String::from_utf8_lossy(&actual_bytes));
            let expected_text = normalize_text(&String::from_utf8_lossy(&expected_bytes));
            if actual_text == expected_text {
                (true, "Files match (normalized)".to_string())
            } else {
                let diff = first_diff_line(&actual_text, &expected_text);
                (false, format!("Files differ (normalized). {diff}"))
            }
        }
    };

    Ok(MetricResult {
        passed,
        metric: "file_compare".to_string(),
        expected: format!("file: {expected_path}"),
        actual: format!("container: {actual_path}"),
        detail,
    })
}

/// file_compare_semantic: Parse structured files and compare data structures.
pub(super) async fn evaluate_file_compare_semantic(
    session: &DockerSession,
    actual_path: &str,
    expected_path: &str,
    format: &crate::task::SemanticFormat,
    artifacts_dir: &Path,
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    // Copy the actual file from the container
    let temp_actual = artifacts_dir.join("eval_semantic_actual");
    tokio::time::timeout(eval_timeout, session.copy_from(actual_path, &temp_actual))
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "Evaluation copy_from timed out after {}s: {actual_path}",
                eval_timeout.as_secs()
            ))
        })?
        .map_err(|e| {
            AppError::Infra(format!(
                "Failed to copy file '{actual_path}' from container: {e}"
            ))
        })?;

    let actual_str = std::fs::read_to_string(&temp_actual)
        .map_err(|e| AppError::Infra(format!("Failed to read copied file: {e}")))?;
    let expected_str = std::fs::read_to_string(expected_path).map_err(|e| {
        AppError::Infra(format!(
            "Failed to read expected file '{expected_path}': {e}"
        ))
    })?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_actual);

    let (passed, detail) = match format {
        crate::task::SemanticFormat::Json => compare_json(&actual_str, &expected_str),
        crate::task::SemanticFormat::Yaml => compare_yaml(&actual_str, &expected_str),
        crate::task::SemanticFormat::Xml => compare_xml(&actual_str, &expected_str),
        crate::task::SemanticFormat::Csv => compare_csv(&actual_str, &expected_str),
    };

    Ok(MetricResult {
        passed,
        metric: "file_compare_semantic".to_string(),
        expected: format!("file: {expected_path} (format: {format:?})"),
        actual: format!("container: {actual_path}"),
        detail,
    })
}

// --- Comparison helpers ---

/// Normalize text by trimming trailing whitespace on each line and trailing newlines.
fn normalize_text(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// Find the first line where two texts differ.
fn first_diff_line(actual: &str, expected: &str) -> String {
    let actual_lines: Vec<&str> = actual.lines().collect();
    let expected_lines: Vec<&str> = expected.lines().collect();

    for (i, (a, e)) in actual_lines.iter().zip(expected_lines.iter()).enumerate() {
        if a != e {
            return format!(
                "First difference at line {}: actual='{}', expected='{}'",
                i + 1,
                a,
                e
            );
        }
    }

    if actual_lines.len() != expected_lines.len() {
        format!(
            "Files have different number of lines (actual: {}, expected: {})",
            actual_lines.len(),
            expected_lines.len()
        )
    } else {
        "Files are identical".to_string()
    }
}

/// Compare two JSON strings semantically (ignoring key order, whitespace).
fn compare_json(actual: &str, expected: &str) -> (bool, String) {
    let actual_val: Result<serde_json::Value, _> = serde_json::from_str(actual);
    let expected_val: Result<serde_json::Value, _> = serde_json::from_str(expected);

    match (actual_val, expected_val) {
        (Ok(a), Ok(e)) => {
            if a == e {
                (true, "JSON data structures match".to_string())
            } else {
                (false, "JSON data structures differ".to_string())
            }
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as JSON: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as JSON: {e}")),
    }
}

/// Compare two YAML strings semantically.
fn compare_yaml(actual: &str, expected: &str) -> (bool, String) {
    let actual_val: Result<serde_yaml::Value, _> = serde_yaml::from_str(actual);
    let expected_val: Result<serde_yaml::Value, _> = serde_yaml::from_str(expected);

    match (actual_val, expected_val) {
        (Ok(a), Ok(e)) => {
            if a == e {
                (true, "YAML data structures match".to_string())
            } else {
                (false, "YAML data structures differ".to_string())
            }
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as YAML: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as YAML: {e}")),
    }
}

/// Compare two XML strings semantically.
///
/// Parses XML into serde_json::Value via quick-xml, then compares the resulting
/// data structures. This ignores whitespace differences and attribute ordering.
fn compare_xml(actual: &str, expected: &str) -> (bool, String) {
    let actual_val = parse_xml_to_value(actual);
    let expected_val = parse_xml_to_value(expected);

    match (actual_val, expected_val) {
        (Ok(a), Ok(e)) => {
            if a == e {
                (true, "XML data structures match".to_string())
            } else {
                (false, "XML data structures differ".to_string())
            }
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as XML: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as XML: {e}")),
    }
}

/// Parse XML string into a serde_json::Value for comparison.
fn parse_xml_to_value(xml: &str) -> Result<serde_json::Value, String> {
    quick_xml::de::from_str(xml).map_err(|e| format!("{e}"))
}

/// Compare two CSV strings semantically (row-by-row, cell-by-cell, ignoring
/// whitespace differences in cell values).
fn compare_csv(actual: &str, expected: &str) -> (bool, String) {
    let actual_rows = parse_csv(actual);
    let expected_rows = parse_csv(expected);

    match (actual_rows, expected_rows) {
        (Ok(a), Ok(e)) => {
            if a.len() != e.len() {
                return (
                    false,
                    format!(
                        "CSV row count differs (actual: {}, expected: {})",
                        a.len(),
                        e.len()
                    ),
                );
            }
            for (row_idx, (ar, er)) in a.iter().zip(e.iter()).enumerate() {
                if ar.len() != er.len() {
                    return (
                        false,
                        format!(
                            "CSV column count differs at row {} (actual: {}, expected: {})",
                            row_idx + 1,
                            ar.len(),
                            er.len()
                        ),
                    );
                }
                for (col_idx, (ac, ec)) in ar.iter().zip(er.iter()).enumerate() {
                    if ac.trim() != ec.trim() {
                        return (
                            false,
                            format!(
                                "CSV cell differs at row {}, col {}: actual='{}', expected='{}'",
                                row_idx + 1,
                                col_idx + 1,
                                ac,
                                ec
                            ),
                        );
                    }
                }
            }
            (true, "CSV data matches".to_string())
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as CSV: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as CSV: {e}")),
    }
}

/// Parse a CSV string into a Vec of rows, each a Vec of cell strings.
fn parse_csv(csv_str: &str) -> Result<Vec<Vec<String>>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_str.as_bytes());

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| format!("{e}"))?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- normalize_text tests ---

    #[test]
    fn test_normalize_text_trims_trailing_whitespace() {
        assert_eq!(normalize_text("hello   \nworld  \n"), "hello\nworld");
    }

    #[test]
    fn test_normalize_text_trims_trailing_newlines() {
        assert_eq!(normalize_text("hello\nworld\n\n\n"), "hello\nworld");
    }

    #[test]
    fn test_normalize_text_preserves_leading_whitespace() {
        assert_eq!(normalize_text("  hello\n  world"), "  hello\n  world");
    }

    // --- first_diff_line tests ---

    #[test]
    fn test_first_diff_identical() {
        let result = first_diff_line("abc\ndef", "abc\ndef");
        assert!(result.contains("identical"));
    }

    #[test]
    fn test_first_diff_different_line() {
        let result = first_diff_line("abc\nXXX", "abc\ndef");
        assert!(result.contains("line 2"));
    }

    #[test]
    fn test_first_diff_different_length() {
        let result = first_diff_line("abc", "abc\ndef");
        assert!(result.contains("different number of lines"));
    }

    // --- compare_json tests ---

    #[test]
    fn test_json_equal_same_order() {
        let (passed, _) = compare_json(r#"{"a":1,"b":2}"#, r#"{"a":1,"b":2}"#);
        assert!(passed);
    }

    #[test]
    fn test_json_equal_different_order() {
        let (passed, _) = compare_json(r#"{"b":2,"a":1}"#, r#"{"a":1,"b":2}"#);
        assert!(passed);
    }

    #[test]
    fn test_json_equal_with_whitespace() {
        let actual = r#"{ "a" : 1, "b" : 2 }"#;
        let expected = r#"{"a":1,"b":2}"#;
        let (passed, _) = compare_json(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_json_not_equal() {
        let (passed, detail) = compare_json(r#"{"a":1}"#, r#"{"a":2}"#);
        assert!(!passed);
        assert!(detail.contains("differ"));
    }

    #[test]
    fn test_json_invalid_actual() {
        let (passed, detail) = compare_json("not json", r#"{"a":1}"#);
        assert!(!passed);
        assert!(detail.contains("parse actual"));
    }

    #[test]
    fn test_json_invalid_expected() {
        let (passed, detail) = compare_json(r#"{"a":1}"#, "not json");
        assert!(!passed);
        assert!(detail.contains("parse expected"));
    }

    // --- compare_yaml tests ---

    #[test]
    fn test_yaml_equal() {
        let actual = "a: 1\nb: 2\n";
        let expected = "b: 2\na: 1\n";
        let (passed, _) = compare_yaml(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_yaml_not_equal() {
        let (passed, _) = compare_yaml("a: 1\n", "a: 2\n");
        assert!(!passed);
    }

    #[test]
    fn test_yaml_invalid() {
        let (passed, detail) = compare_yaml(":\n  :\n    - invalid:", "a: 1\n");
        assert!(!passed);
        assert!(detail.contains("parse"));
    }

    // --- compare_xml tests ---

    #[test]
    fn test_xml_equal() {
        let actual = "<root><a>1</a><b>2</b></root>";
        let expected = "<root><a>1</a><b>2</b></root>";
        let (passed, _) = compare_xml(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_xml_equal_with_whitespace() {
        let actual = "<root>\n  <a>1</a>\n  <b>2</b>\n</root>";
        let expected = "<root><a>1</a><b>2</b></root>";
        let (passed, _) = compare_xml(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_xml_not_equal() {
        let actual = "<root><a>1</a></root>";
        let expected = "<root><a>2</a></root>";
        let (passed, _) = compare_xml(actual, expected);
        assert!(!passed);
    }

    #[test]
    fn test_xml_invalid() {
        let (passed, detail) = compare_xml("<not>closed", "<root/>");
        assert!(!passed);
        assert!(detail.contains("parse"));
    }

    // --- compare_csv tests ---

    #[test]
    fn test_csv_equal() {
        let actual = "a,b,c\n1,2,3\n";
        let expected = "a,b,c\n1,2,3\n";
        let (passed, _) = compare_csv(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_csv_equal_whitespace_trimmed() {
        let actual = "a , b , c\n1 , 2 , 3\n";
        let expected = "a,b,c\n1,2,3\n";
        let (passed, _) = compare_csv(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_csv_different_values() {
        let actual = "a,b,c\n1,X,3\n";
        let expected = "a,b,c\n1,2,3\n";
        let (passed, detail) = compare_csv(actual, expected);
        assert!(!passed);
        assert!(detail.contains("row 2"));
        assert!(detail.contains("col 2"));
    }

    #[test]
    fn test_csv_different_row_count() {
        let actual = "a,b\n1,2\n";
        let expected = "a,b\n1,2\n3,4\n";
        let (passed, detail) = compare_csv(actual, expected);
        assert!(!passed);
        assert!(detail.contains("row count"));
    }

    // --- parse_csv tests ---

    #[test]
    fn test_parse_csv_basic() {
        let rows = parse_csv("a,b,c\n1,2,3\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a", "b", "c"]);
        assert_eq!(rows[1], vec!["1", "2", "3"]);
    }

    #[test]
    fn test_parse_csv_empty() {
        let rows = parse_csv("").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_parse_csv_quoted_fields() {
        let rows = parse_csv(r#""hello, world",b,c"#).unwrap();
        assert_eq!(rows[0][0], "hello, world");
    }

    // --- JSON nested comparison ---

    #[test]
    fn test_json_nested_equal() {
        let actual = r#"{"outer":{"inner":[1,2,3]},"key":"value"}"#;
        let expected = r#"{"key":"value","outer":{"inner":[1,2,3]}}"#;
        let (passed, _) = compare_json(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_json_array_order_matters() {
        // JSON arrays are ordered, so different order should fail
        let (passed, _) = compare_json("[1,2,3]", "[3,2,1]");
        assert!(!passed);
    }

    // --- YAML with nested structures ---

    #[test]
    fn test_yaml_nested() {
        let actual = "outer:\n  inner:\n    - 1\n    - 2\n";
        let expected = "outer:\n  inner:\n    - 1\n    - 2\n";
        let (passed, _) = compare_yaml(actual, expected);
        assert!(passed);
    }

    // --- parse_xml_to_value ---

    #[test]
    fn test_parse_xml_simple() {
        let val = parse_xml_to_value("<root><a>1</a></root>").unwrap();
        assert!(val.is_object() || val.is_string());
    }
}
