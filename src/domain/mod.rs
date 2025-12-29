//! Domain layer - Core business entities and repository traits.
//!
//! This module defines the domain model for the Kaspa Exchange Data API,
//! following clean architecture principles. It contains:
//! - Repository traits that define data access interfaces
//! - Domain entities representing core business concepts
//! - Value objects and types used throughout the application

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Configuration for an allowed repository source.
///
/// Defines which repositories are whitelisted for access through the API.
/// This provides security by restricting data access to pre-approved sources.
///
/// # Examples
///
/// ```
/// use gatewayapi::domain::RepoConfig;
///
/// let config = RepoConfig {
///     source: "github".to_string(),
///     owner: "KaspaDev".to_string(),
///     repo: "Kaspa-Exchange-Data".to_string(),
/// };
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RepoConfig {
    /// The source platform (e.g., "github")
    pub source: String,
    /// The repository owner or organization name
    pub owner: String,
    /// The repository name
    pub repo: String,
}

/// Represents content from a repository (file or directory listing).
///
/// This is the primary domain entity returned by content operations.
/// It can represent either a single file with its contents, or a directory
/// entry in a listing.
///
/// # Fields
///
/// - `name`: The filename or directory name
/// - `path`: The full path within the repository
/// - `item_type`: Whether this is a file, directory, or unknown type
/// - `content`: Base64-encoded file content (for files only)
/// - `encoding`: Content encoding type (typically "base64" for files)
/// - `html_url`: Browser-viewable URL (optional)
/// - `download_url`: Direct download URL (optional)
/// - `url`: API URL for accessing this content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    /// The name of the file or directory
    pub name: String,
    /// The full path within the repository
    pub path: String,
    /// Type of content (file, directory, or unknown)
    pub item_type: ContentType,

    // File-specific fields
    /// Base64-encoded file content (present for files only)
    pub content: Option<String>,
    /// Content encoding type (e.g., "base64")
    pub encoding: Option<String>,

    // URLs
    /// Browser-viewable URL
    pub html_url: Option<String>,
    /// Direct download URL
    pub download_url: Option<String>,
    /// API URL for this content
    pub url: String,
}

/// Type of content item (file, directory, or unknown).
///
/// Used to distinguish between different content types when listing
/// repository contents or processing individual items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    /// A file entry
    File,
    /// A directory entry
    Dir,
    /// Unknown or unsupported type
    Unknown,
}

impl From<String> for ContentType {
    /// Convert a string representation to ContentType.
    ///
    /// # Arguments
    ///
    /// * `s` - String representation ("file", "dir", or anything else for Unknown)
    ///
    /// # Examples
    ///
    /// ```
    /// use gatewayapi::domain::ContentType;
    ///
    /// assert_eq!(ContentType::from("file".to_string()), ContentType::File);
    /// assert_eq!(ContentType::from("dir".to_string()), ContentType::Dir);
    /// assert_eq!(ContentType::from("other".to_string()), ContentType::Unknown);
    /// ```
    fn from(s: String) -> Self {
        match s.as_str() {
            "file" => ContentType::File,
            "dir" => ContentType::Dir,
            _ => ContentType::Unknown,
        }
    }
}

/// Repository trait for content operations.
///
/// Defines the interface for accessing repository content from external sources
/// (e.g., GitHub). Implementations must be thread-safe (`Send + Sync`) for use
/// in async contexts.
///
/// # Implementations
///
/// See `infrastructure::github::GitHubRepository` for the GitHub implementation.
#[async_trait]
pub trait ContentRepository: Send + Sync {
    /// Retrieve a single file's content or a directory listing.
    ///
    /// # Arguments
    ///
    /// * `config` - Repository configuration (owner, repo, source)
    /// * `path` - Path to the file or directory within the repository
    ///
    /// # Returns
    ///
    /// Returns `Ok(Content)` with the file or directory information, or
    /// an error if the content cannot be retrieved.
    ///
    /// # Errors
    ///
    /// - Returns error if the path doesn't exist
    /// - Returns error if API rate limit is exceeded
    /// - Returns error if network communication fails
    async fn get_content(&self, config: &RepoConfig, path: &str) -> anyhow::Result<Content>;

    /// List all items in a directory.
    ///
    /// # Arguments
    ///
    /// * `config` - Repository configuration (owner, repo, source)
    /// * `path` - Path to the directory within the repository
    ///
    /// # Returns
    ///
    /// Returns `Ok(Vec<Content>)` with all items in the directory, or
    /// an error if the directory cannot be listed.
    ///
    /// # Errors
    ///
    /// - Returns error if the path doesn't exist or is not a directory
    /// - Returns error if API rate limit is exceeded
    /// - Returns error if network communication fails
    async fn list_directory(&self, config: &RepoConfig, path: &str)
        -> anyhow::Result<Vec<Content>>;

    /// Fetch raw file content as JSON directly from a URL.
    ///
    /// Used for aggregation operations where we need to fetch multiple files
    /// efficiently. This bypasses the normal content encoding and returns
    /// the parsed JSON directly.
    ///
    /// # Arguments
    ///
    /// * `url` - Direct API URL to the file resource
    ///
    /// # Returns
    ///
    /// Returns the parsed JSON content or an error.
    ///
    /// # Errors
    ///
    /// - Returns error if the URL is invalid or inaccessible
    /// - Returns error if the content is not valid JSON
    /// - Returns error if API rate limit is exceeded
    async fn get_raw_file(&self, url: &str) -> anyhow::Result<serde_json::Value>;
}

/// Repository trait for caching operations.
///
/// Defines the interface for caching layer (e.g., Redis) to improve
/// performance by reducing calls to external APIs.
///
/// # Implementations
///
/// See `infrastructure::redis::RedisRepository` for the Redis implementation.
#[async_trait]
pub trait CacheRepository: Send + Sync {
    /// Retrieve a cached value by key.
    ///
    /// # Arguments
    ///
    /// * `key` - Cache key to look up
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(String))` if the key exists and is not expired,
    /// `Ok(None)` if the key doesn't exist or has expired,
    /// or an error if cache operations fail.
    ///
    /// # Errors
    ///
    /// - Returns error if cache connection fails
    /// - Never errors on cache miss (returns None instead)
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>>;

    /// Store a value in the cache with a TTL.
    ///
    /// # Arguments
    ///
    /// * `key` - Cache key to store under
    /// * `value` - Value to cache (typically JSON-serialized)
    /// * `ttl_seconds` - Time-to-live in seconds before the key expires
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the cache operation fails.
    ///
    /// # Errors
    ///
    /// - Returns error if cache connection fails
    /// - Returns error if the value cannot be stored
    async fn set(&self, key: &str, value: &str, ttl_seconds: u64) -> anyhow::Result<()>;
}
