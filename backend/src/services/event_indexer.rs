//! Contract event indexing service.
//!
//! Indexes Soroban contract events from the Stellar network into a queryable
//! local store backed by PostgreSQL, with Redis caching for hot queries.
//! Includes blockchain reorganization reconciliation to purge orphaned events
//! on block hash mismatch.

use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

/// A single indexed contract event.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct IndexedEvent {
    pub id: Uuid,
    pub contract_id: String,
    pub ledger_sequence: i64,
    pub transaction_hash: String,
    pub event_index: i32,
    pub event_type: String,
    pub topics: serde_json::Value,
    pub data: serde_json::Value,
    pub indexed_at: DateTime<Utc>,
}

/// Query parameters for filtering indexed events.
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub contract_id: Option<String>,
    pub event_type: Option<String>,
    pub from_ledger: Option<i64>,
    pub to_ledger: Option<i64>,
    pub limit: Option<i64>,
}

/// A record representing an indexed ledger header for chain continuity and re-org tracking.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
pub struct LedgerHeaderRecord {
    pub ledger_sequence: i64,
    pub block_hash: String,
    pub parent_hash: String,
    pub indexed_at: DateTime<Utc>,
}

impl LedgerHeaderRecord {
    pub fn new(ledger_sequence: i64, block_hash: impl Into<String>, parent_hash: impl Into<String>) -> Self {
        Self {
            ledger_sequence,
            block_hash: block_hash.into(),
            parent_hash: parent_hash.into(),
            indexed_at: Utc::now(),
        }
    }
}

/// Outcome report of a blockchain reorganization detection and rollback reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReorgReport {
    pub detected: bool,
    pub fork_point_ledger: Option<i64>,
    pub common_ancestor_ledger: Option<i64>,
    pub reverted_ledgers: usize,
    pub purged_events: u64,
}

/// Result of indexing a ledger block and its events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngestBlockOutcome {
    pub ledger_sequence: i64,
    pub block_hash: String,
    pub events_indexed: usize,
    pub reorg: Option<ReorgReport>,
}

/// Locates the common ancestor ledger sequence between an indexed chain and a new candidate branch.
///
/// Compares candidate ancestor headers against existing chain headers,
/// identifying the highest ledger sequence where block hashes match.
pub fn locate_common_ancestor(
    existing_chain: &[LedgerHeaderRecord],
    new_branch: &[LedgerHeaderRecord],
) -> Option<i64> {
    let mut lookup: HashMap<i64, &str> = HashMap::with_capacity(existing_chain.len());
    for h in existing_chain {
        lookup.insert(h.ledger_sequence, &h.block_hash);
    }

    let mut common_ancestor: Option<i64> = None;
    for cand in new_branch {
        if let Some(&stored_hash) = lookup.get(&cand.ledger_sequence) {
            if stored_hash == cand.block_hash {
                if common_ancestor.map_or(true, |best| cand.ledger_sequence > best) {
                    common_ancestor = Some(cand.ledger_sequence);
                }
            }
        }
    }
    common_ancestor
}

/// Service responsible for ingesting and querying contract events.
pub struct EventIndexer {
    db: PgPool,
}

