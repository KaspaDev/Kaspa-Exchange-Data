//! Ticker service for simplified token data access.
//!
//! Provides convenience methods for accessing aggregated token statistics
//! across all exchanges without requiring directory navigation.

use crate::domain::{CacheRepository, ContentRepository, ContentType, RepoConfig};
use base64::{engine::general_purpose, Engine as _};
use chrono::{Duration, NaiveDate, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};
use utoipa::ToSchema;

/// Response structure for ticker stats endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TickerStatsResponse {
    /// Token symbol/name
    pub token: String,
    /// Response timestamp (ISO 8601)
    pub timestamp: String,
    /// Range requested (today, 7d, 30d)
    pub range: String,
    /// Per-exchange statistics
    pub exchanges: Vec<ExchangeStats>,
    /// Aggregated statistics across all exchanges
    pub aggregate: AggregateStats,
}

/// Statistics for a single exchange.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExchangeStats {
    /// Exchange identifier
    pub exchange: String,
    /// Last trade price
    pub last: Option<f64>,
    /// 24h high price
    pub high: Option<f64>,
    /// 24h low price
    pub low: Option<f64>,
    /// 24h volume (base currency)
    pub volume_24h: Option<f64>,
    /// 24h price change percentage
    pub change_pct: Option<f64>,
    /// Number of data points in range
    pub data_points: usize,
}

/// Aggregated statistics across all exchanges.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AggregateStats {
    /// Average price across exchanges
    pub avg_price: Option<f64>,
    /// Total volume across all exchanges
    pub total_volume_24h: Option<f64>,
    /// Volume-weighted average price
    pub vwap: Option<f64>,
    /// Number of active exchanges
    pub exchange_count: usize,
}

/// Response structure for ticker history endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TickerHistoryResponse {
    /// Token symbol/name
    pub token: String,
    /// Range requested
    pub range: String,
    /// Data resolution
    pub resolution: String,
    /// OHLCV data points
    pub data: Vec<OhlcvPoint>,
}

/// Single OHLCV data point for charting.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OhlcvPoint {
    /// Unix timestamp (seconds)
    pub timestamp: i64,
    /// Open price
    pub open: f64,
    /// High price
    pub high: f64,
    /// Low price
    pub low: f64,
    /// Close price
    pub close: f64,
    /// Volume
    pub volume: f64,
}

/// Query parameters for ticker stats endpoint.
#[derive(Debug, Clone, Deserialize, utoipa::IntoParams)]
pub struct TickerStatsQuery {
    /// Lookback range: today, 7d, 30d (default: today)
    #[param(default = "today", example = "7d")]
    pub range: Option<String>,
}

/// Query parameters for ticker history endpoint.
#[derive(Debug, Clone, Deserialize, utoipa::IntoParams)]
pub struct TickerHistoryQuery {
    /// Lookback range: today, 7d, 30d (default: 7d)
    #[param(default = "7d", example = "7d")]
    pub range: Option<String>,
    /// Data resolution: 1m, 5m, 1h, 1d (default: 1h)
    #[param(default = "1h", example = "1h")]
    pub resolution: Option<String>,
}

/// Service for ticker-focused operations.
#[derive(Clone)]
pub struct TickerService {
    content_repo: Arc<dyn ContentRepository>,
    cache_repo: Arc<dyn CacheRepository>,
    default_repo: RepoConfig,
}

impl TickerService {
    pub fn new(
        content_repo: Arc<dyn ContentRepository>,
        cache_repo: Arc<dyn CacheRepository>,
        default_repo: RepoConfig,
    ) -> Self {
        Self {
            content_repo,
            cache_repo,
            default_repo,
        }
    }

