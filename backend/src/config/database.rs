//! CONFIG APPROACH: Option A — layered config crate
//! Rationale: Using the `config` crate provides a robust, layered approach where environment-specific
//! defaults are cleanly defined in TOML files, while sensitive secrets and infrastructure-specific
//! overrides are passed securely via environment variables.

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use std::fmt;
use std::time::Duration;

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
    /// Converts the configuration into a ready-to-use SQLx Postgres pool builder.
    pub fn to_sqlx_pool_options(&self) -> PgPoolOptions {
        let acquire_timeout = Duration::from_secs(self.connect_timeout_secs.clamp(3, 5));
        let keepalive_idle = 60u64;
        PgPoolOptions::new()
            .max_connections(self.max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(acquire_timeout)
            .idle_timeout(Duration::from_secs(self.idle_timeout_secs))
            .after_connect(move |conn, _meta| {
                Box::pin(async move {
                    let stmt = format!(
                        "SET tcp_keepalives_idle = {}; SET tcp_keepalives_interval = 10; SET tcp_keepalives_count = 5",
                        keepalive_idle
                    );
                    sqlx::query(&stmt).execute(conn).await?;
                    Ok(())
                })
            })
    }

    /// Converts the configuration into a dedicated SQLx Postgres pool builder for background batch workers.
    pub fn to_worker_sqlx_pool_options(&self) -> PgPoolOptions {
        let acquire_timeout = Duration::from_secs(self.connect_timeout_secs.clamp(3, 5));
        let keepalive_idle = 60u64;
        PgPoolOptions::new()
            .max_connections(self.worker_max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(acquire_timeout)
            .idle_timeout(Duration::from_secs(self.idle_timeout_secs))
            .after_connect(move |conn, _meta| {
                Box::pin(async move {
                    let stmt = format!(
                        "SET tcp_keepalives_idle = {}; SET tcp_keepalives_interval = 10; SET tcp_keepalives_count = 5",
                        keepalive_idle
                    );
                    sqlx::query(&stmt).execute(conn).await?;
                    Ok(())
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_sqlx_pool_options_clamps_timeout() {
        let mut config = DatabaseConfig {
            url: "postgres://localhost/test".to_string(),
            max_connections: 10,
            worker_max_connections: 5,
            min_connections: 1,
            connect_timeout_secs: 30,
            idle_timeout_secs: 60,
            pool_retry_after_secs: 5,
        };

        // Even though connect_timeout_secs is 30, options should be created without panicking
        let _opts = config.to_sqlx_pool_options();
        let _worker_opts = config.to_worker_sqlx_pool_options();

        config.connect_timeout_secs = 1;
        let _opts_low = config.to_sqlx_pool_options();

        config.connect_timeout_secs = 4;
        let _opts_mid = config.to_sqlx_pool_options();
    }
}
