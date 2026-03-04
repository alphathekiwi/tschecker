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

    let cli = cli::Cli::parse();

    if cli.update {
        return self_update().await;
    }

    if cli.post_commit {
        return run_post_commit_mode(&cli).await;
    }

    let repo_path = cli.repo_path.canonicalize().context("Invalid repo path")?;
    let project_dir = repo_path.join(&cli.project_dir);

    if !project_dir.exists() {
        anyhow::bail!(
            "Project directory not found: {}",
            project_dir.display()
        );
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
        dry_run: cli.dry_run,
        verbose: cli.verbose,
    };

    let mut all_passed = true;

    for branch in &branches_to_check {
        let all_files = gitbutler::branch_changed_files(&status, &branch.name);
        let project_files = gitbutler::filter_to_project(&all_files, &cli.project_dir);

        if project_files.is_empty() {
            info!(branch = %branch.name, "No files in {} — skipping", cli.project_dir);
            continue;
        }

        let passed = pipeline::run(branch, &project_files, &config).await?;
        if !passed {
            all_passed = false;
            error!(branch = %branch.name, "Pipeline failed");
        }
    }

    if all_passed {
        info!("All branches passed");
    } else {
        std::process::exit(1);
    }

    Ok(())
}

async fn run_post_commit_mode(cli: &cli::Cli) -> Result<()> {
    let repo_path = cli.repo_path.canonicalize().context("Invalid repo path")?;
    let project_dir = repo_path.join(&cli.project_dir);

    if !project_dir.exists() {
        anyhow::bail!("Project directory not found: {}", project_dir.display());
    }

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

    info!(branch = %branch.name, "Detected branch from commit");

    let all_files = gitbutler::branch_changed_files(&status, &branch.name);
    let project_files = gitbutler::filter_to_project(&all_files, &cli.project_dir);

    if project_files.is_empty() {
        info!(branch = %branch.name, "No files in {} — skipping", cli.project_dir);
        return Ok(());
    }

    let config = pipeline::PipelineConfig {
        project_dir: project_dir.clone(),
        repo_path: repo_path.clone(),
        max_retries: cli.max_retries,
        but_path: cli.but_path.clone(),
        no_commit: cli.no_commit,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
    };

    let passed = pipeline::run(branch, &project_files, &config).await?;

    if !passed {
        error!(branch = %branch.name, "Post-commit checks failed — manual fixes needed");
    } else {
        info!(branch = %branch.name, "Post-commit checks passed");
    }

    Ok(())
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
        info!("Already up to date");
        return Ok(());
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
