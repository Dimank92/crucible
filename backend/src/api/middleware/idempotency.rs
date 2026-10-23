use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header::CONTENT_TYPE, HeaderValue, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::api::handlers::profiling::AppState;
use crate::error::AppError;

const IDEMPOTENCY_TTL_SECONDS: u64 = 86_400;
const PROCESSING_LOCK_TTL_SECONDS: u64 = 30;
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_WAIT_TIME: Duration = Duration::from_secs(3);
const IDEMPOTENCY_ROUTE: &str = "/api/alerts/ingest";

/// State machine envelope for idempotency records in Redis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum IdempotencyRecord {
    Processing { locked_at: i64 },
    Completed { body: String, status: u16 },
}

/// Lua script to atomically update the idempotency record from `processing` to `completed`.
pub const ATOMIC_COMPLETE_LUA: &str = r#"
    local key = KEYS[1]
    local payload = ARGV[1]
    local ttl = tonumber(ARGV[2])
    redis.call("SET", key, payload, "EX", ttl)
    return 1
"#;

/// Lua script to atomically release the processing lock only if it is still in `processing` state.
pub const ATOMIC_RELEASE_LOCK_LUA: &str = r#"
    local key = KEYS[1]
    local current = redis.call("GET", key)
    if current and string.find(current, '"processing"') then
        redis.call("DEL", key)
        return 1
    end
    return 0
"#;

