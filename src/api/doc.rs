use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::handlers::health_handler,
        crate::api::handlers::metrics_handler,
        crate::api::handlers::content_handler,
        crate::api::handlers::ticker_stats_handler,
        crate::api::handlers::ticker_history_handler
    ),
    components(
        schemas(
            crate::api::handlers::AggregateQuery,
            crate::api::handlers::HealthResponse,
            crate::api::handlers::HealthDependencies,
            crate::api::handlers::TickerStatsResponse,
            crate::api::handlers::TickerHistoryResponse,
            crate::api::handlers::ExchangeStats,
            crate::api::handlers::AggregateStats,
            crate::api::handlers::OhlcvPoint
        )
    ),
    tags(
        (name = "system", description = "System endpoints for health checks and metrics"),
        (name = "content", description = "Content retrieval endpoints for accessing repository data"),
        (name = "ticker", description = "Simplified ticker data endpoints for aggregated token statistics")
    ),
    info(
        title = "Kaspa Exchange Data API",
        version = "0.1.0",
        description = "Production-ready REST API gateway for accessing Kaspa exchange data from GitHub repositories with Redis caching, rate limiting, and comprehensive observability.",
        contact(
            name = "KaspaDev",
            url = "https://github.com/KaspaDev/Kaspa-Exchange-Data"
        )
    )
)]
pub struct ApiDoc;

