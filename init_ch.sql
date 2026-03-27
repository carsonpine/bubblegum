CREATE TABLE IF NOT EXISTS transactions_history (
    signature String,
    slot UInt64,
    block_time Int64,
    program_id String,
    signer String,
    instruction_name LowCardinality(String),
    instruction_args String,
    accounts Array(String),
    transaction_hash String
) ENGINE = MergeTree()
ORDER BY (program_id, slot, signature);

CREATE MATERIALIZED VIEW IF NOT EXISTS instruction_stats_buffer
ENGINE = SummingMergeTree()
ORDER BY (instruction_name, date)
AS SELECT
    instruction_name,
    toStartOfDay(toDateTime(block_time)) AS date,
    count() AS total_count
FROM transactions_history
GROUP BY instruction_name, date;