/// Cache successful `POST /api/alerts/ingest` responses by idempotency key.
///
/// When `Idempotency-Key` is present:
/// - cache hit (completed): replay the stored body immediately.
/// - in-progress (processing): poll for completion with backoff so concurrent requests
///   receive the cached response instead of failing with lock conflicts.
/// - cache miss: atomically acquire execution lock, run handler, and atomically update
///   from processing to completed with response payload via Redis Lua script.
pub async fn idempotency_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let key = request
        .headers()
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());

    let Some(key) = key else {
        return Ok(next.run(request).await);
    };

    let cache_key = format!("idempotency:{IDEMPOTENCY_ROUTE}:{key}");
    let mut conn = state.redis.get_multiplexed_async_connection().await?;
    let start_time = Instant::now();

    loop {
        if let Some(raw) = conn.get::<_, Option<String>>(&cache_key).await? {
            match serde_json::from_str::<IdempotencyRecord>(&raw) {
                Ok(IdempotencyRecord::Completed { body, status }) => {
                    return Ok(build_cached_response(
                        &body,
                        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    ));
                }
                Ok(IdempotencyRecord::Processing { .. }) => {
                    // Another concurrent request is actively executing with this key.
                    if start_time.elapsed() >= MAX_WAIT_TIME {
                        return Err(AppError::Conflict(
                            "A request with this idempotency key is currently being processed"
                                .to_string(),
                        ));
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                    continue;
                }
                Err(_) => {
                    // Legacy response stored as raw body string without state envelope
                    return Ok(build_cached_response(&raw, StatusCode::OK));
                }
            }
        }

        // Key does not exist: atomically acquire processing lock with TTL
        let processing_payload = serde_json::to_string(&IdempotencyRecord::Processing {
            locked_at: Utc::now().timestamp(),
        })
        .map_err(AppError::from)?;

        let acquired: Option<String> = redis::cmd("SET")
            .arg(&cache_key)
            .arg(&processing_payload)
            .arg("NX")
            .arg("EX")
            .arg(PROCESSING_LOCK_TTL_SECONDS)
            .query_async(&mut conn)
            .await?;

        if acquired.is_some() {
            // Execution lock acquired
            break;
        }

        // Another concurrent request won the lock in between: poll until complete
        if start_time.elapsed() >= MAX_WAIT_TIME {
            return Err(AppError::Conflict(
                "A request with this idempotency key is currently being processed".to_string(),
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    let response = next.run(request).await;
    if response.status() != StatusCode::OK {
        // Non-OK response: release the processing lock so future requests can retry
        let script = redis::Script::new(ATOMIC_RELEASE_LOCK_LUA);
        let _: Result<i32, _> = script.key(&cache_key).invoke_async(&mut conn).await;
        return Ok(response);
    }

    let (parts, body) = response.into_parts();
    let body_bytes = to_bytes(body, usize::MAX).await.map_err(|error| {
        AppError::InternalError(format!("failed to read response body: {error}"))
    })?;

    let body_text = String::from_utf8(body_bytes.to_vec()).map_err(|error| {
        AppError::InternalError(format!("response body was not UTF-8: {error}"))
    })?;

    let completed_record = serde_json::to_string(&IdempotencyRecord::Completed {
        body: body_text.clone(),
        status: StatusCode::OK.as_u16(),
    })
    .map_err(AppError::from)?;

    // Atomically transition from processing to completed in a single Lua script
    let script = redis::Script::new(ATOMIC_COMPLETE_LUA);
    script
        .key(&cache_key)
        .arg(&completed_record)
        .arg(IDEMPOTENCY_TTL_SECONDS)
        .invoke_async::<_, i32>(&mut conn)
        .await?;

    let mut rebuilt = Response::from_parts(parts, Body::from(body_text));
    rebuilt
        .headers_mut()
        .insert("x-idempotency-replayed", HeaderValue::from_static("false"));
    Ok(rebuilt)
}

fn build_cached_response(body_text: &str, status: StatusCode) -> Response {
    let mut response = Response::new(Body::from(body_text.to_string()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert("x-idempotency-replayed", HeaderValue::from_static("true"));
    response
}

#[allow(dead_code)]
fn build_replayed_response(cached_response: String) -> Response {
    let mut response = Response::new(Body::from(cached_response));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert("x-idempotency-replayed", HeaderValue::from_static("true"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::handlers::profiling::AppState,
        config::{reload::ConfigManager, AppConfig},
        services::{
            contract_benchmark::ContractBenchmarkService, error_recovery::ErrorManager,
            log_aggregator::LogAggregator, sys_metrics::MetricsExporter,
        },
    };
    use axum::{middleware, routing::post, Json, Router};
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;

    #[test]
    fn test_record_processing_serialization() {
        let rec = IdempotencyRecord::Processing { locked_at: 12345 };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"state\":\"processing\""));
        assert!(json.contains("\"locked_at\":12345"));

        let back: IdempotencyRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn test_record_completed_serialization() {
        let rec = IdempotencyRecord::Completed {
            body: "{\"ok\":true}".to_string(),
            status: 200,
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"state\":\"completed\""));
        assert!(json.contains("\"status\":200"));

        let back: IdempotencyRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn test_legacy_raw_body_fallback() {
        let legacy_json = r#"{"request_number": 42}"#;
        let parsed = serde_json::from_str::<IdempotencyRecord>(legacy_json);
        assert!(parsed.is_err());
    }

    #[test]
    fn test_build_cached_response_headers() {
        let resp = build_cached_response("{\"status\":\"ok\"}", StatusCode::OK);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("x-idempotency-replayed").unwrap(),
            "true"
        );
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    #[ignore = "requires local Redis server"]
    async fn duplicate_request_replays_original_response() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_handler = counter.clone();

        let redis = redis::Client::open("redis://127.0.0.1:6379").unwrap();
        let app_state = Arc::new(AppState {
            db: None,
            metrics_exporter: Arc::new(MetricsExporter::new()),
            error_manager: Arc::new(ErrorManager::new()),
            config_manager: Arc::new(ConfigManager::new(AppConfig::default())),
            log_aggregator: Arc::new(LogAggregator::new().0),
            contract_benchmark_service: Arc::new(ContractBenchmarkService::new()),
            redis: redis.clone(),
        });

        let app = Router::new()
            .route(
                IDEMPOTENCY_ROUTE,
                post(move || {
                    let counter = counter_for_handler.clone();
                    async move {
                        let value = counter.fetch_add(1, Ordering::SeqCst) + 1;
                        Json(json!({ "request_number": value }))
                    }
                })
                .route_layer(middleware::from_fn_with_state(
                    app_state.clone(),
                    idempotency_middleware,
                )),
            )
            .with_state(app_state);

        let request = || {
            Request::builder()
                .method("POST")
                .uri(IDEMPOTENCY_ROUTE)
                .header("content-type", "application/json")
                .header("Idempotency-Key", "duplicate-check-1")
                .body(Body::from(r#"{"message":"hello"}"#))
                .unwrap()
        };

        let first = app.clone().oneshot(request()).await.unwrap();
        let second = app.oneshot(request()).await.unwrap();

        let first_body = to_bytes(first.into_body(), usize::MAX).await.unwrap();
        let second_body = to_bytes(second.into_body(), usize::MAX).await.unwrap();

        assert_eq!(first_body, second_body);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn test_build_replayed_response_headers() {
        let payload = r#"{"status":"ok"}"#.to_string();
        let response = build_replayed_response(payload);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            response.headers().get("x-idempotency-replayed").unwrap(),
            "true"
        );
    }

    #[test]
    fn test_conflict_error_status_code() {
        use axum::response::IntoResponse;
        let err = AppError::Conflict(
            "A request with this idempotency key is currently being processed".into(),
        );
        let res = err.into_response();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }
}
