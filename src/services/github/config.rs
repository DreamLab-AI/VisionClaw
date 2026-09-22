use std::env;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum GitHubConfigError {
    MissingEnvVar(String),
    ValidationError(String),
}

impl fmt::Display for GitHubConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvVar(var) => write!(f, "Missing environment variable: {}", var),
            Self::ValidationError(msg) => write!(f, "Configuration validation error: {}", msg),
        }
    }
}

impl Error for GitHubConfigError {}

/// Environment variable carrying the GitHub token for the corpus repository.
pub const GITHUB_TOKEN_ENV: &str = "PRIVATE_REPO_GITHUB_PAT";
/// Pre-2026-09-02 name, accepted for one release so a stale `.env` still works.
pub const GITHUB_TOKEN_ENV_LEGACY: &str = "LOGSEQ_PRIVATE_REPO_GITHUB";

/// The GitHub token, from `PRIVATE_REPO_GITHUB_PAT` or, failing that, the
/// legacy `LOGSEQ_PRIVATE_REPO_GITHUB`. `None` when neither is set; an empty
/// value counts as set (callers validate emptiness themselves).
pub fn github_token_from_env() -> Option<String> {
    match env::var(GITHUB_TOKEN_ENV) {
        Ok(v) => Some(v),
        Err(_) => match env::var(GITHUB_TOKEN_ENV_LEGACY) {
            Ok(v) => {
                log::warn!(
                    "{} is deprecated; rename it to {} (accepted for one release)",
                    GITHUB_TOKEN_ENV_LEGACY,
                    GITHUB_TOKEN_ENV
                );
                Some(v)
            }
            Err(_) => None,
        },
    }
}

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub token: String,
    pub owner: String,
    pub repo: String,
    /// First configured ingest path (kept for single-path Contents API calls).
    pub base_path: String,
    /// All configured ingest source paths. The dual-graph design ingests two
    /// sources from one repo: `mainKnowledgeGraph/pages` (formal ontology,
    /// pages carry `owl:class::`) and `workingGraph/pages` (working knowledge,
    /// no owl:class → force-directed `page` nodes). Set via comma-separated
    /// `GITHUB_BASE_PATHS` (preferred) or `GITHUB_BASE_PATH`.
    pub base_paths: Vec<String>,
    pub branch: String,
    pub rate_limit: bool,
    pub version: String,
}

impl GitHubConfig {
    /// Returns a placeholder config for when GitHub env vars are not set.
    /// Content API routes will fail gracefully at call-time instead of
    /// crashing the entire server at startup.
    pub fn disabled() -> Self {
        Self {
            token: "disabled".to_string(),
            owner: "none".to_string(),
            repo: "none".to_string(),
            base_path: "/".to_string(),
            base_paths: vec!["/".to_string()],
            branch: "main".to_string(),
            rate_limit: false,
            version: "v3".to_string(),
        }
    }

    pub fn from_env() -> Result<Self, GitHubConfigError> {
        let token = github_token_from_env()
            .ok_or_else(|| GitHubConfigError::MissingEnvVar(GITHUB_TOKEN_ENV.to_string()))?
            .trim_start_matches('=')
            .to_string();

        let owner = env::var("GITHUB_OWNER")
            .map_err(|_| GitHubConfigError::MissingEnvVar("GITHUB_OWNER".to_string()))?;

        let repo = env::var("GITHUB_REPO")
            .map_err(|_| GitHubConfigError::MissingEnvVar("GITHUB_REPO".to_string()))?;

        // Dual-source ingest: prefer the plural GITHUB_BASE_PATHS, fall back to
        // the legacy singular GITHUB_BASE_PATH. Both accept a comma-separated
        // list so a single env var can name both `mainKnowledgeGraph/pages` and
        // `workingGraph/pages`.
        let base_paths_raw = env::var("GITHUB_BASE_PATHS")
            .or_else(|_| env::var("GITHUB_BASE_PATH"))
            .map_err(|_| GitHubConfigError::MissingEnvVar("GITHUB_BASE_PATH".to_string()))?;

        let base_paths: Vec<String> = base_paths_raw
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();

        let base_path = base_paths
            .first()
            .cloned()
            .unwrap_or_else(|| base_paths_raw.trim().to_string());

        let branch = env::var("GITHUB_BRANCH").unwrap_or_else(|_| "main".to_string());

        let rate_limit = env::var("GITHUB_RATE_LIMIT")
            .map(|v| v.parse::<bool>().unwrap_or(true))
            .unwrap_or(true);

        let version = env::var("GITHUB_API_VERSION").unwrap_or_else(|_| "v3".to_string());

        let config = Self {
            token,
            owner,
            repo,
            base_path,
            base_paths,
            branch,
            rate_limit,
            version,
        };

        config.validate()?;

        Ok(config)
    }

