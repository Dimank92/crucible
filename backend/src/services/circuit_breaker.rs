//! Circuit breaker pattern for external service calls.
//!
//! Implements the classic three-state circuit breaker (Closed → Open → Half-Open)
//! to prevent cascading failures when external services are unavailable.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// The state of a circuit breaker.
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    /// Normal operation — requests pass through.
    Closed,
    /// Service is failing — requests are rejected immediately.
    Open { opened_at: Instant },
    /// Testing recovery — one probe request is allowed through.
    HalfOpen,
}

/// Configuration for a circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures within the failure window before opening the circuit.
    pub failure_threshold: u32,
    /// How long to wait in Open state before transitioning to HalfOpen.
    pub recovery_timeout: Duration,
    /// Number of consecutive successes in HalfOpen before closing.
    pub success_threshold: u32,
    /// Sliding time window during which failures are tracked.
    pub failure_window: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            success_threshold: 2,
            failure_window: Duration::from_secs(60),
        }
    }
}

#[derive(Debug)]
struct CircuitBreakerInner {
    state: CircuitState,
    failure_timestamps: Vec<Instant>,
    consecutive_successes: u32,
    config: CircuitBreakerConfig,
}

impl CircuitBreakerInner {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_timestamps: Vec::new(),
            consecutive_successes: 0,
            config,
        }
    }

    fn prune_failures(&mut self, now: Instant) {
        let window = self.config.failure_window;
        self.failure_timestamps.retain(|&t| {
            now.checked_duration_since(t)
                .map_or(true, |elapsed| elapsed <= window)
        });
    }

    fn is_request_allowed(&mut self) -> bool {
        match &self.state {
            CircuitState::Closed => true,
            CircuitState::Open { opened_at } => {
                if opened_at.elapsed() >= self.config.recovery_timeout {
                    self.state = CircuitState::HalfOpen;
                    self.consecutive_successes = 0;
                    info!("Circuit breaker transitioning to HalfOpen");
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    fn record_success(&mut self) {
        match self.state {
            CircuitState::HalfOpen => {
                self.consecutive_successes += 1;
                if self.consecutive_successes >= self.config.success_threshold {
                    self.state = CircuitState::Closed;
                    self.failure_timestamps.clear();
                    info!("Circuit breaker closed after successful recovery");
                }
            }
            CircuitState::Closed => {
                // Prune expired failures from sliding window, but do not
                // clear unexpired failures on an isolated single success.
                self.prune_failures(Instant::now());
            }
            _ => {}
        }
    }

    fn record_failure(&mut self) {
        let now = Instant::now();
        self.prune_failures(now);
        self.failure_timestamps.push(now);

        match self.state {
            CircuitState::Closed => {
                let failure_count = self.failure_timestamps.len() as u32;
                if failure_count >= self.config.failure_threshold {
                    self.state = CircuitState::Open { opened_at: now };
                    warn!(
                        failures = failure_count,
                        "Circuit breaker opened after threshold exceeded"
                    );
                }
            }
            CircuitState::HalfOpen => {
                self.state = CircuitState::Open { opened_at: now };
                self.consecutive_successes = 0;
                warn!("Circuit breaker re-opened after probe failure");
            }
            _ => {}
        }
    }
}

/// A thread-safe circuit breaker for wrapping external service calls.
#[derive(Clone)]
pub struct CircuitBreaker {
    name: String,
    inner: Arc<RwLock<CircuitBreakerInner>>,
}

#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    #[error("Circuit is open for service '{0}'")]
    Open(String),
    #[error("Service call failed: {0}")]
    ServiceError(E),
}

impl CircuitBreaker {
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            inner: Arc::new(RwLock::new(CircuitBreakerInner::new(config))),
        }
    }

    /// Execute a fallible async closure through the circuit breaker.
    pub async fn call<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        {
            let mut inner = self.inner.write().await;
            if !inner.is_request_allowed() {
                return Err(CircuitBreakerError::Open(self.name.clone()));
            }
        }

        match f().await {
            Ok(value) => {
                self.inner.write().await.record_success();
                Ok(value)
            }
            Err(e) => {
                self.inner.write().await.record_failure();
                Err(CircuitBreakerError::ServiceError(e))
            }
        }
    }

    /// Returns the current state label for observability.
    pub async fn state_label(&self) -> &'static str {
        match self.inner.read().await.state {
            CircuitState::Closed => "closed",
            CircuitState::Open { .. } => "open",
            CircuitState::HalfOpen => "half_open",
        }
    }

    /// Returns the number of unexpired failures in the sliding window.
    pub async fn failure_count(&self) -> usize {
        let mut inner = self.inner.write().await;
        inner.prune_failures(Instant::now());
        inner.failure_timestamps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_millis(50),
            success_threshold: 1,
            failure_window: Duration::from_secs(60),
        }
    }

    #[tokio::test]
    async fn test_closed_allows_requests() {
        let cb = CircuitBreaker::new("test", fast_config());
        let result = cb.call(|| async { Ok::<_, String>("ok") }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new("test", fast_config());
        for _ in 0..3 {
            let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        }
        let result = cb.call(|| async { Ok::<_, String>("ok") }).await;
        assert!(matches!(result, Err(CircuitBreakerError::Open(_))));
    }

    #[tokio::test]
    async fn test_recovers_after_timeout() {
        let cb = CircuitBreaker::new("test", fast_config());
        for _ in 0..3 {
            let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
        let result = cb.call(|| async { Ok::<_, String>("ok") }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_state_label() {
        let cb = CircuitBreaker::new("test", fast_config());
        assert_eq!(cb.state_label().await, "closed");
        for _ in 0..3 {
            let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        }
        assert_eq!(cb.state_label().await, "open");
    }

    #[tokio::test]
    async fn test_fluttering_failures_trip_breaker_sliding_window() {
        let cb = CircuitBreaker::new("test", fast_config());
        // Fail twice
        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        assert_eq!(cb.failure_count().await, 2);

        // A single success should NOT reset failure history in Closed state
        let ok_res = cb.call(|| async { Ok::<_, String>("ok") }).await;
        assert!(ok_res.is_ok());
        assert_eq!(cb.failure_count().await, 2);

        // Third failure within the sliding window should trip the breaker to Open
        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        assert_eq!(cb.state_label().await, "open");

        // Subsequent call is rejected by open circuit breaker
        let blocked = cb.call(|| async { Ok::<_, String>("ok") }).await;
        assert!(matches!(blocked, Err(CircuitBreakerError::Open(_))));
    }

    #[tokio::test]
    async fn test_failures_expire_outside_sliding_window() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_millis(50),
            success_threshold: 1,
            failure_window: Duration::from_millis(60),
        };
        let cb = CircuitBreaker::new("test", config);

        // Two failures
        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        assert_eq!(cb.failure_count().await, 2);

        // Wait for sliding window to expire
        tokio::time::sleep(Duration::from_millis(80)).await;

        // A success or failure now prunes expired failures
        let ok_res = cb.call(|| async { Ok::<_, String>("ok") }).await;
        assert!(ok_res.is_ok());
        assert_eq!(cb.failure_count().await, 0);

        // A subsequent failure only has count 1, does not trip
        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        assert_eq!(cb.failure_count().await, 1);
        assert_eq!(cb.state_label().await, "closed");
    }

    #[tokio::test]
    async fn test_recovery_clears_sliding_window() {
        let cb = CircuitBreaker::new("test", fast_config());
        // Trip breaker
        for _ in 0..3 {
            let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        }
        assert_eq!(cb.state_label().await, "open");

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Successful probe resets circuit to closed and clears failure window
        let probe = cb.call(|| async { Ok::<_, String>("ok") }).await;
        assert!(probe.is_ok());
        assert_eq!(cb.state_label().await, "closed");
        assert_eq!(cb.failure_count().await, 0);
    }
}
