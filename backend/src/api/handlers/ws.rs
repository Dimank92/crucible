//! WebSocket handler for real-time dashboard updates.
//!
//! Clients connect to `/api/v1/ws/dashboard` and receive a JSON push every
//! `PUSH_INTERVAL_SECS` seconds containing the latest dashboard metrics.
//! The connection is kept alive with periodic pings.
//!
//! Outbound message queuing enforces a strict backpressure buffer limit
//! and frame send timeout to protect against slow consumers and stalled TCP ACKs.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::services::{
    error_recovery::ErrorManager,
    sys_metrics::{MetricsExporter, SystemMetrics},
};

const PUSH_INTERVAL_SECS: u64 = 5;
const PING_INTERVAL_SECS: u64 = 30;

/// Default buffer limit for outbound messages per WebSocket client.
pub const DEFAULT_MAX_OUTBOUND_BUFFER: usize = 32;

/// Default timeout allowed for a single frame send before disconnecting a slow consumer.
pub const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(3);

/// Shared state required by the WebSocket handler.
#[derive(Clone)]
pub struct WsState {
    pub metrics_exporter: Arc<MetricsExporter>,
    pub error_manager: Arc<ErrorManager>,
    pub outbound_buffer_limit: usize,
    pub send_timeout: Duration,
}

impl WsState {
    pub fn new(metrics_exporter: Arc<MetricsExporter>, error_manager: Arc<ErrorManager>) -> Self {
        Self {
            metrics_exporter,
            error_manager,
            outbound_buffer_limit: DEFAULT_MAX_OUTBOUND_BUFFER,
            send_timeout: DEFAULT_SEND_TIMEOUT,
        }
    }

    pub fn with_outbound_buffer_limit(mut self, limit: usize) -> Self {
        self.outbound_buffer_limit = limit;
        self
    }

    pub fn with_send_timeout(mut self, timeout: Duration) -> Self {
        self.send_timeout = timeout;
        self
    }
}

/// The payload pushed to each connected client.
#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardUpdate {
    pub r#type: String,
    pub metrics: SystemMetrics,
    pub active_errors: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Axum handler — upgrades the HTTP connection to WebSocket.
pub async fn ws_dashboard_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WsState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<WsState>) {
    crate::services::sys_metrics::record_websocket_conn_change(1);
    let (mut sender, mut receiver) = socket.split();

    info!("WebSocket client connected for dashboard updates");

    let buffer_size = if state.outbound_buffer_limit > 0 {
        state.outbound_buffer_limit
    } else {
        DEFAULT_MAX_OUTBOUND_BUFFER
    };
    let send_timeout = state.send_timeout;

    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel::<Message>(buffer_size);

    let mut push_ticker = interval(Duration::from_secs(PUSH_INTERVAL_SECS));
    push_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ping_ticker = interval(Duration::from_secs(PING_INTERVAL_SECS));
    ping_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = push_ticker.tick() => {
                let metrics = state.metrics_exporter.get_metrics().await;
                let active_errors = state.error_manager.get_active_tasks().await.len();

                let update = DashboardUpdate {
                    r#type: "dashboard_update".to_string(),
                    metrics,
                    active_errors,
                    timestamp: chrono::Utc::now(),
                };

                match serde_json::to_string(&update) {
                    Ok(json) => {
                        match outbound_tx.try_send(Message::Text(json.into())) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                warn!(
                                    buffer_capacity = buffer_size,
                                    "WebSocket client dropped: outbound queue buffer limit exceeded (slow consumer)"
                                );
                                break;
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                debug!("WebSocket outbound channel closed");
                                break;
                            }
                        }
                    }
                    Err(e) => warn!(error = %e, "Failed to serialize dashboard update"),
                }
            }

            _ = ping_ticker.tick() => {
                match outbound_tx.try_send(Message::Ping(vec![].into())) {
                    Ok(()) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        warn!(
                            buffer_capacity = buffer_size,
                            "WebSocket client dropped: cannot queue ping, buffer full (slow consumer)"
                        );
                        break;
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                        break;
                    }
                }
            }

            outbound_msg = outbound_rx.recv() => {
                match outbound_msg {
                    Some(msg) => {
                        match tokio::time::timeout(send_timeout, sender.send(msg)).await {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) => {
                                debug!("WebSocket client disconnected (send failed), dropping stream");
                                break;
                            }
                            Err(_) => {
                                warn!(
                                    timeout_secs = send_timeout.as_secs(),
                                    "WebSocket client dropped: send timed out (slow consumer stalled TCP ACKs)"
                                );
                                break;
                            }
                        }
                    }
                    None => {
                        debug!("Outbound queue terminated, closing socket");
                        break;
                    }
                }
            }

            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        info!("WebSocket client explicitly disconnected or channel closed");
                        break;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        debug!("Received pong from WebSocket client");
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "WebSocket receive error, tearing down connection");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Teardown and decrement connection count immediately upon loop termination
    crate::services::sys_metrics::record_websocket_conn_change(-1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_update_serialization() {
        let metrics = SystemMetrics::default();
        let update = DashboardUpdate {
            r#type: "dashboard_update".to_string(),
            metrics,
            active_errors: 2,
            timestamp: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("dashboard_update"));
        assert!(json.contains("active_errors"));
    }

    #[test]
    fn test_ws_state_outbound_buffer_configuration() {
        let metrics = Arc::new(MetricsExporter::new());
        let error_manager = Arc::new(ErrorManager::new());
        let state = WsState::new(metrics.clone(), error_manager.clone());

        assert_eq!(state.outbound_buffer_limit, DEFAULT_MAX_OUTBOUND_BUFFER);
        assert_eq!(state.send_timeout, DEFAULT_SEND_TIMEOUT);

        let custom_state = state
            .with_outbound_buffer_limit(16)
            .with_send_timeout(Duration::from_millis(500));
        assert_eq!(custom_state.outbound_buffer_limit, 16);
        assert_eq!(custom_state.send_timeout, Duration::from_millis(500));
    }

    #[test]
    fn test_slow_consumer_buffer_overflow_detection() {
        let buffer_size = 2;
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(buffer_size);

        assert!(tx.try_send(Message::Text("msg1".into())).is_ok());
        assert!(tx.try_send(Message::Text("msg2".into())).is_ok());

        let overflow_res = tx.try_send(Message::Text("msg3".into()));
        assert!(matches!(
            overflow_res,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));
    }
}
