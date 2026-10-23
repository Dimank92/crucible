use crate::error::AppError;
use crate::services::tracing::TracingService;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Sender};
use tracing::{info, instrument, warn};

pub const DEFAULT_LOG_BUFFER_CAPACITY: usize = 10_000;
pub const DEFAULT_BATCH_SIZE: usize = 100;
pub const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ContractCallLog {
    pub id: Option<i32>,
    pub contract_id: String,
    pub function_name: String,
    pub arguments: serde_json::Value,
    pub caller: Option<String>,
    pub status: String,
    pub gas_used: f64,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone)]
pub struct ContractCallLogger {
    pub db: PgPool,
    sender: Sender<ContractCallLog>,
}

impl ContractCallLogger {
    pub fn new(db: PgPool) -> Self {
        Self::with_config(
            db,
            DEFAULT_LOG_BUFFER_CAPACITY,
            DEFAULT_BATCH_SIZE,
            DEFAULT_FLUSH_INTERVAL,
        )
    }

    pub fn with_config(
        db: PgPool,
        capacity: usize,
        batch_size: usize,
        flush_interval: Duration,
    ) -> Self {
        let (tx, mut rx) = channel::<ContractCallLog>(capacity);
        let db_worker = db.clone();

        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(batch_size);
            let mut interval = tokio::time::interval(flush_interval);

            loop {
                tokio::select! {
                    maybe_log = rx.recv() => {
                        match maybe_log {
                            Some(log) => {
                                batch.push(log);
                                if batch.len() >= batch_size {
                                    Self::flush_batch_internal(&db_worker, &batch).await;
                                    batch.clear();
                                }
                            }
                            None => {
                                if !batch.is_empty() {
                                    Self::flush_batch_internal(&db_worker, &batch).await;
                                    batch.clear();
                                }
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        if !batch.is_empty() {
                            Self::flush_batch_internal(&db_worker, &batch).await;
                            batch.clear();
                        }
                    }
                }
            }
        });

        Self { db, sender: tx }
    }

    async fn flush_batch_internal(db: &PgPool, batch: &[ContractCallLog]) {
        for log in batch {
            let res = sqlx::query(
                "INSERT INTO contract_call_logs (contract_id, function_name, arguments, caller, status, gas_used, timestamp) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)"
            )
            .bind(&log.contract_id)
            .bind(&log.function_name)
            .bind(&log.arguments)
            .bind(&log.caller)
            .bind(&log.status)
            .bind(&log.gas_used)
            .bind(log.timestamp.unwrap_or_else(chrono::Utc::now))
            .execute(db)
            .await;

            if let Err(e) = res {
                warn!(error = %e, contract_id = %log.contract_id, "Failed to flush contract call log to database");
            }
        }
    }

    /// Asynchronously enqueues contract invocation log into bounded buffer without blocking request thread.
    #[instrument(skip(self))]
    pub async fn log_call(&self, log: ContractCallLog) -> Result<(), AppError> {
        let contract_id = log.contract_id.clone();
        let function_name = log.function_name.clone();

        self.sender
            .send(log)
            .await
            .map_err(|e| AppError::InternalError(format!("Failed to buffer contract call log: {}", e)))?;

        info!(contract_id = %contract_id, function_name = %function_name, "Contract call buffered for batch flush");
        Ok(())
    }

    /// Non-blocking try_send for latency-critical paths where dropping/failing on full buffer is preferred
    pub fn try_log_call(&self, log: ContractCallLog) -> Result<(), AppError> {
        self.sender
            .try_send(log)
            .map_err(|e| AppError::InternalError(format!("Log channel buffer full or closed: {}", e)))
    }

    /// Direct synchronous database write for explicit synchronous workflows or tests
    #[instrument(skip(self))]
    pub async fn log_call_sync(&self, log: ContractCallLog) -> Result<(), AppError> {
        let db_span = TracingService::db_query_span(
            "INSERT INTO contract_call_logs",
            "postgres",
            "INSERT",
        );
        let _db_enter = db_span.enter();

        sqlx::query(
            "INSERT INTO contract_call_logs (contract_id, function_name, arguments, caller, status, gas_used, timestamp) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        )
        .bind(&log.contract_id)
        .bind(&log.function_name)
        .bind(&log.arguments)
        .bind(&log.caller)
        .bind(&log.status)
        .bind(&log.gas_used)
        .bind(log.timestamp.unwrap_or_else(chrono::Utc::now))
        .execute(&self.db)
        .await
        .map_err(AppError::db)?;

        info!(contract_id = %log.contract_id, function_name = %log.function_name, "Contract call synchronously logged");
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn get_logs(
        &self,
        contract_id: Option<String>,
        limit: i64,
    ) -> Result<Vec<ContractCallLog>, AppError> {
        let logs = if let Some(cid) = contract_id {
            sqlx::query_as::<_, ContractCallLog>(
                "SELECT id, contract_id, function_name, arguments, caller, status, gas_used, timestamp \
                 FROM contract_call_logs \
                 WHERE contract_id = $1 \
                 ORDER BY timestamp DESC \
                 LIMIT $2"
            )
            .bind(cid)
            .bind(limit)
            .fetch_all(&self.db)
            .await
            .map_err(AppError::db)?
        } else {
            sqlx::query_as::<_, ContractCallLog>(
                "SELECT id, contract_id, function_name, arguments, caller, status, gas_used, timestamp \
                 FROM contract_call_logs \
                 ORDER BY timestamp DESC \
                 LIMIT $1"
            )
            .bind(limit)
            .fetch_all(&self.db)
            .await
            .map_err(AppError::db)?
        };

        Ok(logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_buffered_contract_call_logger_non_blocking_send() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let logger = ContractCallLogger::with_config(
            pool,
            1000,
            10,
            Duration::from_millis(50),
        );

        let log = ContractCallLog {
            id: None,
            contract_id: "test_contract".to_string(),
            function_name: "test_fn".to_string(),
            arguments: serde_json::json!({}),
            caller: Some("caller1".to_string()),
            status: "success".to_string(),
            gas_used: 100.0,
            timestamp: Some(chrono::Utc::now()),
        };

        // Verifies non-blocking buffer enqueue without synchronous db stall
        let result = logger.log_call(log).await;
        assert!(result.is_ok());
    }
}
