use std::path::{Path, PathBuf};

pub const PRETTIER_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx"];
pub const ESLINT_EXTENSIONS: &[&str] = &["ts", "tsx"];
pub const TYPESCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];

/// Filter file paths to those matching given extensions
pub fn filter_by_extensions(files: &[String], extensions: &[&str]) -> Vec<String> {
    files
        .iter()
        .filter(|f| {
            extensions
                .iter()
                .any(|ext| f.ends_with(&format!(".{}", ext)))
        })
        .cloned()
        .collect()
}

/// Check if a file is a runnable test file (not a snapshot)
pub fn is_test_file(path: &str) -> bool {
    if path.ends_with(".snap") {
        return false;
    }
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    name.contains(".test.") || name.contains(".spec.")
}

/// Given a source file, find its test file by checking multiple conventions:
/// 1. Colocated: src/components/Foo.tsx -> src/components/Foo.test.tsx
/// 2. Sibling Tests/ dir: src/Settings/Components/Foo.tsx -> src/Settings/Tests/Foo.test.tsx
/// 3. Sibling __tests__/ dir: src/Settings/Components/Foo.tsx -> src/Settings/__tests__/Foo.test.tsx
/// 4. Mirrored __tests__: src/containers/Cms/Foo.tsx -> src/__tests__/containers/Cms/Foo.test.tsx
pub fn find_test_file(source_path: &str, project_root: &Path) -> Option<PathBuf> {
    let path = Path::new(source_path);
    let stem = path.file_stem()?.to_str()?;
    let ext = path.extension()?.to_str()?;
    let parent = path.parent()?;

    if stem.ends_with(".test") || stem.ends_with(".spec") || ext == "snap" {
        return None;
    }

    let test_filenames = [
        format!("{}.test.{}", stem, ext),
        format!("{}.test.tsx", stem),
        format!("{}.test.ts", stem),
    ];

    // 1. Colocated: same directory
    for name in &test_filenames {
        let candidate = parent.join(name);
        if project_root.join(&candidate).exists() {
            return Some(candidate);
        }
    }

    // 2-3. Sibling directories: go up one level, check Tests/ and __tests__/
    if let Some(grandparent) = parent.parent() {
        for sibling in &["Tests", "__tests__"] {
            for name in &test_filenames {
                let candidate = grandparent.join(sibling).join(name);
                if project_root.join(&candidate).exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // 4. Mirrored __tests__: src/a/b/Foo.tsx -> src/__tests__/a/b/Foo.test.tsx
    if let Ok(stripped) = path.strip_prefix("src") {
        if let Some(rel_parent) = stripped.parent() {
            for name in &test_filenames {
                let candidate = Path::new("src/__tests__").join(rel_parent).join(name);
                if project_root.join(&candidate).exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

/// Find snapshot files that correspond to a set of test files.
/// Looks for __snapshots__/{testfile}.snap next to each test file.
pub fn find_snapshot_files(test_files: &[String], project_root: &Path) -> Vec<String> {
    let mut snapshots = Vec::new();

    for test_file in test_files {
        let path = Path::new(test_file);
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };
        let parent = match path.parent() {
            Some(p) => p,
            None => continue,
        };

        let snap_name = format!("{}.snap", filename);
        let snap_path = parent.join("__snapshots__").join(&snap_name);

        if project_root.join(&snap_path).exists() {
            snapshots.push(snap_path.to_string_lossy().to_string());
        }
    }

    snapshots.sort();
    snapshots
}

/// Collect test files related to a set of changed source files
pub fn collect_test_files(source_files: &[String], project_root: &Path) -> Vec<String> {
    let mut test_files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for source in source_files {
        // If the changed file IS a runnable test file, include it directly
        if is_test_file(source) && seen.insert(source.clone()) {
            test_files.push(source.clone());
            continue;
        }

        // Skip non-source files (snapshots, css, svg, etc.)
        if source.ends_with(".snap") || source.ends_with(".css") || source.ends_with(".svg") {
            continue;
        }

        // Look for a test file via multiple conventions
        if let Some(test_path) = find_test_file(source, project_root) {
            let test_str = test_path.to_string_lossy().to_string();
            if seen.insert(test_str.clone()) {
                test_files.push(test_str);
            }
        }
    }

    test_files.sort();
    test_files
}
