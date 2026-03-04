use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use crate::checks::{self, CheckResult, CheckStage};
use crate::{claude, files, gitbutler, process};

pub struct PipelineConfig {
    pub project_dir: PathBuf,
    pub repo_path: PathBuf,
    pub max_retries: u32,
    pub but_path: String,
    pub no_commit: bool,
    pub dry_run: bool,
    pub verbose: bool,
}

pub async fn run(
    branch: &gitbutler::Branch,
    changed_files: &[String],
    config: &PipelineConfig,
) -> Result<bool> {
    let project_dir = &config.project_dir;

    info!(branch = %branch.name, files = changed_files.len(), "Starting pipeline");

    if changed_files.is_empty() {
        info!(branch = %branch.name, "No changed files, skipping");
        return Ok(true);
    }

    if config.dry_run {
        print_dry_run(branch, changed_files, project_dir);
        return Ok(true);
    }

    // Stage 1: Prettier (deterministic, no Claude)
    let prettier_files = files::filter_by_extensions(changed_files, files::PRETTIER_EXTENSIONS);
    if !prettier_files.is_empty() {
        info!(stage = "prettier", files = prettier_files.len(), "Running");
        log_cmd(config.verbose, "./node_modules/.bin/prettier --write", &prettier_files);
        let result = checks::prettier::run(&prettier_files, project_dir).await?;
        if !result.success {
            error!("Prettier failed: {:?}", result.errors);
            return Ok(false);
        }
        info!(stage = "prettier", "Passed");
    }

    // Stage 2: ESLint (auto-fix, then Claude for remaining)
    let eslint_files = files::filter_by_extensions(changed_files, files::ESLINT_EXTENSIONS);
    if !eslint_files.is_empty() {
        info!(stage = "eslint", files = eslint_files.len(), "Running");
        log_cmd(config.verbose, "./node_modules/.bin/eslint --fix --cache --cache-location .cache/eslint/ --quiet", &eslint_files);
        let mut result = checks::eslint::run(&eslint_files, project_dir).await?;
        if !result.success {
            result = run_fix_loop(CheckStage::Eslint, &eslint_files, &result.errors, config, project_dir).await?;
        }
        if !result.success {
            error!(branch = %branch.name, "ESLint errors remain after {} retries", config.max_retries);
            return Ok(false);
        }
        info!(stage = "eslint", "Passed");
    }

    // Stage 3: TypeScript (full project, filter to changed files, Claude for fixes)
    let ts_files = files::filter_by_extensions(changed_files, files::TYPESCRIPT_EXTENSIONS);
    if !ts_files.is_empty() {
        info!(stage = "tsc", files = ts_files.len(), "Running");
        log_cmd(config.verbose, "./node_modules/.bin/tsc --noEmit --pretty false", &[]);
        log_files(config.verbose, &ts_files);
        let mut result = checks::typescript::run(&ts_files, project_dir).await?;
        if !result.success {
            result = run_fix_loop(CheckStage::Typescript, &ts_files, &result.errors, config, project_dir).await?;
        }
        if !result.success {
            error!(branch = %branch.name, "TypeScript errors remain after {} retries", config.max_retries);
            return Ok(false);
        }
        info!(stage = "tsc", "Passed");
    }

    // Stage 4: Vitest (update snapshots, Claude for failures)
    let test_files = files::collect_test_files(changed_files, project_dir);
    if !test_files.is_empty() {
        let snapshots = files::find_snapshot_files(&test_files, project_dir);
        if !snapshots.is_empty() {
            info!(stage = "vitest", snapshots = snapshots.len(), "Snapshots to update");
            log_files(config.verbose, &snapshots);
        }
        info!(stage = "vitest", files = test_files.len(), "Running");
        log_cmd(config.verbose, "./node_modules/.bin/vitest run --reporter=json -u", &test_files);
        let mut result = checks::vitest::run(&test_files, project_dir).await?;
        if !result.success {
            result = run_fix_loop(CheckStage::Vitest, &test_files, &result.errors, config, project_dir).await?;
        }
        if !result.success {
            error!(branch = %branch.name, "Vitest failures remain after {} retries", config.max_retries);
            return Ok(false);
        }
        info!(stage = "vitest", "Passed");
    }

    // All checks passed — commit
    if !config.no_commit {
        commit_results(branch, &config.repo_path, &config.but_path).await?;
    }

    info!(branch = %branch.name, "Pipeline completed successfully");
    Ok(true)
}

