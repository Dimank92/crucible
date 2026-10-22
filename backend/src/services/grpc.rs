use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Enforce a strict 10MB maximum decoding message size on inbound gRPC frames.
pub const DEFAULT_MAX_DECODING_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB = 10,485,760 bytes

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

/// Inbound contract deployment chunk streamed over gRPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractDeploymentChunk {
    pub deployment_id: String,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub data: Vec<u8>,
    pub is_last: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractDeploymentResponse {
    pub deployment_id: String,
    pub status: String,
    pub total_bytes_received: usize,
}

/// Internal gRPC Service Handler
#[derive(Clone)]
pub struct InternalGrpcService {
    metrics_cache: Arc<RwLock<HashMap<String, ContractMetricsResponse>>>,
    max_decoding_message_size: usize,
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
            max_decoding_message_size: DEFAULT_MAX_DECODING_MESSAGE_SIZE,
        }
    }

    pub fn with_max_decoding_message_size(mut self, size: usize) -> Self {
        self.max_decoding_message_size = size;
        self
    }

    pub fn max_decoding_message_size(&self) -> usize {
        self.max_decoding_message_size
    }

    /// Validates inbound message size against configured limit (10MB default)
    pub fn validate_message_size(&self, size: usize) -> Result<(), String> {
        if size > self.max_decoding_message_size {
            Err(format!(
                "Inbound message size {} bytes exceeds maximum decoding limit of {} bytes (10MB)",
                size, self.max_decoding_message_size
            ))
        } else {
            Ok(())
        }
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

    /// Process inbound contract deployment stream chunk, enforcing 10MB bounds
    pub async fn handle_deployment_chunk(
        &self,
        chunk: ContractDeploymentChunk,
    ) -> Result<ContractDeploymentResponse, String> {
        self.validate_message_size(chunk.data.len())?;

        Ok(ContractDeploymentResponse {
            deployment_id: chunk.deployment_id,
            status: "ACCEPTED".to_string(),
            total_bytes_received: chunk.data.len(),
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
    max_decoding_message_size: usize,
}

impl InternalGrpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            max_decoding_message_size: DEFAULT_MAX_DECODING_MESSAGE_SIZE,
        }
    }

    pub fn with_max_decoding_message_size(mut self, size: usize) -> Self {
        self.max_decoding_message_size = size;
        self
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn max_decoding_message_size(&self) -> usize {
        self.max_decoding_message_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_max_decoding_message_size_is_10mb() {
        let service = InternalGrpcService::new();
        assert_eq!(service.max_decoding_message_size(), 10 * 1024 * 1024);

        let client = InternalGrpcClient::new("http://127.0.0.1:50051");
        assert_eq!(client.max_decoding_message_size(), 10 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_valid_inbound_deployment_chunk_under_10mb() {
        let service = InternalGrpcService::new();
        let chunk = ContractDeploymentChunk {
            deployment_id: "deploy_01".to_string(),
            chunk_index: 0,
            total_chunks: 1,
            data: vec![0u8; 1024 * 1024], // 1MB payload
            is_last: true,
        };

        let res = service.handle_deployment_chunk(chunk).await;
        assert!(res.is_ok());
        let res = res.unwrap();
        assert_eq!(res.status, "ACCEPTED");
        assert_eq!(res.total_bytes_received, 1024 * 1024);
    }

    #[tokio::test]
    async fn test_oversized_inbound_deployment_chunk_rejected() {
        let service = InternalGrpcService::new();
        // 10MB + 1 byte payload
        let oversized = ContractDeploymentChunk {
            deployment_id: "malicious_chunk".to_string(),
            chunk_index: 0,
            total_chunks: 1,
            data: vec![0u8; (10 * 1024 * 1024) + 1],
            is_last: false,
        };

        let err = service.handle_deployment_chunk(oversized).await;
        assert!(err.is_err());
        let err_msg = err.unwrap_err();
        assert!(err_msg.contains("exceeds maximum decoding limit"));
    }

    #[tokio::test]
    async fn test_custom_message_size_limit() {
        let service = InternalGrpcService::new().with_max_decoding_message_size(5 * 1024 * 1024);
        assert_eq!(service.max_decoding_message_size(), 5 * 1024 * 1024);

        assert!(service.validate_message_size(4 * 1024 * 1024).is_ok());
        assert!(service.validate_message_size(6 * 1024 * 1024).is_err());
    }
}
