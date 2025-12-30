use crate::application::service::AggregateOptions;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use validator::Validate;

use crate::api::state::AppState;
use utoipa::{IntoParams, ToSchema};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize, IntoParams, ToSchema, Debug, Validate)]
pub struct AggregateQuery {
    /// Enable aggregation mode to combine multiple files
    #[param(example = "true")]
    pub aggregate: Option<String>,

    /// Page number for pagination (1-10000)
    #[param(default = 1, minimum = 1, example = 1)]
    #[validate(range(min = 1, max = 10000))]
    pub page: Option<usize>,

    /// Number of items per page (1-100)
    #[param(default = 30, minimum = 1, maximum = 100, example = 30)]
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<usize>,

    /// Start date filter for aggregation (YYYY-MM-DD format)
    #[param(example = "2025-12-01")]
    pub start: Option<String>,

    /// End date filter for aggregation (YYYY-MM-DD format)
    #[param(example = "2025-12-31")]
    pub end: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub mode: String,
    pub backend: String,
    pub config: String,
    pub dependencies: HealthDependencies,
}

#[derive(Serialize, ToSchema)]
pub struct HealthDependencies {
    pub redis: String,
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses(
        (status = 200, description = "Health check passed", body = HealthResponse),
        (status = 503, description = "Service degraded or unavailable", body = HealthResponse)
    )
)]
pub async fn health_handler(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<HealthResponse>)> {
    // Check Redis connectivity
    let redis_status = match state.content_service.check_cache_health().await {
        Ok(true) => "healthy",
        Ok(false) => "unavailable",
        Err(_) => "error",
    };

    let overall_status = if redis_status == "healthy" {
        "ok"
    } else {
        "degraded"
    };

    let response = HealthResponse {
        status: overall_status.to_string(),
        version: VERSION.to_string(),
        mode: "read-only".to_string(),
        backend: "rust-axum-onion".to_string(),
        config: "yaml".to_string(),
        dependencies: HealthDependencies {
            redis: redis_status.to_string(),
        },
    };

    if overall_status == "ok" {
        Ok(Json(response))
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, Json(response)))
    }
}

#[utoipa::path(
    get,
    path = "/metrics",
    tag = "system",
    responses(
        (status = 200, description = "Prometheus metrics", content_type = "text/plain")
    )
)]
pub async fn metrics_handler() -> impl IntoResponse {
    let handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus recorder");
    handle.render()
}

#[utoipa::path(
    get,
    path = "/v1/api/{source}/{owner}/{repo}/{*path}",
    params(
        ("source" = String, Path, description = "Source platform", example = "github"),
        ("owner" = String, Path, description = "Repository owner/organization", example = "KaspaDev"),
        ("repo" = String, Path, description = "Repository name", example = "Kaspa-Exchange-Data"),
        ("*path" = String, Path, description = "File or directory path in repository (supports nested paths like 'data/exchange/2025/12' - for aggregation, use a directory path with aggregate=true)", example = "README.md"),
        AggregateQuery
    ),
    tag = "content",
    responses(
        (status = 200, description = "Content retrieved successfully", body = serde_json::Value,
            example = json!({
                "name": "2025-12-28-raw.json",
                "type": "file",
                "path": "data/tbdai/ascendex/2025/12/2025-12-28-raw.json"
            })
        ),
        (status = 400, description = "Bad Request - Invalid parameters", 
            example = json!({"error": "Invalid parameters: page must be less than or equal to 10000"})
        ),
        (status = 403, description = "Access Forbidden - Repository not whitelisted",
            example = json!({"error": "Access denied for repository: github/UnknownOrg/PrivateRepo/data"})
        ),
        (status = 404, description = "Not Found - Resource does not exist",
            example = json!({"error": "Resource not found: github/KaspaDev/Kaspa-Exchange-Data/invalid/path"})
        ),
        (status = 500, description = "Internal Server Error")
    )
)]
#[instrument(skip(state), fields(source = %source, owner = %owner, repo = %repo, path = %path, aggregate = ?query.aggregate))]
pub async fn content_handler(
    Path((source, owner, repo, path)): Path<(String, String, String, String)>,
    Query(query): Query<AggregateQuery>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, String)> {
    // Validate query parameters
    if let Err(e) = query.validate() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid parameters: {}", e),
        ));
    }

    // Increment request counter metric
    metrics::counter!("api_requests_total", "endpoint" => "content", "source" => source.clone())
        .increment(1);

    let opts = AggregateOptions {
        aggregate: query.aggregate.as_deref() == Some("true"),
        page: query.page.unwrap_or(1),
        limit: query.limit.unwrap_or(30),
        start: query.start.clone(),
        end: query.end.clone(),
    };

    match state
        .content_service
        .get_content(
            source.clone(),
            owner.clone(),
            repo.clone(),
            path.clone(),
            opts,
        )
        .await
    {
        Ok(data) => {
            // Success
            Ok(Json(data).into_response())
        }
        Err(e) => {
            // Map anyhow error to status code with context
            let msg = e.to_string();
            let request_info = format!("{}/{}/{}/{}", source, owner, repo, path);

            if msg.contains("Access Denied") {
                Err((
                    StatusCode::FORBIDDEN,
                    format!("Access denied for repository: {}", request_info),
                ))
            } else if msg.contains("Not found") || msg.contains("404") {
                Err((
                    StatusCode::NOT_FOUND,
                    format!("Resource not found: {}", request_info),
                ))
            } else if msg.contains("Too many items") {
                Err((StatusCode::BAD_REQUEST, msg))
            } else {
                tracing::error!("Internal error for {}: {}", request_info, msg);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Internal server error processing: {}", request_info),
                ))
            }
        }
    }
}