async fn run_fix_loop(
    stage: CheckStage,
    files: &[String],
    initial_errors: &[String],
    config: &PipelineConfig,
    project_dir: &Path,
) -> Result<CheckResult> {
    let mut errors = initial_errors.to_vec();

    for attempt in 1..=config.max_retries {
        info!(stage = %stage, attempt, "Invoking Claude for fixes");

        let error_text = errors.join("\n");
        match claude::fix_errors(files, &error_text, &stage.to_string(), project_dir).await {
            Ok(output) => {
                if output.exit_code != 0 {
                    warn!(stage = %stage, attempt, "Claude exited with code {}", output.exit_code);
                }
            }
            Err(e) => {
                warn!(stage = %stage, attempt, "Claude invocation failed: {}", e);
            }
        }

        // Re-run the check
        let result = match stage {
            CheckStage::Prettier => checks::prettier::run(files, project_dir).await?,
            CheckStage::Eslint => checks::eslint::run(files, project_dir).await?,
            CheckStage::Typescript => checks::typescript::run(files, project_dir).await?,
            CheckStage::Vitest => checks::vitest::run(files, project_dir).await?,
        };

        if result.success {
            info!(stage = %stage, attempt, "Fixed successfully");
            return Ok(result);
        }

        errors = result.errors.clone();
        warn!(stage = %stage, attempt, remaining = errors.len(), "Errors remain");

        if attempt == config.max_retries {
            return Ok(result);
        }
    }

    // Should not reach here, but just in case
    Ok(CheckResult {
        stage,
        success: false,
        files_checked: files.to_vec(),
        errors,
        files_modified: vec![],
    })
}

async fn commit_results(
    branch: &gitbutler::Branch,
    repo_path: &Path,
    but_path: &str,
) -> Result<()> {
    let output = process::run_command(
        but_path,
        &[
            "commit",
            &branch.cli_id,
            "-m",
            "tschecker: auto-fix prettier/lint/type/test issues",
        ],
        repo_path,
    )
    .await?;

    if output.exit_code != 0 {
        let stderr = output.stderr.trim();
        if stderr.contains("No changes to commit") {
            info!(branch = %branch.name, "All checks passed with no changes needed");
            return Ok(());
        }
        anyhow::bail!("but commit failed: {}", stderr);
    }

    info!(branch = %branch.name, "Committed fixes");
    Ok(())
}

fn log_cmd(verbose: bool, base_cmd: &str, files: &[String]) {
    if !verbose {
        return;
    }
    if files.is_empty() {
        info!("$ {}", base_cmd);
    } else {
        info!("$ {} {}", base_cmd, files.join(" "));
    }
}

fn log_files(verbose: bool, files: &[String]) {
    if !verbose {
        return;
    }
    for f in files {
        info!("  {}", f);
    }
}

fn print_dry_run(branch: &gitbutler::Branch, changed_files: &[String], project_dir: &Path) {
    println!("Branch: {} ({})", branch.name, branch.cli_id);
    println!("Changed files ({}):", changed_files.len());
    for f in changed_files {
        println!("  {}", f);
    }

    let prettier = files::filter_by_extensions(changed_files, files::PRETTIER_EXTENSIONS);
    let eslint = files::filter_by_extensions(changed_files, files::ESLINT_EXTENSIONS);
    let ts = files::filter_by_extensions(changed_files, files::TYPESCRIPT_EXTENSIONS);
    let test = files::collect_test_files(changed_files, project_dir);

    println!("Prettier targets ({}):", prettier.len());
    println!("  $ ./node_modules/.bin/prettier --write {}", prettier.join(" "));
    println!("ESLint targets ({}):", eslint.len());
    println!("  $ ./node_modules/.bin/eslint --fix --cache --cache-location .cache/eslint/ --quiet {}", eslint.join(" "));
    println!("TypeScript targets ({}):", ts.len());
    println!("  $ ./node_modules/.bin/tsc --noEmit --pretty false");
    println!("  (errors filtered to changed files only)");
    println!("Vitest targets ({}):", test.len());
    println!("  $ ./node_modules/.bin/vitest run --reporter=json -u {}", test.join(" "));
    let snapshots = files::find_snapshot_files(&test, project_dir);
    if !snapshots.is_empty() {
        println!("Snapshots to update ({}):", snapshots.len());
        for f in &snapshots { println!("  {}", f); }
    }
    println!();
}
