//! Error recovery service.
//!
//! Tracks retry state for failing tasks with configurable max retries and
//! truncated exponential backoff with jitter to protect downstream services.

#![allow(dead_code)]

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{error, info, instrument, warn};
use crate::services::tracing::TracingService;

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RecoveryError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Redis error: {0}")]
    Redis(String),
    #[error("Internal service error: {0}")]
    Internal(String),
    #[error("Max retries reached for task: {0}")]
    MaxRetriesReached(String),
    #[error("Task not found: {0}")]
    TaskNotFound(String),
}

/// Configuration parameters for truncated exponential backoff with jitter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackoffConfig {
    /// Initial base backoff duration in milliseconds (default: 500ms).
    pub base_backoff_ms: u64,
    /// Maximum backoff ceiling in milliseconds (default: 30,000ms / 30s).
    pub max_backoff_ms: u64,
    /// Exponential growth multiplier (default: 2.0).
    pub multiplier: f64,
    /// Full jitter fraction [0.0, 1.0] applied to backoff intervals (default: 0.5).
    pub jitter_factor: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            base_backoff_ms: 500,
            max_backoff_ms: 30_000,
            multiplier: 2.0,
            jitter_factor: 0.5,
        }
    }
}

impl BackoffConfig {
    /// Calculate truncated exponential backoff with randomized jitter for a retry attempt.
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        let rng_ratio: f64 = rand::random();
        self.calculate_backoff_with_ratio(attempt, rng_ratio)
    }

    /// Calculate truncated exponential backoff using an explicit jitter ratio in [0.0, 1.0].
    pub fn calculate_backoff_with_ratio(&self, attempt: u32, ratio: f64) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        // Exponential backoff: base * (multiplier ^ (attempt - 1))
        let exp = (attempt - 1).min(16);
        let raw_backoff = (self.base_backoff_ms as f64) * self.multiplier.powi(exp as i32);
        let truncated = raw_backoff.min(self.max_backoff_ms as f64);

        // Truncated exponential backoff with full jitter:
        // [truncated * (1 - jitter_factor), truncated]
        let min_bound = truncated * (1.0 - self.jitter_factor.clamp(0.0, 1.0));
        let span = truncated - min_bound;
        let jittered = min_bound + (ratio.clamp(0.0, 1.0) * span);
        let final_ms = (jittered as u64).min(self.max_backoff_ms);

        Duration::from_millis(final_ms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryTask {
    pub id: uuid::Uuid,
    pub name: String,
    pub retries: u32,
    pub max_retries: u32,
    #[serde(default)]
    pub next_retry_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub backoff_duration_ms: u64,
}

pub struct ErrorManager {
    tasks: Arc<RwLock<Vec<RecoveryTask>>>,
    config: BackoffConfig,
}

impl Default for ErrorManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorManager {
    pub fn new() -> Self {
        Self::with_config(BackoffConfig::default())
    }

    pub fn with_config(config: BackoffConfig) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    /// Return reference to the active backoff configuration.
    pub fn config(&self) -> &BackoffConfig {
        &self.config
    }

    /// Compute truncated exponential backoff for a given attempt.
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        self.config.calculate_backoff(attempt)
    }

    #[instrument(skip(self), fields(service.name = "ErrorManager", service.method = "handle_error"))]
    pub async fn handle_error(&self, error: RecoveryError, task_name: &str) -> Result<(), RecoveryError> {
        self.handle_error_with_backoff(error, task_name).await.map(|_| ())
    }

    /// Record an error and return the computed backoff delay for this retry attempt.
    pub async fn handle_error_with_backoff(
        &self,
        error: RecoveryError,
        task_name: &str,
    ) -> Result<Duration, RecoveryError> {
        let span = TracingService::service_method_span("ErrorManager", "handle_error");
        let _enter = span.enter();

        warn!(task = %task_name, error = %error, "Handling error with backoff");

        let mut tasks = self.tasks.write().await;
        let now = Utc::now();

        if let Some(task) = tasks.iter_mut().find(|t| t.name == task_name) {
            task.retries += 1;
            if task.retries > task.max_retries {
                error!(task = %task_name, "Max retries reached");
                TracingService::record_error(
                    &span,
                    &format!("Max retries reached for {}", task_name),
                    "max_retries",
                );
                return Err(RecoveryError::MaxRetriesReached(task_name.to_string()));
            }

            let backoff = self.config.calculate_backoff(task.retries);
            task.backoff_duration_ms = backoff.as_millis() as u64;
            task.next_retry_at = Some(now + ChronoDuration::milliseconds(backoff.as_millis() as i64));

            info!(
                task = %task_name,
                retry = task.retries,
                backoff_ms = task.backoff_duration_ms,
                "Retrying task with exponential backoff and jitter"
            );
            Ok(backoff)
        } else {
            let backoff = self.config.calculate_backoff(1);
            let next_retry_at = Some(now + ChronoDuration::milliseconds(backoff.as_millis() as i64));

            tasks.push(RecoveryTask {
                id: uuid::Uuid::new_v4(),
                name: task_name.to_string(),
                retries: 1,
                max_retries: 3,
                next_retry_at,
                backoff_duration_ms: backoff.as_millis() as u64,
            });

            info!(
                task = %task_name,
                backoff_ms = backoff.as_millis(),
                "Registered new recovery task with exponential backoff"
            );
            Ok(backoff)
        }
    }

    /// Check if a task has satisfied its backoff delay and is ready for execution.
    pub async fn is_task_ready(&self, task_name: &str) -> bool {
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.iter().find(|t| t.name == task_name) {
            match task.next_retry_at {
                Some(next_time) => Utc::now() >= next_time,
                None => true,
            }
        } else {
            true
        }
    }

    #[instrument(skip(self), fields(service.name = "ErrorManager", service.method = "get_active_tasks"))]
    pub async fn get_active_tasks(&self) -> Vec<RecoveryTask> {
        let span = TracingService::service_method_span("ErrorManager", "get_active_tasks");
        let _enter = span.enter();
        self.tasks.read().await.clone()
    }

    /// Remove a resolved or completed task from recovery tracking.
    pub async fn remove_task(&self, task_name: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        let initial_len = tasks.len();
        tasks.retain(|t| t.name != task_name);
        tasks.len() < initial_len
    }

    /// Reset recovery retries and backoff for a recovered task.
    pub async fn reset_task(&self, task_name: &str) -> Result<(), RecoveryError> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.name == task_name) {
            task.retries = 0;
            task.next_retry_at = None;
            task.backoff_duration_ms = 0;
            Ok(())
        } else {
            Err(RecoveryError::TaskNotFound(task_name.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_exponential_growth_and_truncation() {
        let config = BackoffConfig {
            base_backoff_ms: 100,
            max_backoff_ms: 1000,
            multiplier: 2.0,
            jitter_factor: 0.0, // Zero jitter for deterministic verification
        };

        // Attempt 1: 100 * 2^0 = 100ms
        assert_eq!(config.calculate_backoff_with_ratio(1, 0.5), Duration::from_millis(100));
        // Attempt 2: 100 * 2^1 = 200ms
        assert_eq!(config.calculate_backoff_with_ratio(2, 0.5), Duration::from_millis(200));
        // Attempt 3: 100 * 2^2 = 400ms
        assert_eq!(config.calculate_backoff_with_ratio(3, 0.5), Duration::from_millis(400));
        // Attempt 4: 100 * 2^3 = 800ms
        assert_eq!(config.calculate_backoff_with_ratio(4, 0.5), Duration::from_millis(800));
        // Attempt 5: 100 * 2^4 = 1600ms -> Truncated to 1000ms
        assert_eq!(config.calculate_backoff_with_ratio(5, 0.5), Duration::from_millis(1000));
        // Attempt 10: Truncated to 1000ms
        assert_eq!(config.calculate_backoff_with_ratio(10, 0.5), Duration::from_millis(1000));
    }

    #[test]
    fn test_backoff_jitter_bounds() {
        let config = BackoffConfig {
            base_backoff_ms: 1000,
            max_backoff_ms: 10_000,
            multiplier: 2.0,
            jitter_factor: 0.5,
        };

        // Attempt 1: truncated = 1000ms. Range [500ms, 1000ms]
        let min_jitter = config.calculate_backoff_with_ratio(1, 0.0);
        let max_jitter = config.calculate_backoff_with_ratio(1, 1.0);
        assert_eq!(min_jitter, Duration::from_millis(500));
        assert_eq!(max_jitter, Duration::from_millis(1000));

        // Randomized calculate_backoff must stay within bounds
        for _ in 0..50 {
            let b = config.calculate_backoff(1);
            assert!(b >= Duration::from_millis(500) && b <= Duration::from_millis(1000));
        }
    }

    #[tokio::test]
    async fn test_error_recovery_workflow() {
        let manager = ErrorManager::new();
        let task_name = "test_task";

        manager
            .handle_error(RecoveryError::Database("connection lost".to_string()), task_name)
            .await
            .unwrap();
        assert_eq!(manager.get_active_tasks().await.len(), 1);
        let t1 = &manager.get_active_tasks().await[0];
        assert_eq!(t1.retries, 1);
        assert!(t1.backoff_duration_ms > 0);
        assert!(t1.next_retry_at.is_some());

        manager
            .handle_error(RecoveryError::Redis("timeout".to_string()), task_name)
            .await
            .unwrap();
        let t2 = &manager.get_active_tasks().await[0];
        assert_eq!(t2.retries, 2);
        assert!(t2.backoff_duration_ms > 0);

        manager
            .handle_error(RecoveryError::Internal("unknown".to_string()), task_name)
            .await
            .unwrap();
        let t3 = &manager.get_active_tasks().await[0];
        assert_eq!(t3.retries, 3);

        let result = manager
            .handle_error(RecoveryError::Internal("last straw".to_string()), task_name)
            .await;
        assert!(matches!(result, Err(RecoveryError::MaxRetriesReached(_))));
    }

    #[tokio::test]
    async fn test_task_reset_and_remove() {
        let manager = ErrorManager::new();
        let task_name = "resettable_task";

        manager
            .handle_error(RecoveryError::Database("transient error".into()), task_name)
            .await
            .unwrap();

        assert_eq!(manager.get_active_tasks().await[0].retries, 1);
        manager.reset_task(task_name).await.unwrap();
        assert_eq!(manager.get_active_tasks().await[0].retries, 0);
        assert_eq!(manager.get_active_tasks().await[0].backoff_duration_ms, 0);

        assert!(manager.remove_task(task_name).await);
        assert!(manager.get_active_tasks().await.is_empty());
    }
}
