use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;

use crate::process::ProcessOutput;

const CLAUDE_TIMEOUT: Duration = Duration::from_secs(120);

/// Invoke the Claude CLI to fix errors in specific files.
pub async fn fix_errors(
    files: &[String],
    error_output: &str,
    stage_name: &str,
    project_dir: &Path,
) -> Result<ProcessOutput> {
    let files_str = files.join(", ");
    let prompt = format!(
        "Fix these {} errors in the following files: {}\n\nErrors:\n{}",
        stage_name, files_str, error_output
    );

    let fut = tokio::process::Command::new("claude")
        .arg("-p")
        .arg(&prompt)
        .arg("--dangerously-skip-permissions")
        .current_dir(project_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let output = tokio::time::timeout(CLAUDE_TIMEOUT, fut)
        .await
        .context("Claude CLI timed out")?
        .context("Failed to execute claude CLI")?;

    Ok(ProcessOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
