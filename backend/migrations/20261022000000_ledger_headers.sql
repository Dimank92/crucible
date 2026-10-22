-- Migration: ledger_headers table for event indexer reorganization tracking (#1178)

CREATE TABLE IF NOT EXISTS ledger_headers (
    ledger_sequence   BIGINT      PRIMARY KEY,
    block_hash        TEXT        NOT NULL,
    parent_hash       TEXT        NOT NULL,
    indexed_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ledger_headers_parent_hash
    ON ledger_headers (parent_hash);
