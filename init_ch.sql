CREATE DATABASE IF NOT EXISTS solana_indexer;

CREATE TABLE IF NOT EXISTS solana_indexer.transactions_history
(
    signature        String,
    slot             UInt64,
    block_time       Int64,
    program_id       String,
    signer           String,
    instruction_name LowCardinality(String),
    instruction_args String,
    accounts         Array(String),
    transaction_hash String,
    created_at       DateTime DEFAULT now()
)
ENGINE = MergeTree()
ORDER BY (program_id, slot, signature)
PARTITION BY toYYYYMM(toDateTime(block_time))
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS solana_indexer.instruction_stats_buffer
(
    instruction_name LowCardinality(String),
    date             Date,
    total_count      UInt64
)
ENGINE = SummingMergeTree()
ORDER BY (instruction_name, date);

CREATE MATERIALIZED VIEW IF NOT EXISTS solana_indexer.mv_instruction_stats
TO solana_indexer.instruction_stats_buffer
AS SELECT
    instruction_name,
    toDate(toDateTime(block_time)) AS date,
    count()                         AS total_count
FROM solana_indexer.transactions_history
GROUP BY instruction_name, date;

CREATE TABLE IF NOT EXISTS solana_indexer.signer_stats_buffer
(
    signer      String,
    date        Date,
    tx_count    UInt64
)
ENGINE = SummingMergeTree()
ORDER BY (signer, date);

CREATE MATERIALIZED VIEW IF NOT EXISTS solana_indexer.mv_signer_stats
TO solana_indexer.signer_stats_buffer
AS SELECT
    signer,
    toDate(toDateTime(block_time)) AS date,
    count()                         AS tx_count
FROM solana_indexer.transactions_history
GROUP BY signer, date;

CREATE TABLE IF NOT EXISTS solana_indexer.slot_stats_buffer
(
    program_id  String,
    date        Date,
    slot_count  UInt64,
    tx_count    UInt64
)
ENGINE = SummingMergeTree()
ORDER BY (program_id, date);

CREATE MATERIALIZED VIEW IF NOT EXISTS solana_indexer.mv_slot_stats
TO solana_indexer.slot_stats_buffer
AS SELECT
    program_id,
    toDate(toDateTime(block_time)) AS date,
    uniq(slot)                      AS slot_count,
    count()                         AS tx_count
FROM solana_indexer.transactions_history
GROUP BY program_id, date;