    fn validate(&self) -> Result<(), GitHubConfigError> {
        if self.token.is_empty() {
            return Err(GitHubConfigError::ValidationError(
                "GitHub token cannot be empty".to_string(),
            ));
        }

        if self.owner.is_empty() {
            return Err(GitHubConfigError::ValidationError(
                "GitHub owner cannot be empty".to_string(),
            ));
        }

        if self.repo.is_empty() {
            return Err(GitHubConfigError::ValidationError(
                "GitHub repository cannot be empty".to_string(),
            ));
        }

        if self.base_path.is_empty() || self.base_paths.is_empty() {
            return Err(GitHubConfigError::ValidationError(
                "GitHub base path cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // Mutex to serialize tests that mutate environment variables.
    // env::set_var / env::remove_var are not thread-safe; parallel test
    // runners can race and cause spurious failures.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_missing_required_vars() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("PRIVATE_REPO_GITHUB_PAT");
        env::remove_var("LOGSEQ_PRIVATE_REPO_GITHUB");
        env::remove_var("GITHUB_OWNER");
        env::remove_var("GITHUB_REPO");
        env::remove_var("GITHUB_BASE_PATH");

        match GitHubConfig::from_env() {
            Err(GitHubConfigError::MissingEnvVar(var)) => {
                assert_eq!(var, "PRIVATE_REPO_GITHUB_PAT");
            }
            other => {
                panic!("Expected MissingEnvVar error, got: {:?}", other);
            }
        }
    }

    /// ADR-2115: GitHub ingest is optional and off by default. Boot must not
    /// depend on `PRIVATE_REPO_GITHUB_PAT` / `GITHUB_OWNER` / `GITHUB_REPO`:
    /// `from_env` reports the absence, and the `disabled()` placeholder the
    /// server boots with is itself a valid config.
    #[test]
    fn the_disabled_placeholder_is_valid_without_any_github_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("PRIVATE_REPO_GITHUB_PAT");
        env::remove_var("LOGSEQ_PRIVATE_REPO_GITHUB");
        env::remove_var("GITHUB_OWNER");
        env::remove_var("GITHUB_REPO");
        env::remove_var("GITHUB_BASE_PATH");
        env::remove_var("GITHUB_BASE_PATHS");

        assert!(
            GitHubConfig::from_env().is_err(),
            "an unconfigured environment yields no GitHub config"
        );
        assert!(
            GitHubConfig::disabled().validate().is_ok(),
            "the placeholder the server boots with must validate"
        );
    }

    #[test]
    fn test_empty_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::set_var("PRIVATE_REPO_GITHUB_PAT", "");
        env::set_var("GITHUB_OWNER", "owner");
        env::set_var("GITHUB_REPO", "repo");
        env::set_var("GITHUB_BASE_PATH", "path");

        match GitHubConfig::from_env() {
            Err(GitHubConfigError::ValidationError(msg)) => {
                assert!(msg.contains("token cannot be empty"));
            }
            other => {
                panic!("Expected ValidationError, got: {:?}", other);
            }
        }
    }

    #[test]
    fn test_valid_config() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::set_var("PRIVATE_REPO_GITHUB_PAT", "token");
        env::set_var("GITHUB_OWNER", "owner");
        env::set_var("GITHUB_REPO", "repo");
        env::set_var("GITHUB_BASE_PATH", "path");
        // Reset optional vars to defaults
        env::remove_var("GITHUB_BRANCH");
        env::remove_var("GITHUB_RATE_LIMIT");
        env::remove_var("GITHUB_API_VERSION");

        let config = GitHubConfig::from_env().unwrap();
        assert_eq!(config.token, "token");
        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
        assert_eq!(config.base_path, "path");
        assert_eq!(config.branch, "main");
        assert!(config.rate_limit);
        assert_eq!(config.version, "v3");
    }

    #[test]
    fn test_optional_settings() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::set_var("PRIVATE_REPO_GITHUB_PAT", "token");
        env::set_var("GITHUB_OWNER", "owner");
        env::set_var("GITHUB_REPO", "repo");
        env::set_var("GITHUB_BASE_PATH", "path");
        env::set_var("GITHUB_RATE_LIMIT", "false");
        env::set_var("GITHUB_API_VERSION", "v4");
        env::set_var("GITHUB_BRANCH", "multi-ontology");

        let config = GitHubConfig::from_env().unwrap();
        assert!(!config.rate_limit);
        assert_eq!(config.version, "v4");
        assert_eq!(config.branch, "multi-ontology");
    }
}
