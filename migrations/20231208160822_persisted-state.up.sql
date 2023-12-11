CREATE TABLE IF NOT EXISTS local_attestations (
    identifier VARCHAR(255) NOT NULL,
    block_number VARCHAR(255) NOT NULL,
    ppoi VARCHAR(255) NOT NULL,
    stake_weight VARCHAR(255) NOT NULL,
    sender_group_hash VARCHAR(255) NOT NULL,
    senders TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    UNIQUE (block_number, identifier, ppoi)
);

CREATE TABLE IF NOT EXISTS remote_ppoi_messages (
    identifier VARCHAR(255) NOT NULL,
    nonce VARCHAR(255) NOT NULL,
    graph_account VARCHAR(255) NOT NULL,
    content VARCHAR(255) NOT NULL,
    network VARCHAR(255) NOT NULL,
    block_number VARCHAR(255) NOT NULL,
    block_hash VARCHAR(255) NOT NULL,
    signature VARCHAR(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS upgrade_intent_messages (
    deployment VARCHAR(255) NOT NULL,
    nonce VARCHAR(255) NOT NULL,
    graph_account VARCHAR(255) NOT NULL,
    subgraph_id VARCHAR(255) NOT NULL PRIMARY KEY,
    new_hash VARCHAR(255) NOT NULL,
    signature VARCHAR(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS comparison_results (
    deployment VARCHAR(255) NOT NULL PRIMARY KEY,
    block_number VARCHAR(255) NOT NULL,
    result_type VARCHAR(255) NOT NULL,
    local_attestation_json TEXT NOT NULL,
    attestations_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    deployment VARCHAR(255) UNIQUE NOT NULL,
    message TEXT NOT NULL
);
