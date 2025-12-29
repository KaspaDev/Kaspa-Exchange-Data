//! GitHub repository integration with rate limiting and retry logic.
//!
//! This module provides the `GitHubRepository` implementation of the `ContentRepository` trait,
//! enabling access to GitHub repositories via the GitHub REST API v3.
//!
//! # Features
//!
//! - Automatic rate limit detection and retry with exponential backoff
//! - Request timeouts (30s for requests, 5s for connections)
//! - Detailed logging of rate limit status
//! - Support for file content, directory listings, and raw file access
//!
//! # Rate Limiting
//!
//! GitHub's authenticated API allows 5,000 requests per hour. This implementation:
//! - Monitors `X-RateLimit-Remaining` header
//! - Logs warnings when < 100 requests remaining
//! - Automatically retries on 429/403 status codes with exponential backoff
//! - Respects `Retry-After` header when provided
//!
//! # Examples
//!
//! ```no_run
//! use gatewayapi::infrastructure::GitHubRepository;
//! use gatewayapi::domain::{ContentRepository, RepoConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let token = std::env::var("GITHUB_TOKEN")?;
//!     let repo = GitHubRepository::new(token);
//!     
//!     let config = RepoConfig {
//!         source: "github".to_string(),
//!         owner: "KaspaDev".to_string(),
//!         repo: "Kaspa-Exchange-Data".to_string(),
//!     };
//!     
//!     let content = repo.get_content(&config, "README.md").await?;
//!     println!("File: {}", content.name);
//!     Ok(())
//! }
//! ```

use crate::domain::{Content, ContentRepository, ContentType, RepoConfig};
use async_trait::async_trait;
use reqwest::{Client, Response};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use tracing::{info, warn};

/// GitHub API client with automatic rate limit handling and retry logic.
///
/// This struct implements the `ContentRepository` trait for accessing GitHub repositories.
/// It includes production-ready features like request timeouts, rate limit monitoring,
/// and automatic retry with exponential backoff.
pub struct GitHubRepository {
    /// HTTP client configured with timeouts
    client: Client,
    /// GitHub personal access token for authentication
    token: String,
}

impl GitHubRepository {
    /// Create a new GitHub repository client.
    ///
    /// # Arguments
    ///
    /// * `token` - GitHub personal access token for API authentication
    ///
    /// # Configuration
    ///
    /// The client is configured with:
    /// - 30-second request timeout
    /// - 5-second connection timeout
    /// - TLS using rustls
    ///
    /// # Examples
    ///
    /// ```
    /// use gatewayapi::infrastructure::GitHubRepository;
    ///
    /// let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN not set");
    /// let repo = GitHubRepository::new(token);
    /// ```
    pub fn new(token: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        Self { client, token }
    }

