mod checks;
mod claude;
mod cli;
mod files;
mod gitbutler;
mod pipeline;
mod process;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .compact()
        .init();

    let mut cli = cli::Cli::parse();

    // Parse --stage early so we fail fast on invalid values
    let stage_filter = cli.stage.as_deref()
        .map(|s| s.parse::<checks::CheckStage>())
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?;

    if cli.update {
        return self_update().await;
    }

    if cli.post_commit {
        return run_post_commit_mode(&cli, stage_filter).await;
    }

    // Direct file mode: tschecker file1.ts file2.tsx ...
    if !cli.files.is_empty() {
        return run_files_mode(&cli, stage_filter).await;
    }

    // No args at all (no files, no -b, no -a, no -p, no --update, no --dry-run):
    // default to -a --no-fixes --no-commit
    let no_args = cli.branch.is_none() && !cli.all && !cli.dry_run;
    if no_args {
        cli.all = true;
        cli.no_fixes = true;
        cli.no_commit = true;
    }

    let repo_path = cli.repo_path.canonicalize().context("Invalid repo path")?;
    let project_dir = resolve_project_dir(&repo_path, &cli.project_dir)?;

    let gb_active = gitbutler::is_workspace_active(&repo_path).await;

    if !gb_active && cli.all {
        // GitButler not active — treat -a as "run on current branch"
        return run_plain_git_mode(&cli, &repo_path, &project_dir, stage_filter).await;
    }

    info!("Fetching GitButler status...");
    let status = gitbutler::get_status(&cli.but_path, &repo_path).await?;
    let applied = gitbutler::applied_branches(&status);

    if applied.is_empty() {
        anyhow::bail!("No applied branches found in GitButler workspace");
    }

    // Resolve which branches to check
    let branches_to_check: Vec<&gitbutler::Branch> = if cli.all {
        applied
    } else if let Some(ref name) = cli.branch {
        let branch = applied
            .iter()
            .find(|b| b.name == *name || b.cli_id == *name)
            .ok_or_else(|| {
                let names: Vec<_> = applied.iter().map(|b| b.name.as_str()).collect();
                anyhow::anyhow!(
                    "Branch '{}' not found. Available: {}",
                    name,
                    names.join(", ")
                )
            })?;
        vec![branch]
    } else {
        // Interactive selector
        let idx = ui::select_branch(&applied)?;
        vec![applied[idx]]
    };

    let config = pipeline::PipelineConfig {
        project_dir: project_dir.clone(),
        repo_path: repo_path.clone(),
        max_retries: cli.max_retries,
        but_path: cli.but_path.clone(),
        no_commit: cli.no_commit,
        no_fixes: cli.no_fixes,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
        use_git_commit: false,
        stage: stage_filter,
    };

    let mut all_passed = true;

    for branch in &branches_to_check {
        let all_files = gitbutler::branch_changed_files(&status, &branch.name);
        let project_files = gitbutler::filter_to_project(&all_files, &cli.project_dir);

        if project_files.is_empty() {
            info!(branch = %ui::cyan(&branch.name), "No files in {} — skipping", cli.project_dir);
            continue;
        }

        let passed = pipeline::run(branch, &project_files, &config).await?;
        if !passed {
            all_passed = false;
            error!(branch = %ui::cyan(&branch.name), "Pipeline failed");
        }
    }

    if all_passed {
        info!("All branches passed");
    } else {
        std::process::exit(1);
    }

    Ok(())
}

/// Direct file mode: tschecker file1.ts file2.tsx ...
/// Skips branch resolution, expands to related test files, implies --no-fixes --no-commit
async fn run_files_mode(cli: &cli::Cli, stage_filter: Option<checks::CheckStage>) -> Result<()> {
    let repo_path = cli.repo_path.canonicalize().context("Invalid repo path")?;
    let project_dir = resolve_project_dir(&repo_path, &cli.project_dir)?;

    // Expand the provided files to include related test files
    let mut all_files: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for file in &cli.files {
        if seen.insert(file.clone()) {
            all_files.push(file.clone());
        }
        // Find related test file if this isn't already a test
        if let Some(test_path) = files::find_test_file(file, &project_dir) {
            let test_str = test_path.to_string_lossy().to_string();
            if seen.insert(test_str.clone()) {
                all_files.push(test_str);
            }
        }
    }

    all_files.sort();

    if all_files.is_empty() {
        info!("No files to check");
        return Ok(());
    }

    info!(files = all_files.len(), "Running on specified files");

    let branch = gitbutler::Branch {
        cli_id: "direct".to_string(),
        name: "direct".to_string(),
        commits: vec![],
    };

    let config = pipeline::PipelineConfig {
        project_dir,
        repo_path,
        max_retries: cli.max_retries,
        but_path: cli.but_path.clone(),
        no_commit: true,
        no_fixes: true,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
        use_git_commit: false,
        stage: stage_filter,
    };

    let passed = pipeline::run(&branch, &all_files, &config).await?;

    if passed {
        info!("All checks passed");
    } else {
        error!("Checks failed");
        std::process::exit(1);
    }

    Ok(())
}

