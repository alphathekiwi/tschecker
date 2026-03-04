# tschecker

A Rust CLI that runs TypeScript code quality checks on [GitButler](https://gitbutler.com/) branch changes. It runs prettier, eslint, tsc, and vitest only on files changed in each virtual branch, auto-fixes what it can, escalates remaining errors to Claude CLI, and commits clean results back via GitButler.

## Installation

### From source

```bash
cargo install --path .
```

### From GitHub releases

Download the binary for your platform from the [releases page](https://github.com/alphathekiwi/tschecker/releases), then:

```bash
chmod +x tschecker-*
mv tschecker-* /usr/local/bin/tschecker
```

### Self-update

If installed from source, rebuild and reinstall from the latest source:

```bash
tschecker --update
```

## Quick Start

Run from your monorepo root (the directory containing your GitButler workspace):

```bash
# Interactive branch selector
tschecker

# Check a specific branch
tschecker -b my-feature-branch

# Check all applied branches
tschecker -a

# Preview what would run
tschecker --dry-run
```

## How It Works

tschecker queries GitButler for changed files per branch, then runs a 4-stage pipeline:

1. **Prettier** — `prettier --write` on changed files (deterministic, no Claude needed)
2. **ESLint** — `eslint --fix` with auto-fix, then Claude CLI for remaining errors
3. **TypeScript** — `tsc --noEmit` on the full project, errors filtered to changed files only, Claude for fixes
4. **Vitest** — `vitest run -u` on related test files (with snapshot updates), Claude for failures

If all stages pass, it commits the results to the correct GitButler branch via `but commit`.

### Claude Fix Loop

When eslint, tsc, or vitest produce errors that can't be auto-fixed, tschecker invokes the Claude CLI:

```
for each attempt (up to --max-retries):
    claude -p "Fix these {stage} errors in {files}: {errors}" --dangerously-skip-permissions
    re-run the check
    if pass: break
```

Each Claude invocation has a 120-second timeout.

### Test File Discovery

tschecker finds test files for changed source files using multiple conventions:

1. **Colocated**: `src/components/Foo.tsx` → `src/components/Foo.test.tsx`
2. **Sibling `Tests/`**: `src/Settings/Components/Foo.tsx` → `src/Settings/Tests/Foo.test.tsx`
3. **Sibling `__tests__/`**: `src/Settings/Components/Foo.tsx` → `src/Settings/__tests__/Foo.test.tsx`
4. **Mirrored `__tests__/`**: `src/containers/Cms/Foo.tsx` → `src/__tests__/containers/Cms/Foo.test.tsx`

Snapshot files (`__snapshots__/*.snap`) are automatically detected and updated via `vitest -u`.

## CLI Reference

| Option | Description | Default |
|--------|-------------|---------|
| `-b, --branch <NAME>` | Check a specific branch by name or CLI ID | Interactive selector |
| `-a, --all` | Check all applied branches | - |
| `--repo-path <PATH>` | Path to the monorepo root | `.` |
| `--project-dir <DIR>` | Subdirectory containing the TS project | `ch-client` |
| `--max-retries <N>` | Claude fix attempts per check stage | `3` |
| `--but-path <PATH>` | Path to the `but` CLI | `but` |
| `-n, --no-commit` | Run checks but skip committing | - |
| `-v, --verbose` | Show file lists and commands for each stage | - |
| `--dry-run` | Show what would run without executing | - |
| `--update` | Rebuild and reinstall from source | - |

## Example Output

### Dry run

```
$ tschecker --dry-run -b my-feature

Branch: my-feature (ar)
Changed files (5):
  src/components/Header.tsx
  src/components/Header.test.tsx
  src/utils/format.ts
  src/pages/Settings.tsx
  src/styles/theme.ts
Prettier targets (5):
  $ ./node_modules/.bin/prettier --write src/components/Header.tsx ...
ESLint targets (5):
  $ ./node_modules/.bin/eslint --fix --cache --cache-location .cache/eslint/ --quiet ...
TypeScript targets (5):
  $ ./node_modules/.bin/tsc --noEmit --pretty false
  (errors filtered to changed files only)
Vitest targets (2):
  $ ./node_modules/.bin/vitest run --reporter=json -u src/components/Header.test.tsx ...
Snapshots to update (1):
  src/components/__snapshots__/Header.test.tsx.snap
```

### Normal run

```
$ tschecker -b my-feature

  INFO Fetching GitButler status...
  INFO Starting pipeline branch="my-feature" files=5
  INFO Running stage="prettier" files=5
  INFO Passed stage="prettier"
  INFO Running stage="eslint" files=5
  INFO Passed stage="eslint"
  INFO Running stage="tsc" files=5
  INFO Passed stage="tsc"
  INFO Running stage="vitest" files=2
  INFO Passed stage="vitest"
  INFO Committed fixes branch="my-feature"
  INFO Pipeline completed successfully branch="my-feature"
  INFO All branches passed
```

## Project Structure

```
src/
├── main.rs           # Entry point, CLI dispatch, self-update
├── cli.rs            # clap derive args
├── gitbutler.rs      # but status JSON parsing, branch/file queries
├── pipeline.rs       # 4-stage pipeline orchestration + Claude fix loop
├── checks/
│   ├── mod.rs        # CheckResult, CheckStage types
│   ├── prettier.rs   # prettier --write
│   ├── eslint.rs     # eslint --fix, error parsing
│   ├── typescript.rs # tsc --noEmit, error filtering
│   └── vitest.rs     # vitest run -u, JSON failure parsing
├── claude.rs         # Claude CLI invocation with timeout
├── files.rs          # Extension filtering, test file mapping, snapshots
├── process.rs        # Async command runner
└── ui.rs             # Interactive branch selector (raw terminal)
```

## Requirements

- [GitButler](https://gitbutler.com/) with the `but` CLI available in PATH
- Node.js project with prettier, eslint, typescript, and vitest in `node_modules`
- [Claude CLI](https://docs.anthropic.com/en/docs/claude-cli) for auto-fix escalation
- Rust toolchain (for building from source)