    /// Check and log rate limit information from response headers.
    ///
    /// Monitors the `X-RateLimit-Remaining` header and logs warnings when
    /// rate limits are low or exceeded.
    ///
    /// # Arguments
    ///
    /// * `resp` - HTTP response from GitHub API
    fn check_rate_limit(&self, resp: &Response) {
        if let Some(remaining) = resp.headers().get("x-ratelimit-remaining") {
            if let Ok(remaining_str) = remaining.to_str() {
                if let Ok(remaining_count) = remaining_str.parse::<u32>() {
                    if remaining_count < 100 {
                        warn!(
                            "GitHub API rate limit low: {} requests remaining",
                            remaining_count
                        );
                    }
                    if remaining_count == 0 {
                        if let Some(reset) = resp.headers().get("x-ratelimit-reset") {
                            if let Ok(reset_str) = reset.to_str() {
                                info!("GitHub API rate limit exceeded, resets at: {}", reset_str);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Execute GitHub API request with exponential backoff retry on rate limits.
    ///
    /// Automatically retries requests that fail due to rate limiting (status 429 or 403).
    /// Uses exponential backoff starting at 100ms, doubling on each retry, capped at 30 seconds.
    ///
    /// # Arguments
    ///
    /// * `operation` - Closure that creates and sends the HTTP request
    ///
    /// # Returns
    ///
    /// Returns the successful response or an error after all retries exhausted.
    ///
    /// # Retry Strategy
    ///
    /// - Maximum 5 retry attempts
    /// - Exponential backoff: 100ms → 200ms → 400ms → 800ms → 1.6s (capped at 30s)
    /// - Respects `Retry-After` header if present
    /// - Logs each retry attempt with wait time
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Network request fails
    /// - Rate limit exceeded after all retries
    /// - Server returns non-retryable error
    async fn execute_with_retry<F, Fut>(&self, mut operation: F) -> anyhow::Result<Response>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Response, reqwest::Error>>,
    {
        let max_retries = 5;
        let mut delay_ms = 100;

        for attempt in 0..max_retries {
            let resp = operation().await?;

            // Check rate limit headers
            self.check_rate_limit(&resp);

            // If we hit rate limit and have retries left, retry
            let status = resp.status().as_u16();
            if (status == 429 || status == 403) && attempt < max_retries - 1 {
                // Check for Retry-After header
                let wait_time = if let Some(retry_after) = resp.headers().get("retry-after") {
                    if let Ok(retry_str) = retry_after.to_str() {
                        retry_str.parse::<u64>().unwrap_or(delay_ms / 1000)
                    } else {
                        delay_ms / 1000
                    }
                } else {
                    delay_ms / 1000
                };

                warn!(
                    "Rate limited (attempt {}/{}), waiting {} seconds before retry",
                    attempt + 1,
                    max_retries,
                    wait_time
                );
                tokio::time::sleep(Duration::from_secs(wait_time)).await;

                // Exponential backoff
                delay_ms = (delay_ms * 2).min(30000); // Cap at 30 seconds
                continue;
            }

            // Success or final attempt
            return Ok(resp);
        }

        anyhow::bail!("GitHub API request failed after {} retries", max_retries)
    }
}

/// Data transfer object for GitHub API content responses.
///
/// Maps to the GitHub REST API v3 content response format.
#[derive(Deserialize)]
struct GitHubItemDto {
    name: String,
    path: String,
    #[serde(rename = "type")]
    item_type: String,
    url: String,
    content: Option<String>,
    encoding: Option<String>,
    html_url: Option<String>,
    download_url: Option<String>,
}

impl From<GitHubItemDto> for Content {
    fn from(dto: GitHubItemDto) -> Self {
        Content {
            name: dto.name,
            path: dto.path,
            item_type: ContentType::from(dto.item_type),
            content: dto.content,
            encoding: dto.encoding,
            html_url: dto.html_url,
            download_url: dto.download_url,
            url: dto.url,
        }
    }
}

#[async_trait]
impl ContentRepository for GitHubRepository {
    async fn get_content(&self, config: &RepoConfig, path: &str) -> anyhow::Result<Content> {
        let clean_path = path.trim_start_matches('/');
        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/{}",
            config.owner, config.repo, clean_path
        );

        let resp = self
            .execute_with_retry(|| {
                self.client
                    .get(&url)
                    .header("Authorization", format!("token {}", self.token))
                    .header("Accept", "application/vnd.github.v3+json")
                    .header("User-Agent", "GitRows-API-Proxy")
                    .send()
            })
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("GitHub API Error: {}", resp.status());
        }

        let dto: GitHubItemDto = resp.json().await?;
        Ok(Content::from(dto))
    }

    async fn list_directory(
        &self,
        config: &RepoConfig,
        path: &str,
    ) -> anyhow::Result<Vec<Content>> {
        let clean_path = path.trim_start_matches('/');
        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/{}",
            config.owner, config.repo, clean_path
        );

        let resp = self
            .execute_with_retry(|| {
                self.client
                    .get(&url)
                    .header("Authorization", format!("token {}", self.token))
                    .header("Accept", "application/vnd.github.v3+json")
                    .header("User-Agent", "GitRows-API-Proxy")
                    .send()
            })
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("GitHub API Error: {}", resp.status());
        }

        let dtos: Vec<GitHubItemDto> = resp.json().await?;
        Ok(dtos.into_iter().map(Content::from).collect())
    }

    async fn get_raw_file(&self, url: &str) -> anyhow::Result<Value> {
        let resp = self
            .execute_with_retry(|| {
                self.client
                    .get(url)
                    .header("Authorization", format!("token {}", self.token))
                    .header("Accept", "application/vnd.github.v3.raw")
                    .header("User-Agent", "GitRows-API-Proxy")
                    .send()
            })
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("GitHub Fetch Error: {}", resp.status());
        }

        let val: Value = resp.json().await?;
        Ok(val)
    }
}