    /// Get current stats for a token across all exchanges.
    pub async fn get_ticker_stats(
        &self,
        token: String,
        range: String,
    ) -> anyhow::Result<TickerStatsResponse> {
        let cache_key = format!("v1:ticker:{}:stats:{}", token, range);

        // Check cache first
        if let Ok(Some(cached)) = self.cache_repo.get(&cache_key).await {
            if let Ok(response) = serde_json::from_str::<TickerStatsResponse>(&cached) {
                info!("Cache HIT: {}", cache_key);
                metrics::counter!("cache_operations_total", "operation" => "hit").increment(1);
                return Ok(response);
            }
        }
        metrics::counter!("cache_operations_total", "operation" => "miss").increment(1);

        // Discover exchanges for this token
        let token_path = format!("data/{}", token.to_lowercase());
        let exchanges = self
            .content_repo
            .list_directory(&self.default_repo, &token_path)
            .await?;

        let exchange_dirs: Vec<_> = exchanges
            .into_iter()
            .filter(|e| e.item_type == ContentType::Dir)
            .collect();

        if exchange_dirs.is_empty() {
            anyhow::bail!("No exchanges found for token: {}", token);
        }

        // Calculate date range
        let (start_date, end_date) = Self::calculate_date_range(&range);

        // Fetch stats from each exchange concurrently
        let mut exchange_stats = Vec::new();
        let fetches = futures::stream::iter(exchange_dirs)
            .map(|exchange| {
                let repo = self.content_repo.clone();
                let config = self.default_repo.clone();
                let token = token.clone();
                let start = start_date;
                let end = end_date;
                async move {
                    Self::fetch_exchange_stats(repo, config, token, exchange.name, start, end).await
                }
            })
            .buffer_unordered(10)
            .collect::<Vec<_>>()
            .await;

        for result in fetches {
            match result {
                Ok(stats) => exchange_stats.push(stats),
                Err(e) => warn!("Failed to fetch exchange stats: {}", e),
            }
        }

        // Calculate aggregate stats
        let aggregate = Self::calculate_aggregate(&exchange_stats);

        let response = TickerStatsResponse {
            token: token.clone(),
            timestamp: Utc::now().to_rfc3339(),
            range: range.clone(),
            exchanges: exchange_stats,
            aggregate,
        };

        // Cache result (5 min TTL)
        if let Ok(json) = serde_json::to_string(&response) {
            let _ = self.cache_repo.set(&cache_key, &json, 300).await;
        }

        Ok(response)
    }

    /// Get historical data for a token (for charting).
    pub async fn get_ticker_history(
        &self,
        token: String,
        range: String,
        resolution: String,
    ) -> anyhow::Result<TickerHistoryResponse> {
        let cache_key = format!("v1:ticker:{}:history:{}:{}", token, range, resolution);

        // Check cache first
        if let Ok(Some(cached)) = self.cache_repo.get(&cache_key).await {
            if let Ok(response) = serde_json::from_str::<TickerHistoryResponse>(&cached) {
                info!("Cache HIT: {}", cache_key);
                metrics::counter!("cache_operations_total", "operation" => "hit").increment(1);
                return Ok(response);
            }
        }
        metrics::counter!("cache_operations_total", "operation" => "miss").increment(1);

        // Discover exchanges for this token
        let token_path = format!("data/{}", token.to_lowercase());
        let exchanges = self
            .content_repo
            .list_directory(&self.default_repo, &token_path)
            .await?;

        let exchange_dirs: Vec<_> = exchanges
            .into_iter()
            .filter(|e| e.item_type == ContentType::Dir)
            .collect();

        if exchange_dirs.is_empty() {
            anyhow::bail!("No exchanges found for token: {}", token);
        }

        let (start_date, end_date) = Self::calculate_date_range(&range);

        // Collect raw data from exchanges - try up to 10 to find ones with data
        let mut all_data: Vec<serde_json::Value> = Vec::new();
        let mut exchanges_with_data = 0;
        const MAX_EXCHANGES: usize = 5;
        const MAX_TRIES: usize = 15;

        for exchange in exchange_dirs.iter().take(MAX_TRIES) {
            if exchanges_with_data >= MAX_EXCHANGES {
                break;
            }
            
            match Self::fetch_exchange_raw_data(
                self.content_repo.clone(),
                self.default_repo.clone(),
                token.clone(),
                exchange.name.clone(),
                start_date,
                end_date,
            )
            .await
            {
                Ok(data) => {
                    if !data.is_empty() {
                        info!("Found {} data points from {} for history", data.len(), exchange.name);
                        all_data.extend(data);
                        exchanges_with_data += 1;
                    }
                }
                Err(e) => warn!("Failed to fetch data from {}: {}", exchange.name, e),
            }
        }

        // Aggregate into OHLCV based on resolution
        let ohlcv_data = Self::aggregate_to_ohlcv(&all_data, &resolution);

        let response = TickerHistoryResponse {
            token: token.clone(),
            range: range.clone(),
            resolution: resolution.clone(),
            data: ohlcv_data,
        };

        // Cache result (5 min TTL)
        if let Ok(json) = serde_json::to_string(&response) {
            let _ = self.cache_repo.set(&cache_key, &json, 300).await;
        }

        Ok(response)
    }