async fn run_plain_git_mode(
    cli: &cli::Cli,
    repo_path: &std::path::Path,
    project_dir: &std::path::Path,
    stage_filter: Option<checks::CheckStage>,
) -> Result<()> {
    let branch_name = gitbutler::current_branch_name(repo_path).await?;
    info!(branch = %ui::cyan(&branch_name), "GitButler not active, running on current branch");

    let all_files = gitbutler::git_changed_files(repo_path, cli.base_branch.as_deref()).await?;
    let project_files = gitbutler::filter_to_project(&all_files, &cli.project_dir);

    if project_files.is_empty() {
        info!(branch = %ui::cyan(&branch_name), "No changed files in {} — nothing to check", cli.project_dir);
        return Ok(());
    }

    let branch = gitbutler::Branch {
        cli_id: branch_name.clone(),
        name: branch_name.clone(),
        commits: vec![],
    };

    let config = pipeline::PipelineConfig {
        project_dir: project_dir.to_path_buf(),
        repo_path: repo_path.to_path_buf(),
        max_retries: cli.max_retries,
        but_path: cli.but_path.clone(),
        no_commit: cli.no_commit,
        no_fixes: cli.no_fixes,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
        use_git_commit: true,
        stage: stage_filter,
    };

    let passed = pipeline::run(&branch, &project_files, &config).await?;

    if passed {
        info!(branch = %ui::cyan(&branch_name), "All checks passed");
    } else {
        error!(branch = %ui::cyan(&branch_name), "Pipeline failed");
        std::process::exit(1);
    }

    Ok(())
}

async fn run_post_commit_mode(cli: &cli::Cli, stage_filter: Option<checks::CheckStage>) -> Result<()> {
    let repo_path = cli.repo_path.canonicalize().context("Invalid repo path")?;
    let project_dir = resolve_project_dir(&repo_path, &cli.project_dir)?;

    // Get the last real commit hash (skip GitButler's workspace merge commit)
    let log_output = process::run_command(
        "git",
        &["log", "--no-merges", "-1", "--format=%H"],
        &repo_path,
    )
    .await?;
    let commit_hash = log_output.stdout.trim().to_string();

    if commit_hash.is_empty() {
        warn!("No commits found, skipping post-commit checks");
        return Ok(());
    }

    info!(commit = %commit_hash, "Post-commit hook triggered");

    // Get GitButler status and find which branch contains this commit
    let status = gitbutler::get_status(&cli.but_path, &repo_path).await?;

    let branch = match gitbutler::find_branch_by_commit(&status, &commit_hash) {
        Some(b) => b,
        None => {
            info!(commit = %commit_hash, "Commit not found in any GitButler branch, skipping");
            return Ok(());
        }
    };

    info!(branch = %ui::cyan(&branch.name), "Detected branch from commit");

    let all_files = gitbutler::branch_changed_files(&status, &branch.name);
    let project_files = gitbutler::filter_to_project(&all_files, &cli.project_dir);

    if project_files.is_empty() {
        info!(branch = %ui::cyan(&branch.name), "No files in {} — skipping", cli.project_dir);
        return Ok(());
    }

    let config = pipeline::PipelineConfig {
        project_dir: project_dir.clone(),
        repo_path: repo_path.clone(),
        max_retries: cli.max_retries,
        but_path: cli.but_path.clone(),
        no_commit: cli.no_commit,
        no_fixes: cli.no_fixes,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
        use_git_commit: false,
        stage: stage_filter,
    };

    let passed = pipeline::run(branch, &project_files, &config).await?;

    if !passed {
        error!(branch = %ui::cyan(&branch.name), "Post-commit checks failed — manual fixes needed");
    } else {
        info!(branch = %ui::cyan(&branch.name), "Post-commit checks passed");
    }

    Ok(())
}

/// Resolve the project directory, detecting if we're already inside it.
/// If `repo_path` already ends with `project_dir`, use `repo_path` directly
/// instead of appending the subdirectory again.
fn resolve_project_dir(repo_path: &std::path::Path, project_dir: &str) -> Result<std::path::PathBuf> {
    let joined = repo_path.join(project_dir);
    if joined.exists() {
        return Ok(joined);
    }
    // Check if repo_path itself ends with the project_dir component
    if repo_path.ends_with(project_dir) && repo_path.exists() {
        return Ok(repo_path.to_path_buf());
    }
    anyhow::bail!("Project directory not found: {}", joined.display());
}

async fn self_update() -> Result<()> {
    let source_dir = env!("CARGO_MANIFEST_DIR");
    let running_version = env!("CARGO_PKG_VERSION");

    // Read the current Cargo.toml to get the source version
    let manifest_path = std::path::Path::new(source_dir).join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;

    let source_version = manifest
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if line.starts_with("version") {
                line.split('=').nth(1).map(|v| v.trim().trim_matches('"').to_string())
            } else {
                None
            }
        })
        .context("Could not find version in Cargo.toml")?;

    info!("Running: v{}  Source: v{}", running_version, source_version);

    if !is_newer(&source_version, running_version) {
        eprint!("Already up to date. Force reinstall? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }

    info!("Updating v{} -> v{}", running_version, source_version);

    let output = process::run_command(
        "cargo",
        &["install", "--path", source_dir],
        std::path::Path::new(source_dir),
    )
    .await?;

    if output.exit_code != 0 {
        anyhow::bail!("Update failed:\n{}", output.stderr);
    }

    info!("Updated to v{}", source_version);
    Ok(())
}

/// Compare semver strings: returns true if `a` is strictly greater than `b`
fn is_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.').filter_map(|p| p.parse().ok()).collect()
    };
    let va = parse(a);
    let vb = parse(b);
    va > vb
}
