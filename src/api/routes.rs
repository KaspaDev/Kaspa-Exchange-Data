use crate::api::doc::ApiDoc;
use crate::api::handlers::{content_handler, health_handler, metrics_handler};
use crate::api::state::AppState;
use axum::{routing::get, Router};
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub fn create_router(state: AppState, allowed_origins: String) -> Router {
    // Configure CORS based on configuration
    let cors = if allowed_origins == "*" {
        CorsLayer::permissive()
    } else {
        // Parse comma-separated origins
        let origins: Vec<_> = allowed_origins
            .split(',')
            .map(|s| s.trim().parse().expect("Invalid origin URL"))
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    // Create middleware stack
    let middleware = ServiceBuilder::new()
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(60),
        ))
        .layer(cors);

    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // System endpoints (no versioning)
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        // V1 API endpoints
        .route(
            "/v1/api/{source}/{owner}/{repo}/{*path}",
            get(content_handler),
        )
        // Legacy route for backwards compatibility (can be removed later)
        .route("/api/{source}/{owner}/{repo}/{*path}", get(content_handler))
        .layer(middleware)
        .with_state(state)
}
