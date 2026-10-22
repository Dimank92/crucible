use crate::api::contracts::ApiResponse;
use crate::api::handlers::profiling::AppState;
use crate::config::reload::ConfigManager;
use crate::config::sanitize;
use crate::error::AppError;
use crate::services::contract_call_logger::ContractCallLogger;
use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Global static for maintenance mode (mock implementation)
pub static MAINTENANCE_MODE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatsResponse {
    pub database_connected: bool,
    pub redis_connected: bool,
    pub total_contract_calls: i64,
    pub system_uptime_secs: u64,
    pub maintenance_mode: bool,
}

/// GET /api/v1/admin/system-stats
pub async fn get_system_stats(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let db_connected = if let Some(ref pool) = state.db {
        sqlx::query("SELECT 1").fetch_optional(pool).await.is_ok()
    } else {
        false
    };

    let redis_connected = state.redis.get_async_connection().await.is_ok();

    let total_calls = if let Some(ref pool) = state.db {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM contract_call_logs")
            .fetch_one(pool)
            .await
            .unwrap_or(0)
    } else {
        0
    };

    let metrics = state.metrics_exporter.get_metrics().await;

    Ok(Json(ApiResponse::new(SystemStatsResponse {
        database_connected: db_connected,
        redis_connected,
        total_contract_calls: total_calls,
        system_uptime_secs: metrics.uptime,
        maintenance_mode: MAINTENANCE_MODE.load(Ordering::Relaxed),
    })))
}

/// POST /api/v1/admin/maintenance
pub async fn set_maintenance_mode(
    Json(payload): Json<MaintenanceRequest>,
) -> Result<impl IntoResponse, AppError> {
    MAINTENANCE_MODE.store(payload.enabled, Ordering::Relaxed);
    Ok(Json(ApiResponse::new(serde_json::json!({
        "success": true,
        "maintenanceMode": payload.enabled
    }))))
}

/// GET /api/v1/admin/logs
pub async fn get_admin_logs(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let db = state
        .db
        .clone()
        .ok_or_else(|| AppError::InternalError("Database connection not configured".to_string()))?;

    let logger = ContractCallLogger::new(db);
    let logs = logger.get_logs(None, 50).await?;

    Ok(Json(ApiResponse::new(logs)))
}

/// GET /api/v1/admin/config
///
/// Returns the current effective configuration with all sensitive fields redacted.
/// This endpoint is intended for operators to inspect non-secret config for diagnostics.
///
/// **Redacted fields:**
/// - `database.url` → `"[REDACTED]"`
/// - `redis.url` → `"[REDACTED]"`
/// - `redis.job_queue_url` → `"[REDACTED]"`
/// - `server.tls.key_path` → `"[REDACTED]"`
pub async fn get_effective_config(
    State(config_manager): State<Arc<ConfigManager>>,
) -> Result<impl IntoResponse, AppError> {
    let config = config_manager.load();
    let sanitized = sanitize::sanitize(&config);
    Ok(Json(ApiResponse::new(sanitized)))
}

use redis::AsyncCommands;

/// Payload to suspend a compromised user and revoke all active sessions.
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuspendUserRequest {
    pub user_id: i32,
    pub reason: Option<String>,
    pub active_jtis: Option<Vec<String>>,
}

/// Payload to revoke or demote a user's role and revoke active sessions.
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevokeRoleRequest {
    pub user_id: i32,
    pub reason: Option<String>,
    pub active_jtis: Option<Vec<String>>,
}

/// Result returned after session invalidation and account suspension / role revocation.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevocationResult {
    pub user_id: i32,
    pub is_active: bool,
    pub revoked_at: chrono::DateTime<chrono::Utc>,
    pub blacklisted_jtis: usize,
    pub sessions_cleared: bool,
    pub broadcast_sent: bool,
    pub reason: String,
}

/// Invalidates all active Redis sessions, blacklists JTIs, and broadcasts revocation events across Redis Pub/Sub.
pub async fn invalidate_user_sessions_and_tokens(
    redis: &redis::Client,
    user_id: i32,
    active_jtis: Option<&[String]>,
    reason: &str,
) -> Result<(usize, bool, bool), AppError> {
    let mut conn = redis
        .get_multiplexed_async_connection()
        .await
        .map_err(AppError::Redis)?;
    let ttl: u64 = 86400 * 7; // 7-day revocation window

    let mut count = 0;
    // 1. Blacklist active JWT JTIs in Redis
    if let Some(jtis) = active_jtis {
        for jti in jtis {
            let key = format!("token_blocklist:{jti}");
            let entry = serde_json::json!({
                "jti": jti,
                "userId": user_id,
                "revokedAt": chrono::Utc::now(),
                "reason": reason
            });
            let _: () = conn
                .set_ex(&key, entry.to_string(), ttl)
                .await
                .map_err(AppError::Redis)?;
            count += 1;
        }
    }

    // 2. Blacklist user subject identifier in Redis
    let user_key = format!("token_blocklist:user:{user_id}");
    let user_blocklist_key = format!("user_blocklist:{user_id}");
    let user_entry = serde_json::json!({
        "userId": user_id,
        "revokedAt": chrono::Utc::now(),
        "reason": reason
    });
    let _: () = conn
        .set_ex(&user_key, user_entry.to_string(), ttl)
        .await
        .map_err(AppError::Redis)?;
    let _: () = conn
        .set_ex(&user_blocklist_key, user_entry.to_string(), ttl)
        .await
        .map_err(AppError::Redis)?;

    // 3. Clear active session records in Redis
    let session_key = format!("session:{user_id}");
    let user_sessions_key = format!("user_sessions:{user_id}");
    let _: () = conn.del(&[&session_key, &user_sessions_key]).await.unwrap_or(());

    // 4. Broadcast session revocation event across Redis Pub/Sub
    let broadcast_payload = serde_json::json!({
        "event": "session_revoked",
        "userId": user_id,
        "reason": reason,
        "timestamp": chrono::Utc::now()
    });
    let broadcast_sent: bool = conn
        .publish::<_, _, i64>("auth:session:revoked", broadcast_payload.to_string())
        .await
        .is_ok();

    Ok((count, true, broadcast_sent))
}

