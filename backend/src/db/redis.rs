//! Redis connection manager setup with timeout and retry strategy.

use redis::Client;
use crate::config::redis::RedisConfig;

/// Establish a connection manager with exponential backoff and timeout logic.
pub async fn connect_with_retry(
    client: &Client,
    config: &RedisConfig,
) -> Result<redis::aio::ConnectionManager, redis::RedisError> {
    config.connect_manager_with_retry(client).await
}
