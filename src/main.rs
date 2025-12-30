//! Kaspa Exchange Data API Gateway
//!
//! A production-ready REST API gateway for accessing Kaspa exchange data from GitHub repositories,
//! with Redis caching, rate limiting, and comprehensive observability.
//!
//! # Architecture
//!
//! The API follows clean/onion architecture with clear separation of concerns:
//! - **Domain**: Core business entities and repository traits
//! - **Application**: Business logic and use cases
//! - **Infrastructure**: External integrations (GitHub, Redis)
//! - **API**: HTTP handlers, routing, and middleware
//!
//! # Features
//!
//! - ✅ GitHub API integration with rate limit handling and exponential backoff
//! - ✅ Redis caching with connection pooling for performance
//! - ✅ Prometheus metrics for observability
//! - ✅ Request correlation IDs for distributed tracing
//! - ✅ Input validation and proper error handling
//! - ✅ Graceful shutdown for zero-downtime deployments
//! - ✅ API versioning (`/v1/` prefix)
//!
//! # Configuration
//!
//! The API is configured via `config.yaml` and environment variables:
//! - `GITHUB_TOKEN`: GitHub personal access token (required)
//! - `REDIS_URL`: Redis con nection string (default: redis://localhost:6379)
//! - `RUST_LOG`: Logging level (default: info)
//!
//! # Quick Start
//!
//! ```bash
//! # Set environment variables
//! export GITHUB_TOKEN="your_token_here"
//! export REDIS_URL="redis://localhost:6379"
//!
//! # Run the server
//! cargo run --release
//!
//! # Test endpoints
//! curl http://localhost:3010/health
//! curl http://localhost:3010/metrics
//! curl "http://localhost:3010/v1/api/github/owner/repo/path"
//! ```

mod api;
mod application;
mod domain;
mod infrastructure;

use crate::api::routes::create_router;
use crate::api::state::AppState;
use crate::application::{ContentService, TickerService};
use crate::domain::RepoConfig;
use crate::infrastructure::{GitHubRepository, RedisRepository};
use anyhow::Context;
use serde::Deserialize;
use std::env;
use std::fs;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Top-level application configuration loaded from `config.yaml`.
///
/// Contains server settings and repository whitelist configuration.
#[derive(Deserialize, Debug, Clone)]
struct Config {
    /// Server configuration (host, port, CORS origins)
    server: ServerConfig,
    /// List of allowed repositories that can be accessed through the API
    allowed_repos: Vec<RepoConfig>,
}

/// Server configuration settings.
///
/// Defines how the HTTP server should bind and what CORS origins to allow.
#[derive(Deserialize, Debug, Clone)]
struct ServerConfig {
    /// Host address to bind to (default: "0.0.0.0")
    #[serde(default = "default_host")]
    host: String,
    /// Port number to listen on (default: 3010)
    #[serde(default = "default_port")]
    port: u16,
    /// Comma-separated list of allowed CORS origins (default: "*")
    #[serde(default = "default_allowed_origins")]
    allowed_origins: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    3010
}
fn default_allowed_origins() -> String {
    "*".to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    if env::var("GITHUB_TOKEN").is_err() {
        tracing::warn!("GITHUB_TOKEN not found in env, ensure .env is loaded or vars are set");
    }

    tracing_subscriber::registry()
        .with(EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load Config
    let config_content = fs::read_to_string("config.yaml")
        .context("Failed to read config.yaml - ensure file exists in working directory")?;
    let config: Config = serde_yaml::from_str(&config_content)
        .context("Failed to parse config.yaml - check YAML syntax and structure")?;

    let github_token =
        env::var("GITHUB_TOKEN").context("GITHUB_TOKEN environment variable must be set")?;
    let redis_url = env::var("REDIS_URL").ok();

    // Infrastructure
    let github_repo = Arc::new(GitHubRepository::new(github_token));
    let redis_repo = Arc::new(RedisRepository::new(redis_url));

    // Get default repo for ticker service (first allowed repo)
    let default_repo = config
        .allowed_repos
        .first()
        .cloned()
        .expect("At least one allowed repo must be configured");

    // Application
    let content_service = Arc::new(ContentService::new(
        github_repo.clone(),
        redis_repo.clone(),
        config.allowed_repos.clone(),
    ));

    let ticker_service = Arc::new(TickerService::new(
        github_repo,
        redis_repo,
        default_repo,
    ));

    let state = AppState {
        content_service,
        ticker_service,
    };

    let app = create_router(state, config.server.allowed_origins.clone());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to address {}", addr))?;
    tracing::info!("GitRows Rust API server running at http://{}", addr);
    tracing::info!("Allowed repos: {:?}", config.allowed_repos);

    // Graceful shutdown handling
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error during operation")?;

    Ok(())
}

/// Wait for SIGTERM or SIGINT (Ctrl+C) to initiate graceful shutdown
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, initiating graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown");
        },
    }
}