// Re-export ticker types for use in doc.rs
pub use crate::application::ticker_service::{
    AggregateStats, ExchangeStats, OhlcvPoint, TickerHistoryQuery, TickerHistoryResponse,
    TickerStatsQuery, TickerStatsResponse,
};

/// Get current stats for a token across all exchanges.
///
/// Returns aggregated statistics for the specified token across all
/// supported exchanges, with optional lookback range.
#[utoipa::path(
    get,
    path = "/v1/ticker/{token}",
    params(
        ("token" = String, Path, description = "Token symbol (e.g., kaspa, slow, nacho)", example = "kaspa"),
        TickerStatsQuery
    ),
    tag = "ticker",
    responses(
        (status = 200, description = "Token stats retrieved successfully", body = TickerStatsResponse,
            example = json!({
                "token": "slow",
                "timestamp": "2025-12-29T22:45:00Z",
                "range": "today",
                "exchanges": [
                    {"exchange": "ascendex", "last": 0.000123, "high": 0.00013, "low": 0.000118, "volume_24h": 1234567.89, "change_pct": 2.5, "data_points": 1440}
                ],
                "aggregate": {"avg_price": 0.0001235, "total_volume_24h": 2500000.0, "vwap": 0.0001233, "exchange_count": 2}
            })
        ),
        (status = 404, description = "Token not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[instrument(skip(state), fields(token = %token, range = ?query.range))]
pub async fn ticker_stats_handler(
    Path(token): Path<String>,
    Query(query): Query<TickerStatsQuery>,
    State(state): State<AppState>,
) -> Result<Json<TickerStatsResponse>, (StatusCode, String)> {
    let range = query.range.unwrap_or_else(|| "today".to_string());

    // Validate range
    if !["today", "7d", "30d"].contains(&range.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid range. Use: today, 7d, or 30d".to_string(),
        ));
    }

    metrics::counter!("api_requests_total", "endpoint" => "ticker_stats", "token" => token.clone())
        .increment(1);

    match state.ticker_service.get_ticker_stats(token.clone(), range).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("No exchanges found") {
                Err((StatusCode::NOT_FOUND, format!("Token not found: {}", token)))
            } else {
                tracing::error!("Ticker stats error for {}: {}", token, msg);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to get stats for token: {}", token),
                ))
            }
        }
    }
}

/// Get historical data for a token (for charting).
///
/// Returns OHLCV data aggregated across exchanges for the specified
/// token, suitable for charting applications.
#[utoipa::path(
    get,
    path = "/v1/ticker/{token}/history",
    params(
        ("token" = String, Path, description = "Token symbol (e.g., kaspa, slow, nacho)", example = "kaspa"),
        TickerHistoryQuery
    ),
    tag = "ticker",
    responses(
        (status = 200, description = "Token history retrieved successfully", body = TickerHistoryResponse,
            example = json!({
                "token": "kaspa",
                "range": "7d",
                "resolution": "1h",
                "data": [
                    {"timestamp": 1735500000, "open": 0.04512, "high": 0.04561, "low": 0.04381, "close": 0.04505, "volume": 60853.37}
                ]
            })
        ),
        (status = 404, description = "Token not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[instrument(skip(state), fields(token = %token, range = ?query.range, resolution = ?query.resolution))]
pub async fn ticker_history_handler(
    Path(token): Path<String>,
    Query(query): Query<TickerHistoryQuery>,
    State(state): State<AppState>,
) -> Result<Json<TickerHistoryResponse>, (StatusCode, String)> {
    let range = query.range.unwrap_or_else(|| "7d".to_string());
    let resolution = query.resolution.unwrap_or_else(|| "1h".to_string());

    // Validate range
    if !["today", "7d", "30d"].contains(&range.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid range. Use: today, 7d, or 30d".to_string(),
        ));
    }

    // Validate resolution
    if !["1m", "5m", "1h", "1d"].contains(&resolution.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid resolution. Use: 1m, 5m, 1h, or 1d".to_string(),
        ));
    }

    metrics::counter!("api_requests_total", "endpoint" => "ticker_history", "token" => token.clone())
        .increment(1);

    match state
        .ticker_service
        .get_ticker_history(token.clone(), range, resolution)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("No exchanges found") {
                Err((StatusCode::NOT_FOUND, format!("Token not found: {}", token)))
            } else {
                tracing::error!("Ticker history error for {}: {}", token, msg);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to get history for token: {}", token),
                ))
            }
        }
    }
}

/// Dashboard HTML content (embedded for simplicity)
const DASHBOARD_HTML: &str = include_str!("../../dashboard/index.html");

/// Serve the development dashboard
pub async fn dashboard_handler() -> impl IntoResponse {
    axum::response::Html(DASHBOARD_HTML)
}
