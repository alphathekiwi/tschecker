use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

use crate::process;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ButStatus {
    #[serde(default)]
    pub stacks: Vec<Stack>,
    #[serde(default)]
    pub unassigned_changes: Vec<FileChange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Stack {
    pub cli_id: String,
    #[serde(default)]
    pub assigned_changes: Vec<FileChange>,
    #[serde(default)]
    pub branches: Vec<Branch>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub cli_id: String,
    pub name: String,
    #[serde(default)]
    pub commits: Vec<Commit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Commit {
    pub cli_id: String,
    #[serde(default)]
    pub commit_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub changes: Option<Vec<FileChange>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct FileChange {
    #[serde(default)]
    pub cli_id: Option<String>,
    pub file_path: String,
    #[serde(default)]
    pub change_type: Option<String>,
}

pub async fn get_status(but_path: &str, repo_path: &Path) -> Result<ButStatus> {
    let output = process::run_command(but_path, &["status", "-f", "--json"], repo_path).await?;

    if output.exit_code != 0 {
        anyhow::bail!("but status failed (exit {}): {}", output.exit_code, output.stderr);
    }

    let status: ButStatus =
        serde_json::from_str(&output.stdout).context("Failed to parse but status JSON output")?;

    Ok(status)
}

/// Get all applied branches from all stacks
pub fn applied_branches(status: &ButStatus) -> Vec<&Branch> {
    status.stacks.iter().flat_map(|s| s.branches.iter()).collect()
}

/// Collect all unique file paths changed in a branch (across all commits + assigned changes)
pub fn branch_changed_files(status: &ButStatus, branch_name: &str) -> Vec<String> {
    let mut files = std::collections::HashSet::new();

    for stack in &status.stacks {
        let branch_in_stack = stack
            .branches
            .iter()
            .any(|b| b.name == branch_name || b.cli_id == branch_name);

        if !branch_in_stack {
            continue;
        }

        for branch in &stack.branches {
            if branch.name != branch_name && branch.cli_id != branch_name {
                continue;
            }
            for commit in &branch.commits {
                if let Some(changes) = &commit.changes {
                    for change in changes {
                        // Skip deleted files
                        if change.change_type.as_deref() != Some("removed") {
                            files.insert(change.file_path.clone());
                        }
                    }
                }
            }
        }

        // Include assigned (uncommitted) changes for this stack
        for change in &stack.assigned_changes {
            if change.change_type.as_deref() != Some("removed") {
                files.insert(change.file_path.clone());
            }
        }
    }

    let mut result: Vec<String> = files.into_iter().collect();
    result.sort();
    result
}

/// Find which branch contains a specific commit hash
pub fn find_branch_by_commit<'a>(status: &'a ButStatus, commit_hash: &str) -> Option<&'a Branch> {
    for stack in &status.stacks {
        for branch in &stack.branches {
            for commit in &branch.commits {
                if commit.commit_id.as_deref() == Some(commit_hash) {
                    return Some(branch);
                }
            }
        }
    }
    None
}

/// Filter file paths to a project subdirectory and make them relative to it
pub fn filter_to_project(files: &[String], project_dir: &str) -> Vec<String> {
    let prefix = format!("{}/", project_dir.trim_end_matches('/'));
    files
        .iter()
        .filter(|f| f.starts_with(&prefix))
        .map(|f| f[prefix.len()..].to_string())
        .collect()
}
