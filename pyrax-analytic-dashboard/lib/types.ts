export interface ChainStatus {
    head_block: number;
    lowest_block: number;
    height_delta: number;
    block_rate: number;
    avg_latency_ms: number;
    stalled: boolean;
    fork_detected: boolean;
    discovered_nodes: number;
    online_nodes: number;
    network_hashrate: number;  // Kept for compatibility even if 0
    difficulty: number;        // Kept for compatibility even if 0
    total_blocks_found: number;// Kept for compatibility even if 0
}

export interface NodeInfo {
    endpoint: string;
    block_height: number;
    reachable: boolean;
    syncing: boolean;
    peer_count: number;
    latency_ms: number;
}

export interface StatusResponse {
    chain: ChainStatus;
    nodes: NodeInfo[];
    alerts: {
        stalled: boolean;
        fork_detected: boolean;
        nodes_unreachable: number;
    };
}