impl EventIndexer {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Ensure the `ledger_headers` table exists for tracking block hashes and re-orgs.
    #[instrument(skip(self))]
    pub async fn ensure_ledger_headers_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ledger_headers (
                ledger_sequence   BIGINT      PRIMARY KEY,
                block_hash        TEXT        NOT NULL,
                parent_hash       TEXT        NOT NULL,
                indexed_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX IF NOT EXISTS idx_ledger_headers_parent_hash
                ON ledger_headers (parent_hash);
            "#,
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Ingest a batch of raw events from the Stellar network.
    #[instrument(skip(self, events), fields(count = events.len()))]
    pub async fn ingest(&self, events: Vec<RawEvent>) -> Result<usize, sqlx::Error> {
        if events.is_empty() {
            return Ok(0);
        }

        let mut inserted = 0;
        for event in events {
            let result = sqlx::query(
                r#"
                INSERT INTO contract_events
                    (id, contract_id, ledger_sequence, transaction_hash, event_index, event_type, topics, data, indexed_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (transaction_hash, event_index) DO NOTHING
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(&event.contract_id)
            .bind(event.ledger_sequence)
            .bind(&event.transaction_hash)
            .bind(event.event_index)
            .bind(&event.event_type)
            .bind(&event.topics)
            .bind(&event.data)
            .bind(Utc::now())
            .execute(&self.db)
            .await?;

            if result.rows_affected() > 0 {
                inserted += 1;
                debug!(contract_id = %event.contract_id, event_type = %event.event_type, "Indexed event");
            }
        }

        info!(inserted, "Event ingestion complete");
        Ok(inserted)
    }

    /// Query indexed events with optional filters.
    #[instrument(skip(self))]
    pub async fn query(&self, q: EventQuery) -> Result<Vec<IndexedEvent>, sqlx::Error> {
        let limit = q.limit.unwrap_or(100).min(1000);

        let rows = sqlx::query_as::<_, IndexedEvent>(
            r#"
                 SELECT id, contract_id, ledger_sequence, transaction_hash, event_index,
                   event_type, topics, data, indexed_at
            FROM contract_events
            WHERE ($1::text IS NULL OR contract_id = $1)
              AND ($2::text IS NULL OR event_type  = $2)
              AND ($3::bigint IS NULL OR ledger_sequence >= $3)
              AND ($4::bigint IS NULL OR ledger_sequence <= $4)
            ORDER BY ledger_sequence DESC, indexed_at DESC
            LIMIT $5
            "#,
        )
        .bind(q.contract_id)
        .bind(q.event_type)
        .bind(q.from_ledger)
        .bind(q.to_ledger)
        .bind(limit)
        .fetch_all(&self.db)
        .await?;

        Ok(rows)
    }

    /// Return the highest ledger sequence that has been indexed.
    pub async fn latest_ledger(&self) -> Result<Option<i64>, sqlx::Error> {
        let row: Option<(Option<i64>,)> =
            sqlx::query_as("SELECT MAX(ledger_sequence) FROM contract_events")
                .fetch_optional(&self.db)
                .await?;
        Ok(row.and_then(|(v,)| v))
    }

    /// Fetch the latest indexed ledger header.
    #[instrument(skip(self))]
    pub async fn latest_ledger_header(&self) -> Result<Option<LedgerHeaderRecord>, sqlx::Error> {
        let row = sqlx::query_as::<_, LedgerHeaderRecord>(
            r#"
            SELECT ledger_sequence, block_hash, parent_hash, indexed_at
            FROM ledger_headers
            ORDER BY ledger_sequence DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.db)
        .await?;
        Ok(row)
    }

    /// Fetch a ledger header by sequence number.
    #[instrument(skip(self))]
    pub async fn get_ledger_header(&self, ledger_sequence: i64) -> Result<Option<LedgerHeaderRecord>, sqlx::Error> {
        let row = sqlx::query_as::<_, LedgerHeaderRecord>(
            r#"
            SELECT ledger_sequence, block_hash, parent_hash, indexed_at
            FROM ledger_headers
            WHERE ledger_sequence = $1
            "#,
        )
        .bind(ledger_sequence)
        .fetch_optional(&self.db)
        .await?;
        Ok(row)
    }

    /// Fetch the most recent ledger headers up to `limit`.
    #[instrument(skip(self))]
    pub async fn fetch_recent_headers(&self, limit: i64) -> Result<Vec<LedgerHeaderRecord>, sqlx::Error> {
        let limit = limit.clamp(1, 1000);
        let rows = sqlx::query_as::<_, LedgerHeaderRecord>(
            r#"
            SELECT ledger_sequence, block_hash, parent_hash, indexed_at
            FROM ledger_headers
            ORDER BY ledger_sequence DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.db)
        .await?;
        Ok(rows)
    }

    /// Record a ledger header in the database.
    #[instrument(skip(self, header))]
    pub async fn record_ledger_header(&self, header: &LedgerHeaderRecord) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO ledger_headers (ledger_sequence, block_hash, parent_hash, indexed_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (ledger_sequence) DO UPDATE
            SET block_hash = EXCLUDED.block_hash,
                parent_hash = EXCLUDED.parent_hash,
                indexed_at = EXCLUDED.indexed_at
            "#,
        )
        .bind(header.ledger_sequence)
        .bind(&header.block_hash)
        .bind(&header.parent_hash)
        .bind(header.indexed_at)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Reconcile a blockchain reorganization by rolling back all indexed events and ledger headers
    /// strictly higher than `common_ancestor_ledger` in an atomic database transaction.
    #[instrument(skip(self))]
    pub async fn reconcile_reorganization(&self, common_ancestor_ledger: i64) -> Result<ReorgReport, sqlx::Error> {
        let mut tx = self.db.begin().await?;

        // 1. Determine highest ledger sequence among events before rollback
        let max_ledger: Option<(Option<i64>,)> = sqlx::query_as(
            "SELECT MAX(ledger_sequence) FROM contract_events WHERE ledger_sequence > $1"
        )
        .bind(common_ancestor_ledger)
        .fetch_optional(&mut *tx)
        .await?;

        let tip_before = max_ledger.and_then(|(v,)| v).unwrap_or(common_ancestor_ledger);

        // 2. Purge orphaned contract events strictly after common ancestor
        let delete_events_res = sqlx::query(
            "DELETE FROM contract_events WHERE ledger_sequence > $1"
        )
        .bind(common_ancestor_ledger)
        .execute(&mut *tx)
        .await?;
        let purged_events = delete_events_res.rows_affected();

        // 3. Purge orphaned ledger headers strictly after common ancestor
        let _ = sqlx::query(
            "DELETE FROM ledger_headers WHERE ledger_sequence > $1"
        )
        .bind(common_ancestor_ledger)
        .execute(&mut *tx)
        .await;

        // 4. Commit atomic transaction
        tx.commit().await?;

        let reverted_ledgers = (tip_before.saturating_sub(common_ancestor_ledger)) as usize;

        warn!(
            common_ancestor = common_ancestor_ledger,
            reverted_ledgers,
            purged_events,
            "Reconciled blockchain reorganization: rolled back orphaned events and headers"
        );

        Ok(ReorgReport {
            detected: true,
            fork_point_ledger: Some(common_ancestor_ledger + 1),
            common_ancestor_ledger: Some(common_ancestor_ledger),
            reverted_ledgers,
            purged_events,
        })
    }

    /// Ingest a ledger block with header and events, automatically reconciling re-orgs on block hash mismatch.
    #[instrument(skip(self, events, ancestor_chain))]
    pub async fn ingest_ledger_block(
        &self,
        header: LedgerHeaderRecord,
        events: Vec<RawEvent>,
        ancestor_chain: &[LedgerHeaderRecord],
    ) -> Result<IngestBlockOutcome, sqlx::Error> {
        let _ = self.ensure_ledger_headers_table().await;

        let latest = self.latest_ledger_header().await?;
        let mut reorg_report = None;

        if let Some(prev) = latest {
            let hash_mismatch = prev.block_hash != header.parent_hash;
            let sequence_divergence = header.ledger_sequence <= prev.ledger_sequence;

            if hash_mismatch || sequence_divergence {
                warn!(
                    prev_seq = prev.ledger_sequence,
                    prev_hash = %prev.block_hash,
                    incoming_seq = header.ledger_sequence,
                    incoming_parent = %header.parent_hash,
                    "Reorganization detected on block hash mismatch: reconciling orphaned ledgers"
                );

                let recent_headers = self.fetch_recent_headers(100).await.unwrap_or_default();
                let common_ancestor = if !ancestor_chain.is_empty() {
                    locate_common_ancestor(&recent_headers, ancestor_chain)
                } else {
                    None
                }
                .or_else(|| {
                    recent_headers.iter()
                        .find(|h| h.block_hash == header.parent_hash)
                        .map(|h| h.ledger_sequence)
                })
                .unwrap_or(header.ledger_sequence.saturating_sub(1));

                let report = self.reconcile_reorganization(common_ancestor).await?;
                reorg_report = Some(report);
            }
        }

        // Atomically insert header and events
        let mut tx = self.db.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO ledger_headers (ledger_sequence, block_hash, parent_hash, indexed_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (ledger_sequence) DO UPDATE
            SET block_hash = EXCLUDED.block_hash,
                parent_hash = EXCLUDED.parent_hash,
                indexed_at = EXCLUDED.indexed_at
            "#,
        )
        .bind(header.ledger_sequence)
        .bind(&header.block_hash)
        .bind(&header.parent_hash)
        .bind(header.indexed_at)
        .execute(&mut *tx)
        .await?;

        let mut inserted = 0;
        for event in events {
            let res = sqlx::query(
                r#"
                INSERT INTO contract_events
                    (id, contract_id, ledger_sequence, transaction_hash, event_index, event_type, topics, data, indexed_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (transaction_hash, event_index) DO NOTHING
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(&event.contract_id)
            .bind(event.ledger_sequence)
            .bind(&event.transaction_hash)
            .bind(event.event_index)
            .bind(&event.event_type)
            .bind(&event.topics)
            .bind(&event.data)
            .bind(Utc::now())
            .execute(&mut *tx)
            .await?;

            if res.rows_affected() > 0 {
                inserted += 1;
            }
        }

        tx.commit().await?;

        Ok(IngestBlockOutcome {
            ledger_sequence: header.ledger_sequence,
            block_hash: header.block_hash,
            events_indexed: inserted,
            reorg: reorg_report,
        })
    }

    /// Delete events older than `keep_ledgers` ledgers behind the latest.
    #[instrument(skip(self))]
    pub async fn prune(&self, keep_ledgers: i64) -> Result<u64, sqlx::Error> {
        let latest = self.latest_ledger().await?.unwrap_or(0);
        let cutoff = latest.saturating_sub(keep_ledgers);
        let result = sqlx::query("DELETE FROM contract_events WHERE ledger_sequence < $1")
            .bind(cutoff)
            .execute(&self.db)
            .await?;
        let deleted = result.rows_affected();
        if deleted > 0 {
            warn!(deleted, cutoff, "Pruned old contract events");
        }
        Ok(deleted)
    }
}

/// A raw event as received from the Stellar network / horizon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub contract_id: String,
    pub ledger_sequence: i64,
    pub transaction_hash: String,
    pub event_index: i32,
    pub event_type: String,
    pub topics: serde_json::Value,
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn lazy_pool() -> PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://postgres:postgres@localhost/crucible_test")
            .unwrap()
    }

    fn make_event(contract_id: &str, ledger: i64, tx: &str, kind: &str) -> RawEvent {
        RawEvent {
            contract_id: contract_id.to_string(),
            ledger_sequence: ledger,
            transaction_hash: tx.to_string(),
            event_index: 0,
            event_type: kind.to_string(),
            topics: serde_json::json!(["transfer"]),
            data: serde_json::json!({"amount": 100}),
        }
    }

    #[tokio::test]
    async fn test_ingest_empty_batch_returns_zero() {
        let indexer = EventIndexer::new(lazy_pool());
        let result = indexer.ingest(vec![]).await.unwrap();
        assert_eq!(result, 0);
    }

    #[tokio::test]
    async fn test_event_query_defaults() {
        let q = EventQuery::default();
        assert!(q.contract_id.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn test_raw_event_serialization() {
        let ev = make_event("CABC", 100, "txhash1", "transfer");
        let json = serde_json::to_string(&ev).unwrap();
        let back: RawEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.contract_id, "CABC");
        assert_eq!(back.ledger_sequence, 100);
        assert_eq!(back.event_index, 0);
    }

    #[test]
    fn test_ledger_header_record_creation() {
        let header = LedgerHeaderRecord::new(100, "block_hash_100", "parent_hash_99");
        assert_eq!(header.ledger_sequence, 100);
        assert_eq!(header.block_hash, "block_hash_100");
        assert_eq!(header.parent_hash, "parent_hash_99");
    }

    #[test]
    fn test_locate_common_ancestor_fork_detected() {
        // Main chain: 100 -> 101A -> 102A -> 103A
        let existing = vec![
            LedgerHeaderRecord::new(100, "hash_100", "hash_99"),
            LedgerHeaderRecord::new(101, "hash_101_A", "hash_100"),
            LedgerHeaderRecord::new(102, "hash_102_A", "hash_101_A"),
            LedgerHeaderRecord::new(103, "hash_103_A", "hash_102_A"),
        ];

        // New branch diverged after ledger 101: 100 -> 101A -> 102B -> 103B
        let candidate_branch = vec![
            LedgerHeaderRecord::new(100, "hash_100", "hash_99"),
            LedgerHeaderRecord::new(101, "hash_101_A", "hash_100"),
            LedgerHeaderRecord::new(102, "hash_102_B", "hash_101_A"),
            LedgerHeaderRecord::new(103, "hash_103_B", "hash_102_B"),
        ];

        let ancestor = locate_common_ancestor(&existing, &candidate_branch);
        assert_eq!(ancestor, Some(101));
    }

    #[test]
    fn test_locate_common_ancestor_deep_reorg() {
        // Main chain diverged at ledger 50
        let mut existing = Vec::new();
        let mut new_branch = Vec::new();

        for i in 1..=100 {
            let parent = format!("hash_{}", i - 1);
            if i <= 50 {
                existing.push(LedgerHeaderRecord::new(i, format!("hash_{}", i), parent.clone()));
                new_branch.push(LedgerHeaderRecord::new(i, format!("hash_{}", i), parent));
            } else {
                existing.push(LedgerHeaderRecord::new(i, format!("hash_{}_old", i), format!("hash_{}_old", i - 1)));
                new_branch.push(LedgerHeaderRecord::new(i, format!("hash_{}_new", i), format!("hash_{}_new", i - 1)));
            }
        }

        let ancestor = locate_common_ancestor(&existing, &new_branch);
        assert_eq!(ancestor, Some(50));
    }

    #[test]
    fn test_locate_common_ancestor_no_overlap() {
        let existing = vec![
            LedgerHeaderRecord::new(10, "hash_10", "hash_9"),
        ];
        let new_branch = vec![
            LedgerHeaderRecord::new(20, "hash_20", "hash_19"),
        ];
        assert_eq!(locate_common_ancestor(&existing, &new_branch), None);
    }

    #[test]
    fn test_locate_common_ancestor_identical_chains() {
        let chain = vec![
            LedgerHeaderRecord::new(1, "h1", "h0"),
            LedgerHeaderRecord::new(2, "h2", "h1"),
            LedgerHeaderRecord::new(3, "h3", "h2"),
        ];
        assert_eq!(locate_common_ancestor(&chain, &chain), Some(3));
    }

    #[test]
    fn test_reorg_report_serialization() {
        let report = ReorgReport {
            detected: true,
            fork_point_ledger: Some(102),
            common_ancestor_ledger: Some(101),
            reverted_ledgers: 2,
            purged_events: 15,
        };
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: ReorgReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, report);
    }

    #[test]
    fn test_ingest_block_outcome_structure() {
        let outcome = IngestBlockOutcome {
            ledger_sequence: 105,
            block_hash: "hash105".to_string(),
            events_indexed: 3,
            reorg: Some(ReorgReport {
                detected: true,
                fork_point_ledger: Some(104),
                common_ancestor_ledger: Some(103),
                reverted_ledgers: 1,
                purged_events: 5,
            }),
        };
        assert_eq!(outcome.ledger_sequence, 105);
        assert_eq!(outcome.events_indexed, 3);
        assert!(outcome.reorg.unwrap().detected);
    }
}
