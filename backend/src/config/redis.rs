//! CONFIG APPROACH: Option A — layered config crate
//! Rationale: Using the `config` crate provides a robust, layered approach where environment-specific
//! defaults are cleanly defined in TOML files, while sensitive secrets and infrastructure-specific
//! overrides are passed securely via environment variables.

use redis::IntoConnectionInfo;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Redis configuration for caching and optional job queues.
#[derive(Clone, Deserialize, Serialize)]
pub struct RedisConfig {
    /// The primary Redis connection URL (used for cache). Never logged or serialized.
    #[serde(skip_serializing)]
    pub url: String,
    /// Optional separate Redis URL for a job queue. Falls back to `url` if not provided.
    #[serde(skip_serializing)]
    pub job_queue_url: Option<String>,
    /// Number of connections in the Redis pool.
    pub pool_size: u32,
    /// Connection timeout in milliseconds.
    pub connection_timeout_ms: u64,
    /// Maximum number of connection retries.
    pub max_retries: u32,
    /// Maximum startup timeout in milliseconds for establishing the initial connection (default: 30000ms / 30s).
    #[serde(default = "default_startup_timeout_ms")]
    pub startup_timeout_ms: u64,
}

fn default_startup_timeout_ms() -> u64 {
    30_000
}

impl fmt::Debug for RedisConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisConfig")
            .field("url", &"[REDACTED]")
            .field(
                "job_queue_url",
                if self.job_queue_url.is_some() {
                    &Some("[REDACTED]")
                } else {
                    &None::<&str>
                },
            )
            .field("pool_size", &self.pool_size)
            .field("connection_timeout_ms", &self.connection_timeout_ms)
            .field("max_retries", &self.max_retries)
            .field("startup_timeout_ms", &self.startup_timeout_ms)
            .finish()
    }
}

impl RedisConfig {
    /// Converts the primary config to Redis connection info.
    pub fn to_connection_info(
        &self,
    ) -> Result<redis::ConnectionInfo, crate::config::error::ConfigError> {
        self.url.as_str().into_connection_info().map_err(|e| {
            crate::config::error::ConfigError::InvalidValue {
                field: "redis.url".to_string(),
                reason: e.to_string(),
            }
        })
    }

    /// Converts the job queue config to Redis connection info.
    pub fn to_job_queue_connection_info(
        &self,
    ) -> Result<redis::ConnectionInfo, crate::config::error::ConfigError> {
        let url = self.job_queue_url.as_ref().unwrap_or(&self.url);
        url.as_str().into_connection_info().map_err(|e| {
            crate::config::error::ConfigError::InvalidValue {
                field: "redis.job_queue_url".to_string(),
                reason: e.to_string(),
            }
        })
    }

    /// Establishes a Redis `ConnectionManager` with exponential backoff retry and overall startup timeout.
    pub async fn get_connection_manager(
        &self,
        client: &redis::Client,
    ) -> Result<redis::aio::ConnectionManager, redis::RedisError> {
        let startup_timeout = std::time::Duration::from_millis(self.startup_timeout_ms);
        let connect_fut = async {
            let mut attempt = 0;
            let mut delay = std::time::Duration::from_millis(100);
            let max_delay = std::time::Duration::from_secs(5);
            let per_attempt_timeout = std::time::Duration::from_millis(self.connection_timeout_ms);

            loop {
                attempt += 1;
                tracing::info!(
                    "Attempting initial Redis connection (attempt {}/{})",
                    attempt, self.max_retries
                );

                let conn_fut = redis::aio::ConnectionManager::new(client.clone());
                match tokio::time::timeout(per_attempt_timeout, conn_fut).await {
                    Ok(Ok(conn)) => {
                        tracing::info!("Successfully established Redis connection manager");
                        return Ok(conn);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            "Failed to connect to Redis on attempt {}: {:?}",
                            attempt, e
                        );
                        if attempt >= self.max_retries {
                            return Err(e);
                        }
                    }
                    Err(_) => {
                        tracing::warn!(
                            "Connection attempt {} to Redis timed out after {}ms",
                            attempt, self.connection_timeout_ms
                        );
                        if attempt >= self.max_retries {
                            return Err(redis::RedisError::from((
                                redis::ErrorKind::IoError,
                                "Connection timed out",
                            )));
                        }
                    }
                }

                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, max_delay);
            }
        };

        match tokio::time::timeout(startup_timeout, connect_fut).await {
            Ok(result) => result,
            Err(_) => {
                tracing::error!(
                    "Redis initial connection timed out after {}ms",
                    self.startup_timeout_ms
                );
                Err(redis::RedisError::from((
                    redis::ErrorKind::IoError,
                    "Redis startup timeout exceeded while establishing initial connection",
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_config_defaults_include_startup_timeout() {
        let json_str = r#"{
            "url": "redis://127.0.0.1:6379",
            "pool_size": 10,
            "connection_timeout_ms": 5000,
            "max_retries": 5
        }"#;

        let config: RedisConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.startup_timeout_ms, 30_000);
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("startup_timeout_ms: 30000"));
        assert!(debug_str.contains(r#"url: "[REDACTED]""#));
    }

    #[test]
    fn redis_config_custom_startup_timeout() {
        let json_str = r#"{
            "url": "redis://127.0.0.1:6379",
            "pool_size": 10,
            "connection_timeout_ms": 2000,
            "max_retries": 10,
            "startup_timeout_ms": 60000
        }"#;

        let config: RedisConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.startup_timeout_ms, 60_000);
    }
}
