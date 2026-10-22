use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Default keepalive ping interval (30 seconds)
pub const DEFAULT_HTTP2_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// Default timeout waiting for keepalive ping ack before considering connection dead (10 seconds)
pub const DEFAULT_HTTP2_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);

/// Default maximum connection lifetime before graceful retirement (20 minutes)
pub const DEFAULT_MAX_CONNECTION_AGE: Duration = Duration::from_secs(1200);

/// Default grace period to finish in-flight RPCs after max connection age reached (30 seconds)
pub const DEFAULT_MAX_CONNECTION_AGE_GRACE: Duration = Duration::from_secs(30);

/// Default TCP socket keepalive (60 seconds)
pub const DEFAULT_TCP_KEEPALIVE: Duration = Duration::from_secs(60);

/// Configuration for Tonic gRPC server keepalive, timeouts, and connection lifecycle management.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GrpcServerKeepaliveConfig {
    pub http2_keepalive_interval: Option<Duration>,
    pub http2_keepalive_timeout: Option<Duration>,
    pub max_connection_age: Option<Duration>,
    pub max_connection_age_grace: Option<Duration>,
    pub tcp_keepalive: Option<Duration>,
}

impl Default for GrpcServerKeepaliveConfig {
    fn default() -> Self {
        Self {
            http2_keepalive_interval: Some(DEFAULT_HTTP2_KEEPALIVE_INTERVAL),
            http2_keepalive_timeout: Some(DEFAULT_HTTP2_KEEPALIVE_TIMEOUT),
            max_connection_age: Some(DEFAULT_MAX_CONNECTION_AGE),
            max_connection_age_grace: Some(DEFAULT_MAX_CONNECTION_AGE_GRACE),
            tcp_keepalive: Some(DEFAULT_TCP_KEEPALIVE),
        }
    }
}

impl GrpcServerKeepaliveConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_http2_keepalive_interval(mut self, interval: Duration) -> Self {
        self.http2_keepalive_interval = Some(interval);
        self
    }

    pub fn with_http2_keepalive_timeout(mut self, timeout: Duration) -> Self {
        self.http2_keepalive_timeout = Some(timeout);
        self
    }

    pub fn with_max_connection_age(mut self, age: Duration) -> Self {
        self.max_connection_age = Some(age);
        self
    }

    pub fn with_max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.max_connection_age_grace = Some(grace);
        self
    }

    pub fn with_tcp_keepalive(mut self, tcp: Duration) -> Self {
        self.tcp_keepalive = Some(tcp);
        self
    }

    /// Configures an existing Tonic `Server` builder with keepalive and connection lifecycle bounds.
    pub fn apply_to_server_builder(&self, mut builder: tonic::transport::Server) -> tonic::transport::Server {
        if let Some(interval) = self.http2_keepalive_interval {
            builder = builder.http2_keepalive_interval(Some(interval));
        }
        if let Some(timeout) = self.http2_keepalive_timeout {
            builder = builder.http2_keepalive_timeout(Some(timeout));
        }
        if let Some(max_age) = self.max_connection_age {
            builder = builder.max_connection_age(max_age);
        }
        if let Some(grace) = self.max_connection_age_grace {
            builder = builder.max_connection_age_grace(grace);
        }
        if let Some(tcp_keepalive) = self.tcp_keepalive {
            builder = builder.tcp_keepalive(Some(tcp_keepalive));
        }
        builder
    }

    /// Instantiates and returns a Tonic `Server` builder with all keepalive parameters applied.
    pub fn configure_server(&self) -> tonic::transport::Server {
        self.apply_to_server_builder(tonic::transport::Server::builder())
    }
}

