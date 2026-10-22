-- Migration: Add contract_rollouts table for blue-green / staged deployment tracking and rollback (#1175)
CREATE TABLE IF NOT EXISTS contract_rollouts (
    id VARCHAR(64) PRIMARY KEY,
    contract_id VARCHAR(128) NOT NULL,
    blue_version VARCHAR(32) NOT NULL,
    green_version VARCHAR(32) NOT NULL,
    green_wasm_hash VARCHAR(64),
    active_version VARCHAR(32),
    status VARCHAR(32) NOT NULL,
    rollback_executed BOOLEAN DEFAULT FALSE,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_contract_rollouts_contract_id ON contract_rollouts(contract_id);
CREATE INDEX IF NOT EXISTS idx_contract_rollouts_status ON contract_rollouts(status);
