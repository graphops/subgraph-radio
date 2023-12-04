CREATE TABLE IF NOT EXISTS local_attestations (
    identifier VARCHAR(255) NOT NULL,
    block_number INTEGER NOT NULL,
    ppoi VARCHAR(255) NOT NULL,
    stake_weight INTEGER NOT NULL,
    sender_group_hash VARCHAR(255) NOT NULL,
    UNIQUE (block_number, identifier, ppoi)
);

CREATE TABLE IF NOT EXISTS local_attestation_senders (
    attestation_block_number INTEGER NOT NULL,
    attestation_identifier VARCHAR(255) NOT NULL,
    attestation_ppoi VARCHAR(255) NOT NULL,
    sender VARCHAR(255) NOT NULL,
    FOREIGN KEY (attestation_block_number, attestation_identifier, attestation_ppoi) REFERENCES local_attestations(block_number, identifier, ppoi)
);

CREATE INDEX IF NOT EXISTS idx_senders_on_foreign_key ON local_attestation_senders(attestation_block_number, attestation_identifier, attestation_ppoi);

CREATE TABLE IF NOT EXISTS local_attestation_timestamps (
    attestation_block_number INTEGER NOT NULL,
    attestation_identifier VARCHAR(255) NOT NULL,
    attestation_ppoi VARCHAR(255) NOT NULL,
    timestamp BIGINT NOT NULL,
    FOREIGN KEY (attestation_block_number, attestation_identifier, attestation_ppoi) REFERENCES local_attestations(block_number, identifier, ppoi)
);

CREATE INDEX IF NOT EXISTS idx_timestamps_on_foreign_key ON local_attestation_timestamps(attestation_block_number, attestation_identifier, attestation_ppoi);

CREATE TABLE IF NOT EXISTS remote_ppoi_messages (
    identifier VARCHAR(255) NOT NULL,
    nonce BIGINT NOT NULL,
    graph_account VARCHAR(255) NOT NULL,
    content VARCHAR(255) NOT NULL,
    network VARCHAR(255) NOT NULL,
    block_number BIGINT NOT NULL,
    block_hash VARCHAR(255) NOT NULL,
    signature VARCHAR(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS upgrade_intent_messages (
    deployment VARCHAR(255) NOT NULL,
    nonce BIGINT NOT NULL,
    graph_account VARCHAR(255) NOT NULL,
    subgraph_id VARCHAR(255) NOT NULL PRIMARY KEY,
    new_hash VARCHAR(255) NOT NULL,
    signature VARCHAR(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS comparison_results (
    deployment VARCHAR(255) NOT NULL PRIMARY KEY,
    block_number BIGINT NOT NULL,
    result_type VARCHAR(255) NOT NULL,
    local_attestation_json TEXT NOT NULL,
    attestations_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    deployment VARCHAR(255) UNIQUE NOT NULL,
    message TEXT NOT NULL
);
