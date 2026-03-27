CREATE TABLE IF NOT EXISTS transactions_history (
    signature String,
    slot UInt64,
    block_time Int64,
    program_id String,
    signer String,
    instruction_name LowCardinality(String),
    instruction_args String,
    accounts Array(String)
) ENGINE = MergeTree()
ORDER BY (program_id, slot, signature);
