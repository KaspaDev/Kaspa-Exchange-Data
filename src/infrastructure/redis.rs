use crate::domain::CacheRepository;
use async_trait::async_trait;
use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::{Config, Pool, Runtime};
use tracing::{error, info};

pub struct RedisRepository {
    pool: Option<Pool>,
}

impl RedisRepository {
    pub fn new(url: Option<String>) -> Self {
        if let Some(redis_url) = url {
            match Config::from_url(&redis_url).create_pool(Some(Runtime::Tokio1)) {
                Ok(pool) => {
                    info!("Redis connection pool initialized");
                    Self { pool: Some(pool) }
                }
                Err(e) => {
                    error!("Failed to create Redis connection pool: {}", e);
                    Self { pool: None }
                }
            }
        } else {
            info!("Redis URL not provided, caching disabled");
            Self { pool: None }
        }
    }
}

#[async_trait]
impl CacheRepository for RedisRepository {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        if let Some(pool) = &self.pool {
            match pool.get().await {
                Ok(mut conn) => {
                    let result: Option<String> = conn.get(key).await.ok();
                    Ok(result)
                }
                Err(e) => {
                    error!("Failed to get Redis connection from pool: {}", e);
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn set(&self, key: &str, value: &str, ttl_seconds: u64) -> anyhow::Result<()> {
        if let Some(pool) = &self.pool {
            match pool.get().await {
                Ok(mut conn) => {
                    let _: () = conn.set_ex(key, value, ttl_seconds).await?;
                }
                Err(e) => {
                    error!("Failed to get Redis connection from pool: {}", e);
                }
            }
        }
        Ok(())
    }
}
