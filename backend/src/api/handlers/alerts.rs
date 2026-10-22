use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::{info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::api::handlers::profiling::AppState;
use crate::error::AppError;

pub const ALERT_DEDUPLICATION_WINDOW_SECS: u64 = 300; // 5-minute suppression window

/// Request body for `POST /api/alerts/ingest`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct AlertIngestRequest {
    pub source: String,
    pub message: String,
    #[serde(default, alias = "alertType")]
    pub alert_type: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

/// Response body for `POST /api/alerts/ingest`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AlertIngestResponse {
    pub alert_id: String,
    pub accepted: bool,
    pub source: String,
    pub message: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert_type: Option<String>,
    #[serde(default)]
    pub suppressed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deduplication_window_secs: Option<u64>,
    pub metadata: Option<Value>,
    pub received_at: DateTime<Utc>,
}

/// Computes a canonical SHA-256 fingerprint from (alert_type, source, message).
pub fn compute_alert_fingerprint(alert_type: &str, source: &str, message: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{alert_type}:{source}:{message}").as_bytes());
    hex::encode(hasher.finalize())
}

/// Checks if an alert is duplicate within the 5-minute suppression window in Redis.
/// Returns Ok(true) if the alert is new and acquired the suppression window lock,
/// or Ok(false) if the alert is a duplicate and should be suppressed.
pub async fn check_and_set_dedup_window(
    redis: &redis::Client,
    fingerprint: &str,
) -> Result<bool, AppError> {
    let mut conn = redis
        .get_multiplexed_async_connection()
        .await
        .map_err(AppError::Redis)?;

    let key = format!("alerts:dedup:{}", fingerprint);

    // SET key 1 EX 300 NX
    let set_result: Option<String> = redis::cmd("SET")
        .arg(&key)
        .arg("1")
        .arg("EX")
        .arg(ALERT_DEDUPLICATION_WINDOW_SECS)
        .arg("NX")
        .query_async(&mut conn)
        .await
        .map_err(AppError::Redis)?;

    Ok(set_result.is_some())
}

/// Ingests one alert event with sliding-window deduplication.
#[utoipa::path(
    post,
    path = "/api/alerts/ingest",
    request_body = AlertIngestRequest,
    params(
        ("Idempotency-Key" = Option<String>, Header, description = "Optional key used to make retries idempotent for 24 hours")
    ),
    responses(
        (status = 200, description = "Alert ingested", body = AlertIngestResponse),
        (status = 400, description = "Invalid payload")
    ),
    tag = "alerts"
)]
pub async fn ingest_alert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AlertIngestRequest>,
) -> Result<Json<AlertIngestResponse>, AppError> {
    if payload.source.trim().is_empty() {
        return Err(AppError::BadRequest("source is required".to_string()));
    }
    if payload.message.trim().is_empty() {
        return Err(AppError::BadRequest("message is required".to_string()));
    }

    let alert_type = payload
        .alert_type
        .clone()
        .unwrap_or_else(|| "alert".to_string());
    let fingerprint = compute_alert_fingerprint(&alert_type, &payload.source, &payload.message);

    // Check Redis 5-minute suppression window
    let mut is_suppressed = false;
    match check_and_set_dedup_window(&state.redis, &fingerprint).await {
        Ok(is_new) => {
            if !is_new {
                info!(
                    fingerprint = %fingerprint,
                    source = %payload.source,
                    "Alert suppressed within 5-minute deduplication window"
                );
                is_suppressed = true;
            }
        }
        Err(err) => {
            warn!(
                error = %err,
                "Redis alert deduplication check failed; proceeding with ingestion"
            );
        }
    }

    let response = AlertIngestResponse {
        alert_id: Uuid::new_v4().to_string(),
        accepted: !is_suppressed,
        source: payload.source,
        message: payload.message,
        severity: payload
            .severity
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "info".to_string()),
        alert_type: Some(alert_type),
        suppressed: is_suppressed,
        deduplication_window_secs: if is_suppressed {
            Some(ALERT_DEDUPLICATION_WINDOW_SECS)
        } else {
            None
        },
        metadata: payload.metadata,
        received_at: Utc::now(),
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_alert_fingerprint_deterministic() {
        let fp1 = compute_alert_fingerprint("critical_error", "contract_runtime", "Out of memory");
        let fp2 = compute_alert_fingerprint("critical_error", "contract_runtime", "Out of memory");
        assert_eq!(fp1, fp2);
        assert_eq!(fp1.len(), 64);

        // Different message produces different fingerprint
        let fp3 = compute_alert_fingerprint("critical_error", "contract_runtime", "Different message");
        assert_ne!(fp1, fp3);

        // Different alert_type produces different fingerprint
        let fp4 = compute_alert_fingerprint("warning", "contract_runtime", "Out of memory");
        assert_ne!(fp1, fp4);

        // Different source produces different fingerprint
        let fp5 = compute_alert_fingerprint("critical_error", "db_pool", "Out of memory");
        assert_ne!(fp1, fp5);
    }

    #[test]
    fn test_alert_ingest_request_deserialization() {
        let json_data = r#"{
            "source": "validator",
            "message": "Signature failure",
            "alertType": "security",
            "severity": "high"
        }"#;

        let req: AlertIngestRequest = serde_json::from_str(json_data).unwrap();
        assert_eq!(req.source, "validator");
        assert_eq!(req.message, "Signature failure");
        assert_eq!(req.alert_type.as_deref(), Some("security"));
        assert_eq!(req.severity.as_deref(), Some("high"));
    }

    #[test]
    fn test_alert_ingest_response_serialization_suppressed() {
        let resp = AlertIngestResponse {
            alert_id: "alert-123".to_string(),
            accepted: false,
            source: "node".to_string(),
            message: "disk space low".to_string(),
            severity: "warning".to_string(),
            alert_type: Some("system".to_string()),
            suppressed: true,
            deduplication_window_secs: Some(300),
            metadata: None,
            received_at: Utc::now(),
        };

        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: AlertIngestResponse = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.accepted);
        assert!(deserialized.suppressed);
        assert_eq!(deserialized.deduplication_window_secs, Some(300));
        assert_eq!(deserialized.alert_type.as_deref(), Some("system"));
    }
}
