CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE IF NOT EXISTS transactions (
    signature       VARCHAR(88) PRIMARY KEY,
    slot            BIGINT NOT NULL,
    block_time      BIGINT,
    program_id      VARCHAR(44) NOT NULL,
    signer          VARCHAR(44) NOT NULL,
    instruction_name VARCHAR(255) NOT NULL,
    instruction_args JSONB,
    accounts        JSONB,
    raw_transaction JSONB,
    created_at      TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_transactions_signer
    ON transactions(signer);

CREATE INDEX IF NOT EXISTS idx_transactions_instruction
    ON transactions(instruction_name);

CREATE INDEX IF NOT EXISTS idx_transactions_slot
    ON transactions(slot);

CREATE INDEX IF NOT EXISTS idx_transactions_created
    ON transactions(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_transactions_program_id
    ON transactions(program_id);

CREATE INDEX IF NOT EXISTS idx_transactions_block_time
    ON transactions(block_time DESC);

CREATE TABLE IF NOT EXISTS checkpoints (
    key     TEXT PRIMARY KEY,
    value   BIGINT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS dead_letters (
    id              BIGSERIAL PRIMARY KEY,
    signature       VARCHAR(88),
    slot            BIGINT,
    error_message   TEXT NOT NULL,
    raw_data        TEXT,
    created_at      TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS program_registry (
    program_id      VARCHAR(44) PRIMARY KEY,
    name            TEXT,
    idl_path        TEXT,
    indexed_since   TIMESTAMPTZ DEFAULT NOW(),
    last_slot       BIGINT DEFAULT 0
);

INSERT INTO checkpoints (key, value) VALUES ('last_indexed_slot', 0)
ON CONFLICT (key) DO NOTHING;