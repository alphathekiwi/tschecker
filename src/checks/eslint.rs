use anyhow::Result;
use std::path::Path;

use crate::checks::{CheckResult, CheckStage};
use crate::process;

pub async fn run(files: &[String], project_dir: &Path) -> Result<CheckResult> {
    if files.is_empty() {
        return Ok(CheckResult {
            stage: CheckStage::Eslint,
            success: true,
            files_checked: vec![],
            errors: vec![],
            files_modified: vec![],
        });
    }

    let mut args: Vec<&str> = vec!["--fix", "--cache", "--cache-location", ".cache/eslint/", "--quiet"];
    args.extend(files.iter().map(|f| f.as_str()));

    let output =
        process::run_command("./node_modules/.bin/eslint", &args, project_dir).await?;

    let errors = if output.exit_code != 0 {
        parse_eslint_errors(&output.stdout, &output.stderr)
    } else {
        vec![]
    };

    Ok(CheckResult {
        stage: CheckStage::Eslint,
        success: output.exit_code == 0,
        files_checked: files.to_vec(),
        errors,
        files_modified: files.to_vec(),
    })
}

/// Parse eslint output into individual error strings.
/// ESLint outputs errors grouped by file, one error per line after the filename.
fn parse_eslint_errors(stdout: &str, stderr: &str) -> Vec<String> {
    let combined = format!("{}\n{}", stdout, stderr);
    let lines: Vec<&str> = combined.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return vec![];
    }
    vec![combined]
}
