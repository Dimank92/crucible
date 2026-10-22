//! HTTP handlers for the contract test coverage reporting API.
//!
//! Routes:
//!   POST /api/v1/coverage                      – submit a new coverage snapshot
//!   GET  /api/v1/coverage/:project             – retrieve the latest snapshot for a project
//!   GET  /api/v1/coverage/:contract_id/history – retrieve progression history (composite index backed)
//!   GET  /api/v1/coverage/progression/:contract_id – alternative path for progression history

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::services::test_coverage::{NewTestCoverage, TestCoverageService};
use crate::error::AppError;

/// Shared state for coverage routes.
#[derive(Clone)]
pub struct CoverageState {
    pub service: TestCoverageService,
}

/// Query parameters for coverage historical queries.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CoverageHistoryQuery {
    pub limit: Option<i64>,
}

/// POST /api/v1/coverage
///
/// Accepts a JSON coverage payload and persists a new snapshot.
/// Returns `201 Created` with the stored record on success.
pub async fn submit_coverage(
    State(state): State<Arc<CoverageState>>,
    Json(payload): Json<NewTestCoverage>,
) -> Result<impl IntoResponse, AppError> {
    let record = state
        .service
        .submit_coverage(payload)
        .await
        .map_err(AppError::from)?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// GET /api/v1/coverage/:project
///
/// Returns the most-recent coverage snapshot for `project`.
/// Returns `404 Not Found` when no report has been submitted yet.
pub async fn get_latest_coverage(
    State(state): State<Arc<CoverageState>>,
    Path(project): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let record = state
        .service
        .get_latest_coverage(&project)
        .await
        .map_err(AppError::from)?;
    Ok(Json(record))
}

/// GET /api/v1/coverage/:contract_id/history
/// GET /api/v1/coverage/progression/:contract_id
///
/// Returns historical coverage records sorted by `created_at DESC`.
/// Supports `?limit=N` (defaults to 20, clamped 1..=100).
pub async fn get_coverage_progression(
    State(state): State<Arc<CoverageState>>,
    Path(contract_id): Path<String>,
    Query(query): Query<CoverageHistoryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = query.limit.unwrap_or(20);
    let records = state
        .service
        .get_coverage_progression(&contract_id, limit)
        .await
        .map_err(AppError::from)?;
    Ok(Json(records))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_deserializes_default() {
        let q: CoverageHistoryQuery = serde_urlencoded::from_str("").unwrap();
        assert_eq!(q.limit, None);
    }

    #[test]
    fn query_deserializes_limit() {
        let q: CoverageHistoryQuery = serde_urlencoded::from_str("limit=50").unwrap();
        assert_eq!(q.limit, Some(50));
    }
}

