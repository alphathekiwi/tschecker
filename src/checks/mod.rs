pub mod eslint;
pub mod prettier;
pub mod typescript;
pub mod vitest;

#[derive(Debug)]
#[allow(dead_code)]
pub struct CheckResult {
    pub stage: CheckStage,
    pub success: bool,
    pub files_checked: Vec<String>,
    pub errors: Vec<String>,
    pub files_modified: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum CheckStage {
    Prettier,
    Eslint,
    Typescript,
    Vitest,
}

impl std::fmt::Display for CheckStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prettier => write!(f, "prettier"),
            Self::Eslint => write!(f, "eslint"),
            Self::Typescript => write!(f, "tsc"),
            Self::Vitest => write!(f, "vitest"),
        }
    }
}
