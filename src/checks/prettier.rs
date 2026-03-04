use anyhow::Result;
use std::path::Path;

use crate::checks::{CheckResult, CheckStage};
use crate::process;

pub async fn run(files: &[String], project_dir: &Path) -> Result<CheckResult> {
    if files.is_empty() {
        return Ok(CheckResult {
            stage: CheckStage::Prettier,
            success: true,
            files_checked: vec![],
            errors: vec![],
            files_modified: vec![],
        });
    }

    let mut args: Vec<&str> = vec!["--write"];
    args.extend(files.iter().map(|f| f.as_str()));

    let output = process::run_command("./node_modules/.bin/prettier", &args, project_dir).await?;

    Ok(CheckResult {
        stage: CheckStage::Prettier,
        success: output.exit_code == 0,
        files_checked: files.to_vec(),
        errors: if output.exit_code != 0 {
            vec![format!("{}\n{}", output.stdout, output.stderr)]
        } else {
            vec![]
        },
        files_modified: if output.exit_code == 0 {
            files.to_vec()
        } else {
            vec![]
        },
    })
}
