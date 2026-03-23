//! Bug report writer for QA mode.
//!
//! When `--qa` is enabled, the agent can emit `BUG` commands to report
//! application bugs. Each bug gets a unique ID and is written as a
//! markdown file in the `bugs/` subdirectory of the artifacts folder.

use std::path::{Path, PathBuf};

use tracing::info;

/// Writes bug reports as markdown files with incrementing IDs.
pub struct BugReporter {
    bugs_dir: PathBuf,
    counter: usize,
}

impl BugReporter {
    /// Create a new bug reporter, creating the `bugs/` subdirectory.
    pub fn new(artifacts_dir: &Path) -> std::io::Result<Self> {
        let bugs_dir = artifacts_dir.join("bugs");
        std::fs::create_dir_all(&bugs_dir)?;
        Ok(Self {
            bugs_dir,
            counter: 0,
        })
    }

    /// Record a bug report, writing a markdown file.
    ///
    /// Returns the bug ID (e.g., "BUG-001").
    pub fn report_bug(
        &mut self,
        step: usize,
        description: &str,
        screenshot_path: Option<&str>,
        a11y_tree_text: Option<&str>,
        bash_output: Option<&str>,
    ) -> std::io::Result<String> {
        let next = self.counter + 1;
        let bug_id = format!("BUG-{:03}", next);

        let summary = description.lines().next().unwrap_or("No summary");

        let mut md = format!(
            "# {bug_id}\n\n\
             **Step:** {step}\n\
             **Summary:** {summary}\n\n\
             ## Description\n\n\
             {description}\n\n"
        );

        if let Some(screenshot) = screenshot_path {
            md.push_str(&format!(
                "## Screenshot\n\n\
                 ![screenshot]({screenshot})\n\n"
            ));
        }

        if let Some(output) = bash_output {
            if !output.trim().is_empty() {
                let truncated: String = output.chars().take(5000).collect();
                let suffix = if output.chars().count() > 5000 {
                    "\n... (truncated)"
                } else {
                    ""
                };
                md.push_str(&format!(
                    "## Diagnostic Evidence\n\n\
                     ```\n{truncated}{suffix}\n```\n\n"
                ));
            }
        }

        if let Some(a11y) = a11y_tree_text {
            // Truncate large a11y trees to keep reports readable
            let truncated: String = a11y.chars().take(3000).collect();
            let suffix = if a11y.chars().count() > 3000 {
                "\n... (truncated)"
            } else {
                ""
            };
            md.push_str(&format!(
                "## Accessibility Tree State\n\n\
                 ```\n{truncated}{suffix}\n```\n"
            ));
        }

        let path = self.bugs_dir.join(format!("{bug_id}.md"));
        std::fs::write(&path, &md)?;

        // Only increment after successful write
        self.counter = next;
        info!("Wrote bug report: {} -> {}", bug_id, path.display());

        Ok(bug_id)
    }

    /// Number of bugs reported so far.
    pub fn bug_count(&self) -> usize {
        self.counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_bug_reporter_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let reporter = BugReporter::new(tmp.path()).unwrap();
        assert!(tmp.path().join("bugs").is_dir());
        assert_eq!(reporter.bug_count(), 0);
    }

    #[test]
    fn test_bug_reporter_generates_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reporter = BugReporter::new(tmp.path()).unwrap();

        let id1 = reporter
            .report_bug(1, "First bug", None, None, None)
            .unwrap();
        let id2 = reporter
            .report_bug(2, "Second bug", None, None, None)
            .unwrap();

        assert_eq!(id1, "BUG-001");
        assert_eq!(id2, "BUG-002");
        assert_eq!(reporter.bug_count(), 2);
    }

    #[test]
    fn test_bug_report_markdown_format() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reporter = BugReporter::new(tmp.path()).unwrap();

        reporter
            .report_bug(
                5,
                "Save dialog loses extension\nExpected .txt but got nothing",
                Some("../step_005.png"),
                Some("button\tOK\t\tGtkButton"),
                Some("$ bash block 1:\nTraceback: FileNotFoundError at save.py:42"),
            )
            .unwrap();

        let content = fs::read_to_string(tmp.path().join("bugs/BUG-001.md")).unwrap();
        assert!(content.contains("# BUG-001"));
        assert!(content.contains("**Step:** 5"));
        assert!(content.contains("**Summary:** Save dialog loses extension"));
        assert!(content.contains("Expected .txt but got nothing"));
        assert!(content.contains("![screenshot](../step_005.png)"));
        assert!(content.contains("Diagnostic Evidence"));
        assert!(content.contains("FileNotFoundError"));
        assert!(content.contains("GtkButton"));
    }

    #[test]
    fn test_bug_report_without_optional_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reporter = BugReporter::new(tmp.path()).unwrap();

        reporter
            .report_bug(1, "Simple bug", None, None, None)
            .unwrap();

        let content = fs::read_to_string(tmp.path().join("bugs/BUG-001.md")).unwrap();
        assert!(content.contains("# BUG-001"));
        assert!(content.contains("Simple bug"));
        assert!(!content.contains("Screenshot"));
        assert!(!content.contains("Diagnostic Evidence"));
        assert!(!content.contains("Accessibility Tree"));
    }
}
