use anyhow::Result;
use std::path::Path;

use crate::checks::{CheckResult, CheckStage};
use crate::process;

pub async fn run(test_files: &[String], project_dir: &Path) -> Result<CheckResult> {
    if test_files.is_empty() {
        return Ok(CheckResult {
            stage: CheckStage::Vitest,
            success: true,
            files_checked: vec![],
            errors: vec![],
            files_modified: vec![],
        });
    }

    // vitest run -u updates snapshots, --reporter=json gives parseable output
    let mut args: Vec<&str> = vec!["run", "--reporter=json", "-u"];
    args.extend(test_files.iter().map(|f| f.as_str()));

    let output = process::run_command("./node_modules/.bin/vitest", &args, project_dir).await?;

    let errors = if output.exit_code != 0 {
        parse_vitest_failures(&output.stdout, &output.stderr)
    } else {
        vec![]
    };

    Ok(CheckResult {
        stage: CheckStage::Vitest,
        success: output.exit_code == 0,
        files_checked: test_files.to_vec(),
        errors,
        files_modified: vec![],
    })
}

/// Parse vitest JSON output for failure messages.
/// Falls back to raw output if JSON parsing fails.
fn parse_vitest_failures(stdout: &str, stderr: &str) -> Vec<String> {
    // Try to parse the JSON reporter output
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout) {
        let mut errors = Vec::new();
        if let Some(test_results) = json.get("testResults").and_then(|v| v.as_array()) {
            for result in test_results {
                if result.get("status").and_then(|s| s.as_str()) == Some("failed") {
                    let file = result
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown");
                    if let Some(messages) =
                        result.get("assertionResults").and_then(|v| v.as_array())
                    {
                        for msg in messages {
                            if msg.get("status").and_then(|s| s.as_str()) == Some("failed") {
                                let title = msg
                                    .get("fullName")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown test");
                                let failure_messages = msg
                                    .get("failureMessages")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|m| m.as_str())
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                                    .unwrap_or_default();
                                errors.push(format!("{}: {} — {}", file, title, failure_messages));
                            }
                        }
                    }
                }
            }
        }
        if !errors.is_empty() {
            return errors;
        }
    }

    // Fallback: return raw combined output
    let combined = format!("{}\n{}", stdout, stderr);
    if combined.trim().is_empty() {
        vec![]
    } else {
        vec![combined]
    }
}
