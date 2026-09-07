use super::config::GitHubConfig;
use crate::config::AppFullSettings;
use crate::errors::VisionClawResult;
use log::{debug, info};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// const GITHUB_API_DELAY: Duration = Duration::from_millis(500);
// const MAX_RETRIES: u32 = 3;
// const RETRY_DELAY: Duration = Duration::from_secs(2);

pub struct GitHubClient {
    client: Client,
    token: String,
    owner: String,
    repo: String,
    base_path: String,
    base_paths: Vec<String>,
    branch: String,
    settings: Arc<RwLock<AppFullSettings>>,
}

impl GitHubClient {
    pub async fn new(
        config: GitHubConfig,
        settings: Arc<RwLock<AppFullSettings>>,
    ) -> VisionClawResult<Self> {
        let debug_enabled = crate::utils::logging::is_debug_enabled();

        if debug_enabled {
            debug!(
                "Initializing GitHub client - Owner: '{}', Repo: '{}', Base path: '{}'",
                config.owner, config.repo, config.base_path
            );
        }

        if debug_enabled {
            debug!("Configuring HTTP client - Timeout: 30s, User-Agent: github-api-client");
        }

        let client = Client::builder()
            .user_agent("github-api-client")
            .timeout(Duration::from_secs(30))
            .build()?;

        if debug_enabled {
            debug!("HTTP client configured successfully");
        }

        let raw_path = urlencoding::decode(&config.base_path)
            .unwrap_or(std::borrow::Cow::Owned(config.base_path.clone()))
            .into_owned();

        if debug_enabled {
            debug!("Decoded base path: '{}'", raw_path);
        }

        let base_path = raw_path
            .trim_matches('/')
            .replace("//", "/")
            .replace('\\', "/");

        // Normalise every configured ingest path the same way as base_path so
        // the Trees API prefix filter can match each source dir.
        let base_paths: Vec<String> = config
            .base_paths
            .iter()
            .map(|p| {
                let decoded = urlencoding::decode(p)
                    .unwrap_or(std::borrow::Cow::Owned(p.clone()))
                    .into_owned();
                decoded
                    .trim_matches('/')
                    .replace("//", "/")
                    .replace('\\', "/")
            })
            .filter(|p| !p.is_empty())
            .collect();

        let base_paths = if base_paths.is_empty() {
            vec![base_path.clone()]
        } else {
            base_paths
        };

        if debug_enabled {
            debug!(
                "Cleaned base path: '{}', all ingest paths: {:?}",
                base_path, base_paths
            );
            debug!("GitHub client initialization complete");
        }

        Ok(Self {
            client,
            token: config.token,
            owner: config.owner,
            repo: config.repo,
            base_path,
            base_paths,
            branch: config.branch,
            settings: Arc::clone(&settings),
        })
    }

    pub async fn get_full_path(&self, path: &str) -> String {
        let settings = self.settings.read().await;
        let debug_enabled = crate::utils::logging::is_debug_enabled();
        drop(settings);

        if debug_enabled {
            debug!(
                "Getting full path - Base: '{}', Input path: '{}'",
                self.base_path, path
            );
        }

        let base = self.base_path.trim_matches('/');
        let path = path.trim_matches('/');

        if debug_enabled {
            log::debug!("Trimmed paths - Base: '{}', Path: '{}'", base, path);
        }

        // NO percent-decoding here. A repository path is a literal filename,
        // not an encoded URL fragment: the corpus contains `Presentation%3A
        // Conclusion.md`, and decoding turned that into `Presentation:
        // Conclusion.md` — a file that does not exist — before the URL was even
        // built. Encoding happens once, at URL construction, in `url_path`.
        let raw_path = path.to_string();
        let raw_base = base.to_string();

        let full_path = if raw_base.is_empty() {
            if debug_enabled {
                log::debug!("Base path is empty, using raw path only: '{}'", raw_path);
            }
            raw_path
        } else {
            if raw_path.is_empty() {
                if debug_enabled {
                    log::debug!("Path is empty, using base path only: '{}'", raw_base);
                }
                raw_base
            } else if (raw_path == raw_base || raw_path.starts_with(&format!("{raw_base}/"))) {
                if debug_enabled {
                    log::debug!(
                        "Path already contains base path, using as-is: '{}'",
                        raw_path
                    );
                }
                raw_path
            } else {
                let combined = format!("{}/{}", raw_base, raw_path);
                if debug_enabled {
                    log::debug!("Combined path: '{}'", combined);
                }
                combined
            }
        };

        // Returns the RAW repository path. Encoding is not this function's job
        // — `url_path::encode_repo_path` does it per segment at the point the
        // path enters a URL, which is also what keeps `/` a literal separator.
        if debug_enabled {
            log::debug!("Final full path (raw, unencoded): '{}'", full_path);
        }

        full_path
    }

    pub async fn get_contents_url(&self, path: &str) -> String {
        let settings = self.settings.read().await;
        let _debug_enabled = crate::utils::logging::is_debug_enabled();
        drop(settings);

        info!("get_contents_url: Building GitHub API URL - Owner: '{}', Repo: '{}', Base path: '{}', Input path: '{}', Branch: '{}'",
            self.owner, self.repo, self.base_path, path, self.branch);

        let full_path = self.get_full_path(path).await;

        info!("get_contents_url: Raw repository path: '{}'", full_path);

        let url =
            super::url_path::contents_url(&self.owner, &self.repo, &full_path, Some(&self.branch));

        info!("get_contents_url: Final GitHub API URL: '{}'", url);

        url
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn owner(&self) -> &str {
        &self.owner
    }

    pub(crate) fn repo(&self) -> &str {
        &self.repo
    }

    pub(crate) fn base_path(&self) -> &str {
        &self.base_path
    }

    /// All configured ingest source paths (dual-graph: ontology + working KG).
    pub(crate) fn base_paths(&self) -> &[String] {
        &self.base_paths
    }

    pub(crate) fn branch(&self) -> &str {
        &self.branch
    }
}

#[cfg(test)]
mod closeout_path_tests {
    use super::*;
    #[tokio::test]
    async fn base_prefix_requires_a_directory_boundary() {
        let mut config = GitHubConfig::disabled();
        config.base_path = "knowledge/pages".into();
        let client = GitHubClient::new(config, Arc::new(RwLock::new(AppFullSettings::default())))
            .await
            .unwrap();
        for (input, expected) in [
            ("knowledge/pages/A.md", "knowledge/pages/A.md"),
            (
                "knowledge/pages-extra/A.md",
                "knowledge/pages/knowledge/pages-extra/A.md",
            ),
            (
                "Presentation%3A Conclusion.md",
                "knowledge/pages/Presentation%3A Conclusion.md",
            ),
            ("/knowledge/pages/", "knowledge/pages"),
            ("", "knowledge/pages"),
        ] {
            assert_eq!(client.get_full_path(input).await, expected);
        }
    }
}