/// High-performance gRPC Internal Communication Layer structures
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PingRequest {
    pub client_id: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PingResponse {
    pub status: String,
    pub server_time: i64,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractMetricsRequest {
    pub contract_id: String,
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractMetricsResponse {
    pub contract_id: String,
    pub total_calls: i64,
    pub total_gas_used: i64,
    pub avg_execution_time_ms: f64,
    pub error_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractEventNotification {
    pub contract_id: String,
    pub topic: String,
    pub payload: Vec<u8>,
    pub sequence: i64,
    pub timestamp: i64,
}

/// Internal gRPC Service Handler
#[derive(Clone)]
pub struct InternalGrpcService {
    metrics_cache: Arc<RwLock<HashMap<String, ContractMetricsResponse>>>,
    keepalive_config: GrpcServerKeepaliveConfig,
}

impl Default for InternalGrpcService {
    fn default() -> Self {
        Self::new()
    }
}

impl InternalGrpcService {
    pub fn new() -> Self {
        Self {
            metrics_cache: Arc::new(RwLock::new(HashMap::new())),
            keepalive_config: GrpcServerKeepaliveConfig::default(),
        }
    }

    pub fn with_keepalive_config(mut self, config: GrpcServerKeepaliveConfig) -> Self {
        self.keepalive_config = config;
        self
    }

    pub fn keepalive_config(&self) -> &GrpcServerKeepaliveConfig {
        &self.keepalive_config
    }

    /// Create a Tonic Server builder configured with keepalive and max connection age limits
    pub fn create_tonic_server(&self) -> tonic::transport::Server {
        self.keepalive_config.configure_server()
    }

    /// Process high-speed ping diagnostic request
    pub async fn handle_ping(&self, _req: PingRequest) -> Result<PingResponse, String> {
        let now = chrono::Utc::now().timestamp_millis();
        Ok(PingResponse {
            status: "OK".to_string(),
            server_time: now,
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    /// Get telemetry metrics for a target contract over gRPC
    pub async fn get_contract_metrics(
        &self,
        req: ContractMetricsRequest,
    ) -> Result<ContractMetricsResponse, String> {
        let cache = self.metrics_cache.read().await;
        if let Some(metrics) = cache.get(&req.contract_id) {
            return Ok(metrics.clone());
        }

        // Default metrics if not cached
        Ok(ContractMetricsResponse {
            contract_id: req.contract_id,
            total_calls: 0,
            total_gas_used: 0,
            avg_execution_time_ms: 0.0,
            error_count: 0,
        })
    }

    /// Update internal contract metrics state
    pub async fn update_contract_metrics(&self, metrics: ContractMetricsResponse) {
        let mut cache = self.metrics_cache.write().await;
        cache.insert(metrics.contract_id.clone(), metrics);
    }
}

/// gRPC Client Helper for inter-service communication
#[derive(Clone)]
pub struct InternalGrpcClient {
    endpoint: String,
}

impl InternalGrpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_keepalive_configuration() {
        let config = GrpcServerKeepaliveConfig::default();
        assert_eq!(config.http2_keepalive_interval, Some(Duration::from_secs(30)));
        assert_eq!(config.http2_keepalive_timeout, Some(Duration::from_secs(10)));
        assert_eq!(config.max_connection_age, Some(Duration::from_secs(1200)));
        assert_eq!(config.max_connection_age_grace, Some(Duration::from_secs(30)));
        assert_eq!(config.tcp_keepalive, Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_custom_keepalive_builder_methods() {
        let config = GrpcServerKeepaliveConfig::new()
            .with_http2_keepalive_interval(Duration::from_secs(15))
            .with_http2_keepalive_timeout(Duration::from_secs(5))
            .with_max_connection_age(Duration::from_secs(600))
            .with_max_connection_age_grace(Duration::from_secs(15))
            .with_tcp_keepalive(Duration::from_secs(45));

        assert_eq!(config.http2_keepalive_interval, Some(Duration::from_secs(15)));
        assert_eq!(config.http2_keepalive_timeout, Some(Duration::from_secs(5)));
        assert_eq!(config.max_connection_age, Some(Duration::from_secs(600)));
        assert_eq!(config.max_connection_age_grace, Some(Duration::from_secs(15)));
        assert_eq!(config.tcp_keepalive, Some(Duration::from_secs(45)));
    }

    #[test]
    fn test_tonic_server_builder_configuration() {
        let service = InternalGrpcService::new();
        let _server = service.create_tonic_server();

        let custom_config = GrpcServerKeepaliveConfig::new()
            .with_http2_keepalive_interval(Duration::from_secs(20))
            .with_max_connection_age(Duration::from_secs(1800));

        let custom_service = InternalGrpcService::new().with_keepalive_config(custom_config.clone());
        assert_eq!(custom_service.keepalive_config().http2_keepalive_interval, Some(Duration::from_secs(20)));
        assert_eq!(custom_service.keepalive_config().max_connection_age, Some(Duration::from_secs(1800)));

        let _custom_server = custom_service.create_tonic_server();
    }
}
