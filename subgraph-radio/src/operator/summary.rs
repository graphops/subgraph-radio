use std::collections::HashMap;

pub struct Summary {
    pub deployments: Vec<String>,
    pub average_message_frequency: u32,
    pub average_processing_time: u32,
    pub diverged_count: u32,
    pub frequent_senders: Vec<String>,
    pub divergence_rate: u32,
    pub latest_message_timestamp: u64,
    pub consensus_ppoi_stake: HashMap<String, (String, u64)>,
    pub average_gossip_peers: u32,
}