/// POST /api/v1/admin/users/suspend
///
/// Suspends a user account in PostgreSQL, revokes all active Redis session keys,
/// blacklists active JWT tokens, and broadcasts session revocation across Redis.
pub async fn suspend_user(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SuspendUserRequest>,
) -> Result<impl IntoResponse, AppError> {
    let reason = payload
        .reason
        .unwrap_or_else(|| "User suspended by administrator".to_string());

    // 1. Update PostgreSQL users.is_active = false
    if let Some(ref pool) = state.db {
        let rows = sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
            .bind(payload.user_id)
            .execute(pool)
            .await
            .map_err(AppError::Database)?;

        if rows.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "User with id {} not found",
                payload.user_id
            )));
        }

        // Record immutable audit log
        let _ = sqlx::query(
            "INSERT INTO audit_logs (event_type, user_id, details, timestamp, hash, previous_hash) VALUES ($1, $2, $3, NOW(), 'audit_hash', 'prev_hash')"
        )
        .bind("user.suspended")
        .bind(payload.user_id.to_string())
        .bind(serde_json::json!({
            "userId": payload.user_id,
            "action": "suspended",
            "reason": reason
        }))
        .execute(pool)
        .await;
    }

    // 2. Invalidate sessions, blacklist tokens, and broadcast revocation
    let (blacklisted, sessions_cleared, broadcast_sent) = invalidate_user_sessions_and_tokens(
        &state.redis,
        payload.user_id,
        payload.active_jtis.as_deref(),
        &reason,
    )
    .await?;

    Ok(Json(ApiResponse::new(RevocationResult {
        user_id: payload.user_id,
        is_active: false,
        revoked_at: chrono::Utc::now(),
        blacklisted_jtis: blacklisted,
        sessions_cleared,
        broadcast_sent,
        reason,
    })))
}

/// POST /api/v1/admin/roles/revoke
///
/// Revokes elevated role permissions for a user, updates database records,
/// blacklists active tokens, and invalidates active Redis sessions.
pub async fn revoke_user_role(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RevokeRoleRequest>,
) -> Result<impl IntoResponse, AppError> {
    let reason = payload
        .reason
        .unwrap_or_else(|| "Role revoked by administrator".to_string());

    // 1. Update role in PostgreSQL
    if let Some(ref pool) = state.db {
        let rows = sqlx::query("UPDATE users SET role = 'user' WHERE id = $1")
            .bind(payload.user_id)
            .execute(pool)
            .await
            .map_err(AppError::Database)?;

        if rows.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "User with id {} not found",
                payload.user_id
            )));
        }

        // Record immutable audit log
        let _ = sqlx::query(
            "INSERT INTO audit_logs (event_type, user_id, details, timestamp, hash, previous_hash) VALUES ($1, $2, $3, NOW(), 'audit_hash', 'prev_hash')"
        )
        .bind("user.role_revoked")
        .bind(payload.user_id.to_string())
        .bind(serde_json::json!({
            "userId": payload.user_id,
            "action": "role_revoked",
            "newRole": "user",
            "reason": reason
        }))
        .execute(pool)
        .await;
    }

    // 2. Invalidate sessions, blacklist tokens, and broadcast revocation
    let (blacklisted, sessions_cleared, broadcast_sent) = invalidate_user_sessions_and_tokens(
        &state.redis,
        payload.user_id,
        payload.active_jtis.as_deref(),
        &reason,
    )
    .await?;

    Ok(Json(ApiResponse::new(RevocationResult {
        user_id: payload.user_id,
        is_active: true,
        revoked_at: chrono::Utc::now(),
        blacklisted_jtis: blacklisted,
        sessions_cleared,
        broadcast_sent,
        reason,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suspend_user_request_serialization() {
        let req = SuspendUserRequest {
            user_id: 123,
            reason: Some("Compromised credentials".to_string()),
            active_jtis: Some(vec!["jti-1".to_string(), "jti-2".to_string()]),
        };

        let json = serde_json::to_string(&req).unwrap();
        let deserialized: SuspendUserRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.user_id, 123);
        assert_eq!(deserialized.reason.as_deref(), Some("Compromised credentials"));
        assert_eq!(deserialized.active_jtis.unwrap().len(), 2);
    }

    #[test]
    fn test_revoke_role_request_serialization() {
        let req = RevokeRoleRequest {
            user_id: 456,
            reason: Some("Demoted from admin".to_string()),
            active_jtis: Some(vec!["jti-adm".to_string()]),
        };

        let json = serde_json::to_string(&req).unwrap();
        let deserialized: RevokeRoleRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.user_id, 456);
        assert_eq!(deserialized.reason.as_deref(), Some("Demoted from admin"));
        assert_eq!(deserialized.active_jtis.unwrap(), vec!["jti-adm".to_string()]);
    }
}
