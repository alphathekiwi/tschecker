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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStage {
    Prettier,
    Eslint,
    Typescript,
    Vitest,
}

impl std::str::FromStr for CheckStage {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "prettier" => Ok(Self::Prettier),
            "eslint" => Ok(Self::Eslint),
            "tsc" | "typescript" => Ok(Self::Typescript),
            "vitest" | "test" => Ok(Self::Vitest),
            _ => Err(format!("unknown stage '{}' (valid: prettier, eslint, tsc, vitest)", s)),
        }
    }
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
