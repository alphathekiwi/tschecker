use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "tschecker")]
#[command(about = "Run TypeScript checks on GitButler branch changes")]
pub struct Cli {
    /// Check a specific branch by name or CLI ID
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Check all applied branches
    #[arg(short, long)]
    pub all: bool,

    /// Path to the monorepo root (default: current directory)
    #[arg(long, default_value = ".")]
    pub repo_path: PathBuf,

    /// Subdirectory containing the TS project (relative to repo root)
    #[arg(long, default_value = "ch-client")]
    pub project_dir: String,

    /// Max Claude fix attempts per check stage
    #[arg(long, default_value_t = 3)]
    pub max_retries: u32,

    /// Path to the but CLI
    #[arg(long, default_value = "but")]
    pub but_path: String,

    /// Skip the commit step (just check and fix, don't commit)
    #[arg(short, long)]
    pub no_commit: bool,

    /// List files for each check stage
    #[arg(short, long)]
    pub verbose: bool,

    /// Show what would be checked without running
    #[arg(long)]
    pub dry_run: bool,

    /// Run as a post-commit hook (detect branch from last commit, run checks)
    #[arg(short, long)]
    pub post_commit: bool,

    /// Rebuild and reinstall tschecker from source
    #[arg(long)]
    pub update: bool,
}
