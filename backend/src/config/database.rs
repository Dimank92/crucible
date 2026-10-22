//! CONFIG APPROACH: Option A — layered config crate
//! Rationale: Using the `config` crate provides a robust, layered approach where environment-specific
//! defaults are cleanly defined in TOML files, while sensitive secrets and infrastructure-specific
//! overrides are passed securely via environment variables.

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use std::fmt;
use std::time::Duration;

/// Default connection acquire timeout in seconds (3 seconds) to prevent cascading stalls.
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 3;
/// Maximum connection acquire timeout allowed for HTTP request handling (5 seconds).
pub const MAX_ACQUIRE_TIMEOUT_SECS: u64 = 5;

/// Database configuration for PostgreSQL via SQLx.
#[derive(Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    /// The connection URL. Never logged or serialized.
    #[serde(skip_serializing)]
    pub url: String,
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
    /// Minimum number of idle connections to maintain.
    pub min_connections: u32,
    /// Maximum time in seconds to wait for a connection.
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    /// Maximum time in seconds a connection can remain idle before being closed.
    pub idle_timeout_secs: u64,
    /// Duration in seconds for Retry-After header when pool is exhausted.
    #[serde(default = "default_pool_retry_after_secs")]
    pub pool_retry_after_secs: u64,
    /// Dedicated maximum connections for background batch workers.
    #[serde(default = "default_worker_max_connections")]
    pub worker_max_connections: u32,
}

fn default_connect_timeout_secs() -> u64 {
    DEFAULT_CONNECT_TIMEOUT_SECS
}

fn default_pool_retry_after_secs() -> u64 {
    5
}

fn default_worker_max_connections() -> u32 {
    10
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("url", &"[REDACTED]")
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("idle_timeout_secs", &self.idle_timeout_secs)
            .field("pool_retry_after_secs", &self.pool_retry_after_secs)
            .field("worker_max_connections", &self.worker_max_connections)
            .finish()
    }
}

impl DatabaseConfig {
    /// Returns the connection acquisition timeout, capped between 1 and 5 seconds to prevent request stalls.
    pub fn acquire_timeout(&self) -> Duration {
        let bounded_secs = self.connect_timeout_secs.clamp(1, MAX_ACQUIRE_TIMEOUT_SECS);
        Duration::from_secs(bounded_secs)
    }

    /// Converts the configuration into a ready-to-use SQLx Postgres pool builder.
    pub fn to_sqlx_pool_options(&self) -> PgPoolOptions {
        PgPoolOptions::new()
            .max_connections(self.max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(self.acquire_timeout())
            .idle_timeout(Duration::from_secs(self.idle_timeout_secs))
    }

    /// Converts the configuration into a dedicated SQLx Postgres pool builder for background batch workers.
    pub fn to_worker_sqlx_pool_options(&self) -> PgPoolOptions {
        PgPoolOptions::new()
            .max_connections(self.worker_max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(self.acquire_timeout())
            .idle_timeout(Duration::from_secs(self.idle_timeout_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_connect_timeout() {
        assert_eq!(default_connect_timeout_secs(), 3);
        assert!(default_connect_timeout_secs() >= 3 && default_connect_timeout_secs() <= 5);
    }

    #[test]
    fn test_acquire_timeout_clamping() {
        let mut config = DatabaseConfig {
            url: "postgres://localhost/test".to_string(),
            max_connections: 10,
            min_connections: 2,
            connect_timeout_secs: 30, // 30s from old config that causes stalls
            idle_timeout_secs: 300,
            pool_retry_after_secs: 5,
            worker_max_connections: 5,
        };
        // Should be clamped to MAX_ACQUIRE_TIMEOUT_SECS (5s)
        assert_eq!(config.acquire_timeout(), Duration::from_secs(5));

        // When set to 3s, should remain 3s
        config.connect_timeout_secs = 3;
        assert_eq!(config.acquire_timeout(), Duration::from_secs(3));

        // When set to 0s, should clamp to minimum 1s
        config.connect_timeout_secs = 0;
        assert_eq!(config.acquire_timeout(), Duration::from_secs(1));
    }

    #[test]
    fn test_serde_default_connect_timeout() {
        let json = r#"{
            "url": "postgres://localhost/test",
            "max_connections": 10,
            "min_connections": 2,
            "idle_timeout_secs": 300
        }"#;
        let config: DatabaseConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.connect_timeout_secs, 3);
        assert_eq!(config.acquire_timeout(), Duration::from_secs(3));
        assert_eq!(config.pool_retry_after_secs, 5);
        assert_eq!(config.worker_max_connections, 10);
    }
}

