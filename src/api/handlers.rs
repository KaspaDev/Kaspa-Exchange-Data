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
    path = "/v1/api/{source}/{owner}/{repo}/{path}",
    params(
        ("source" = String, Path, description = "Source platform", example = "github"),
        ("owner" = String, Path, description = "Repository owner/organization", example = "KaspaDev"),
        ("repo" = String, Path, description = "Repository name", example = "Kaspa-Exchange-Data"),
        ("path" = String, Path, description = "File or directory path in repository", example = "data/tbdai/ascendex/2025/12/2025-12-28-raw.json"),
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
