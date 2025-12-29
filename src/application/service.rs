use crate::domain::{CacheRepository, Content, ContentRepository, ContentType, RepoConfig};
use base64::{engine::general_purpose, Engine as _};
use chrono::NaiveDate;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Clone)]
pub struct ContentService {
    content_repo: Arc<dyn ContentRepository>,
    cache_repo: Arc<dyn CacheRepository>,
    allowed_repos: Vec<RepoConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AggregateOptions {
    pub aggregate: bool,
    pub page: usize,
    pub limit: usize,
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Serialize)]
pub struct AggregatedResult {
    pub total_count: usize,
    pub total_pages: usize,
    pub current_page: usize,
    pub limit: usize,
    pub data: Vec<serde_json::Value>,
}

impl ContentService {
    pub fn new(
        content_repo: Arc<dyn ContentRepository>,
        cache_repo: Arc<dyn CacheRepository>,
        allowed_repos: Vec<RepoConfig>,
    ) -> Self {
        Self {
            content_repo,
            cache_repo,
            allowed_repos,
        }
    }

    fn validate_access(&self, source: &str, owner: &str, repo: &str) -> bool {
        self.allowed_repos
            .iter()
            .any(|r| r.source == source && r.owner == owner && r.repo == repo)
    }

    /// Check cache health for deep health checks
    pub async fn check_cache_health(&self) -> anyhow::Result<bool> {
        // Try a simple get operation as health check
        match self.cache_repo.get("_health_check").await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    pub async fn get_content(
        &self,
        source: String,
        owner: String,
        repo: String,
        path: String,
        options: AggregateOptions,
    ) -> anyhow::Result<serde_json::Value> {
        if !self.validate_access(&source, &owner, &repo) {
            anyhow::bail!("Access Denied: This repository path is not whitelisted.");
        }

        let repo_config = RepoConfig {
            source: source.clone(),
            owner: owner.clone(),
            repo: repo.clone(),
        };
        let cache_key = if options.aggregate {
            format!(
                "v1:gh:{}:{}:{}:{}:agg=true:p{}:l{}",
                source, owner, repo, path, options.page, options.limit
            )
        } else {
            format!("v1:gh:{}:{}:{}:{}", source, owner, repo, path)
        };

        // 1. Try Cache
        if let Ok(Some(cached)) = self.cache_repo.get(&cache_key).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&cached) {
                info!("Cache HIT: {}", cache_key);
                // Track cache hit metric
                metrics::counter!("cache_operations_total", "operation" => "hit").increment(1);
                return Ok(json);
            }
        }

        // Track cache miss metric
        metrics::counter!("cache_operations_total", "operation" => "miss").increment(1);

        // 2. Process - clone repository for static methods
        let c_repo = self.content_repo.clone();

        let result = if options.aggregate {
            Self::process_aggregation(c_repo, repo_config, path, options).await?
        } else {
            Self::process_standard(c_repo, repo_config, path).await?
        };

        // 3. Cache
        let cache_repo = self.cache_repo.clone();
        if let Ok(json_str) = serde_json::to_string(&result) {
            let ttl = 300; // 5 mins
            let _ = cache_repo.set(&cache_key, &json_str, ttl).await;
        }

        Ok(result)
    }

    // Static implementations to decouple from &self lifetime
    async fn process_standard(
        content_repo: Arc<dyn ContentRepository>,
        config: RepoConfig,
        path: String,
    ) -> anyhow::Result<serde_json::Value> {
        match content_repo.list_directory(&config, &path).await {
            Ok(items) => {
                let listing: Vec<serde_json::Value> = items.into_iter().map(|c| {
                     serde_json::json!({
                         "name": c.name,
                         "type": match c.item_type { ContentType::File => "file", ContentType::Dir => "dir", _ => "unknown" },
                         "path": c.path,
                     })
                 }).collect();
                Ok(serde_json::json!(listing))
            }
            Err(_) => {
                let content = content_repo.get_content(&config, &path).await?;
                Self::parse_file_content(content)
            }
        }
    }

    fn parse_file_content(content: Content) -> anyhow::Result<serde_json::Value> {
        if let (Some(raw), Some(enc)) = (content.content, content.encoding) {
            if enc == "base64" {
                let clean = raw.replace('\n', "");
                let bytes = general_purpose::STANDARD.decode(&clean)?;
                let s = String::from_utf8(bytes)?;
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&s) {
                    return Ok(j);
                }
                return Ok(serde_json::Value::String(s));
            }
        }
        Ok(serde_json::json!({
            "name": content.name,
            "type": "file",
            "path": content.path,
            "url": content.url // API url or html_url?
        }))
    }

    async fn process_aggregation(
        content_repo: Arc<dyn ContentRepository>,
        config: RepoConfig,
        path: String,
        opts: AggregateOptions,
    ) -> anyhow::Result<serde_json::Value> {
        // 1. List files
        let mut items = content_repo.list_directory(&config, &path).await?;

        // 2. Filter JSON
        items.retain(|i| i.item_type == ContentType::File && i.name.ends_with(".json"));

        // 3. Date Filter
        let start_date = opts
            .start
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
        let end_date = opts
            .end
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

        if start_date.is_some() || end_date.is_some() {
            items.retain(|f| {
                let date_str = f.name.trim_end_matches(".json");
                if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                    if let Some(s) = start_date {
                        if date < s {
                            return false;
                        }
                    }
                    if let Some(e) = end_date {
                        if date > e {
                            return false;
                        }
                    }
                    true
                } else {
                    false
                }
            });
        }

        let total_count = items.len();

        // CRITICAL: Prevent unbounded processing
        const MAX_TOTAL_ITEMS: usize = 1000;
        if total_count > MAX_TOTAL_ITEMS {
            anyhow::bail!(
                "Too many items to aggregate: {} (max: {}). Please use date filters to reduce the range.",
                total_count,
                MAX_TOTAL_ITEMS
            );
        }

        let limit = opts.limit.clamp(1, 100);
        let total_pages = (total_count as f64 / limit as f64).ceil() as usize;
        let page = opts.page.max(1);

        let start_index = (page - 1) * limit;
        if start_index >= total_count {
            return Ok(serde_json::to_value(AggregatedResult {
                total_count,
                total_pages,
                current_page: page,
                limit,
                data: vec![],
            })?);
        }

        let end_index = (start_index + limit).min(total_count);
        let page_items = &items[start_index..end_index];
        let page_items_owned = page_items.to_vec();

        // 4. Fetch Concurrently (bounded by pagination)
        let fetches = futures::stream::iter(page_items_owned)
            .map(|item| {
                let repo = content_repo.clone();
                let url = item.url.clone();
                async move {
                    if url.is_empty() {
                        return None;
                    }
                    match repo.get_raw_file(&url).await {
                        Ok(v) => Some(v),
                        Err(e) => {
                            error!("Failed fetch {}: {}", item.name, e);
                            None
                        }
                    }
                }
            })
            .buffer_unordered(10)
            .collect::<Vec<_>>()
            .await;

        let results: Vec<_> = fetches.into_iter().flatten().collect();

        Ok(serde_json::to_value(AggregatedResult {
            total_count,
            total_pages,
            current_page: page,
            limit,
            data: results,
        })?)
    }
}
