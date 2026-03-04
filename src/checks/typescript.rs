use anyhow::Result;
use std::path::Path;

use crate::checks::{CheckResult, CheckStage};
use crate::process;

pub async fn run(changed_files: &[String], project_dir: &Path) -> Result<CheckResult> {
    if changed_files.is_empty() {
        return Ok(CheckResult {
            stage: CheckStage::Typescript,
            success: true,
            files_checked: vec![],
            errors: vec![],
            files_modified: vec![],
        });
    }

    // tsc checks the entire project — we filter errors to changed files only
    let output =
        process::run_command("./node_modules/.bin/tsc", &["--noEmit", "--pretty", "false"], project_dir)
            .await?;

    if output.exit_code == 0 {
        return Ok(CheckResult {
            stage: CheckStage::Typescript,
            success: true,
            files_checked: changed_files.to_vec(),
            errors: vec![],
            files_modified: vec![],
        });
    }

    // tsc with --pretty false outputs: src/path/file.ts(line,col): error TS1234: message
    let tsc_output = if output.stdout.contains("error TS") {
        &output.stdout
    } else {
        &output.stderr
    };

    let relevant_errors = filter_tsc_errors(tsc_output, changed_files);

    Ok(CheckResult {
        stage: CheckStage::Typescript,
        success: relevant_errors.is_empty(),
        files_checked: changed_files.to_vec(),
        errors: relevant_errors,
        files_modified: vec![],
    })
}

/// Filter tsc error lines to only those referencing our changed files.
/// tsc format: `src/path/file.ts(line,col): error TS1234: message`
fn filter_tsc_errors(tsc_output: &str, changed_files: &[String]) -> Vec<String> {
    tsc_output
        .lines()
        .filter(|line| changed_files.iter().any(|f| line.starts_with(f.as_str())))
        .map(|line| line.to_string())
        .collect()
}