    fn calculate_date_range(range: &str) -> (NaiveDate, NaiveDate) {
        let today = Utc::now().date_naive();
        let start = match range {
            "today" => today,
            "7d" => today - Duration::days(7),
            "30d" => today - Duration::days(30),
            _ => today,
        };
        (start, today)
    }

    async fn fetch_exchange_stats(
        repo: Arc<dyn ContentRepository>,
        config: RepoConfig,
        token: String,
        exchange: String,
        _start_date: NaiveDate,
        _end_date: NaiveDate,
    ) -> anyhow::Result<ExchangeStats> {
        // Try to get data file - try today first, then fall back to previous days
        let today = Utc::now().date_naive();
        let days_to_try = [today, today - Duration::days(1), today - Duration::days(2)];

        for date in days_to_try {
            let year = date.format("%Y");
            let month = date.format("%m");
            let date_path = format!(
                "data/{}/{}/{}/{}/{}-raw.json",
                token.to_lowercase(),
                exchange,
                year,
                month,
                date.format("%Y-%m-%d")
            );

            // Try to fetch the file
            match repo.get_content(&config, &date_path).await {
                Ok(content) => {
                    // Parse the content
                    if let (Some(raw), Some(enc)) = (content.content, content.encoding) {
                        if enc == "base64" {
                            let clean = raw.replace('\n', "");
                            if let Ok(bytes) = general_purpose::STANDARD.decode(&clean) {
                                if let Ok(s) = String::from_utf8(bytes) {
                                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&s) {
                                        info!("Found data for {} from {} for date {}", token, exchange, date);
                                        return Self::parse_exchange_stats(&exchange, &json);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    // Try next day
                    continue;
                }
            }
        }

        // Return empty stats if no data found in any of the days
        Ok(ExchangeStats {
            exchange,
            last: None,
            high: None,
            low: None,
            volume_24h: None,
            change_pct: None,
            data_points: 0,
        })
    }

    fn parse_exchange_stats(
        exchange: &str,
        json: &serde_json::Value,
    ) -> anyhow::Result<ExchangeStats> {
        let data = json.get("data").and_then(|d| d.as_array());

        if let Some(arr) = data {
            if arr.is_empty() {
                return Ok(ExchangeStats {
                    exchange: exchange.to_string(),
                    last: None,
                    high: None,
                    low: None,
                    volume_24h: None,
                    change_pct: None,
                    data_points: 0,
                });
            }

            // Get latest data point
            let latest = &arr[arr.len() - 1];

            // Calculate high/low across all data points
            let mut high: Option<f64> = None;
            let mut low: Option<f64> = None;
            let mut total_volume: f64 = 0.0;

            for point in arr {
                if let Some(h) = point.get("high").and_then(|v| v.as_f64()) {
                    high = Some(high.map_or(h, |curr| curr.max(h)));
                }
                if let Some(l) = point.get("low").and_then(|v| v.as_f64()) {
                    low = Some(low.map_or(l, |curr| curr.min(l)));
                }
                if let Some(v) = point.get("quoteVolume").and_then(|v| v.as_f64()) {
                    total_volume = v; // Use latest quoteVolume as it's cumulative
                }
            }

            Ok(ExchangeStats {
                exchange: exchange.to_string(),
                last: latest.get("last").and_then(|v| v.as_f64()),
                high,
                low,
                volume_24h: Some(total_volume),
                change_pct: latest.get("percentage").and_then(|v| v.as_f64()),
                data_points: arr.len(),
            })
        } else {
            Ok(ExchangeStats {
                exchange: exchange.to_string(),
                last: None,
                high: None,
                low: None,
                volume_24h: None,
                change_pct: None,
                data_points: 0,
            })
        }
    }

    fn calculate_aggregate(exchanges: &[ExchangeStats]) -> AggregateStats {
        let active_exchanges: Vec<_> = exchanges
            .iter()
            .filter(|e| e.last.is_some())
            .collect();

        if active_exchanges.is_empty() {
            return AggregateStats {
                avg_price: None,
                total_volume_24h: None,
                vwap: None,
                exchange_count: 0,
            };
        }

        let sum_price: f64 = active_exchanges
            .iter()
            .filter_map(|e| e.last)
            .sum();
        let avg_price = sum_price / active_exchanges.len() as f64;

        let total_volume: f64 = active_exchanges
            .iter()
            .filter_map(|e| e.volume_24h)
            .sum();

        // Calculate VWAP (volume-weighted average price)
        let mut weighted_sum = 0.0;
        let mut volume_sum = 0.0;
        for e in &active_exchanges {
            if let (Some(price), Some(vol)) = (e.last, e.volume_24h) {
                weighted_sum += price * vol;
                volume_sum += vol;
            }
        }
        let vwap = if volume_sum > 0.0 {
            Some(weighted_sum / volume_sum)
        } else {
            None
        };

        AggregateStats {
            avg_price: Some(avg_price),
            total_volume_24h: Some(total_volume),
            vwap,
            exchange_count: active_exchanges.len(),
        }
    }

    async fn fetch_exchange_raw_data(
        repo: Arc<dyn ContentRepository>,
        config: RepoConfig,
        token: String,
        exchange: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let mut all_data = Vec::new();
        let mut current = start_date;

        while current <= end_date {
            let year = current.format("%Y");
            let month = current.format("%m");
            let date_path = format!(
                "data/{}/{}/{}/{}/{}-raw.json",
                token.to_lowercase(),
                exchange,
                year,
                month,
                current.format("%Y-%m-%d")
            );

            if let Ok(content) = repo.get_content(&config, &date_path).await {
                if let (Some(raw), Some(enc)) = (content.content, content.encoding) {
                    if enc == "base64" {
                        let clean = raw.replace('\n', "");
                        if let Ok(bytes) = general_purpose::STANDARD.decode(&clean) {
                            if let Ok(s) = String::from_utf8(bytes) {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&s) {
                                    if let Some(data) = json.get("data").and_then(|d| d.as_array())
                                    {
                                        all_data.extend(data.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            current += Duration::days(1);
        }

        Ok(all_data)
    }

    fn aggregate_to_ohlcv(data: &[serde_json::Value], resolution: &str) -> Vec<OhlcvPoint> {
        if data.is_empty() {
            return vec![];
        }

        let interval_secs: i64 = match resolution {
            "1m" => 60,
            "5m" => 300,
            "1h" => 3600,
            "1d" => 86400,
            _ => 3600,
        };

        // Group data points by time bucket
        let mut buckets: std::collections::BTreeMap<i64, Vec<&serde_json::Value>> =
            std::collections::BTreeMap::new();

        for point in data {
            if let Some(ts) = point.get("timestamp").and_then(|v| v.as_i64()) {
                // Convert milliseconds to seconds and bucket
                let ts_secs = ts / 1000;
                let bucket = (ts_secs / interval_secs) * interval_secs;
                buckets.entry(bucket).or_default().push(point);
            }
        }

        // Convert buckets to OHLCV
        buckets
            .into_iter()
            .map(|(timestamp, points)| {
                let mut open = 0.0;
                let mut high = f64::MIN;
                let mut low = f64::MAX;
                let mut close = 0.0;
                let mut volume = 0.0;

                if let Some(first) = points.first() {
                    open = first.get("last").and_then(|v| v.as_f64()).unwrap_or(0.0);
                }
                if let Some(last) = points.last() {
                    close = last.get("last").and_then(|v| v.as_f64()).unwrap_or(0.0);
                }

                for p in &points {
                    if let Some(h) = p.get("high").and_then(|v| v.as_f64()) {
                        high = high.max(h);
                    }
                    if let Some(l) = p.get("low").and_then(|v| v.as_f64()) {
                        low = low.min(l);
                    }
                    if let Some(v) = p.get("quoteVolume").and_then(|v| v.as_f64()) {
                        volume = v; // Use latest as it's cumulative
                    }
                }

                // Fix edge cases
                if high == f64::MIN {
                    high = close;
                }
                if low == f64::MAX {
                    low = close;
                }

                OhlcvPoint {
                    timestamp,
                    open,
                    high,
                    low,
                    close,
                    volume,
                }
            })
            .collect()
    }
}
