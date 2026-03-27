CREATE TABLE IF NOT EXISTS transactions (
    signature VARCHAR(88) PRIMARY KEY,
    slot BIGINT NOT NULL,
    block_time BIGINT,
    program_id VARCHAR(44) NOT NULL,
    signer VARCHAR(44) NOT NULL,
    instruction_name VARCHAR(255) NOT NULL,
    instruction_args JSONB,
    accounts JSONB,
    raw_transaction JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_transactions_signer ON transactions(signer);
CREATE INDEX IF NOT EXISTS idx_transactions_instruction ON transactions(instruction_name);
CREATE INDEX IF NOT EXISTS idx_transactions_slot ON transactions(slot);
CREATE INDEX IF NOT EXISTS idx_transactions_created ON transactions(created_at);

CREATE TABLE IF NOT EXISTS checkpoints (
    key TEXT PRIMARY KEY,
    value BIGINT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);