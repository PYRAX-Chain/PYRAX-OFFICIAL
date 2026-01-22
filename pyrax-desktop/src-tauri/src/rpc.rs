//! RPC Client for real node connections
//!
//! Production-ready RPC client that connects to actual pyrax-node.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info};

/// RPC Client for connecting to pyrax-node
pub struct RpcClient {
    client: reqwest::Client,
    url: String,
}

impl RpcClient {
    /// Create a new RPC client
    /// PERFORMANCE FIX: Reduced timeouts to prevent UI freezing
    /// - Request timeout: 10s (was 30s)
    /// - Connect timeout: 3s (was 5s)
    pub fn new(url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            url: url.to_string(),
        }
    }

    /// Create client for default localhost
    pub fn localhost(port: u16) -> Self {
        Self::new(&format!("http://127.0.0.1:{}", port))
    }

    /// Send JSON-RPC request
    async fn request<P: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, RpcError> {
        let request = RpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
            id: 1,
        };

        debug!("RPC request: {}", method);

        let response = self.client
            .post(&self.url)
            .json(&request)
            .send()
            .await
            .map_err(|e| RpcError::Network(e.to_string()))?;

        let rpc_response: RpcResponse<R> = response
            .json()
            .await
            .map_err(|e| RpcError::Parse(e.to_string()))?;

        if let Some(error) = rpc_response.error {
            return Err(RpcError::Rpc {
                code: error.code,
                message: error.message,
            });
        }

        rpc_response.result.ok_or(RpcError::NoResult)
    }

    /// Check if node is reachable - uses simple health check first, falls back to chain info
    pub async fn is_connected(&self) -> bool {
        // Try simple health check first (no database access)
        if self.health_check().await.is_ok() {
            return true;
        }
        // Fall back to chain info check
        match self.get_block_number().await {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Simple health check that doesn't require database access
    pub async fn health_check(&self) -> Result<String, RpcError> {
        self.request("pyrax_health", ()).await
    }

    // ═══════════════════════════════════════════════════════════════
    // Chain Methods
    // ═══════════════════════════════════════════════════════════════

    /// Get current block number (uses chain info)
    pub async fn get_block_number(&self) -> Result<u64, RpcError> {
        let info: ChainInfoResponse = self.request("pyrax_getChainInfo", ()).await?;
        Ok(info.best_block_height)
    }

    /// Get chain info
    pub async fn get_chain_info(&self) -> Result<ChainInfoResponse, RpcError> {
        self.request("pyrax_getChainInfo", ()).await
    }

    /// Get block by number
    pub async fn get_block_by_number(&self, number: u64, full_txs: bool) -> Result<Option<BlockResponse>, RpcError> {
        self.request("pyrax_getBlockByNumber", (number, full_txs)).await
    }

    /// Get block by hash
    pub async fn get_block_by_hash(&self, hash: &str, full_txs: bool) -> Result<Option<BlockResponse>, RpcError> {
        self.request("pyrax_getBlockByHash", (hash, full_txs)).await
    }

    // ═══════════════════════════════════════════════════════════════
    // Account Methods
    // ═══════════════════════════════════════════════════════════════

    /// Get balance
    pub async fn get_balance(&self, address: &str) -> Result<u64, RpcError> {
        let result: BalanceResponse = self.request("pyrax_getBalance", (address,)).await?;
        Ok(result.balance)
    }

    /// Get transaction count (nonce) - not yet implemented in node
    pub async fn get_transaction_count(&self, address: &str) -> Result<u64, RpcError> {
        // PYRAX uses UTXO model, no nonce
        Ok(0)
    }

    /// Get UTXOs for address
    pub async fn get_utxos(&self, address: &str) -> Result<Vec<UtxoResponse>, RpcError> {
        self.request("pyrax_getUtxos", (address,)).await
    }

    // ═══════════════════════════════════════════════════════════════
    // Transaction Methods
    // ═══════════════════════════════════════════════════════════════

    /// Send raw transaction
    pub async fn send_raw_transaction(&self, tx_hex: &str) -> Result<String, RpcError> {
        let result: SubmitResponse = self.request("pyrax_sendRawTransaction", (tx_hex,)).await?;
        result.hash.ok_or(RpcError::NoResult)
    }

    /// Get transaction by hash
    pub async fn get_transaction(&self, hash: &str) -> Result<Option<TransactionResponse>, RpcError> {
        self.request("pyrax_getTransaction", (hash,)).await
    }

    /// Get transaction receipt - PYRAX uses UTXO, no receipts
    pub async fn get_transaction_receipt(&self, hash: &str) -> Result<Option<ReceiptResponse>, RpcError> {
        // UTXO model doesn't have receipts like account model
        Ok(None)
    }

    // ═══════════════════════════════════════════════════════════════
    // Network Methods
    // ═══════════════════════════════════════════════════════════════

    /// Get peer info
    pub async fn get_peers(&self) -> Result<Vec<PeerResponse>, RpcError> {
        self.request("pyrax_getPeers", ()).await
    }

    /// Get mempool info
    pub async fn get_mempool_info(&self) -> Result<MempoolResponse, RpcError> {
        self.request("pyrax_getMempoolInfo", ()).await
    }

    /// Get network info with extended P2P stats
    pub async fn get_network_info(&self) -> Result<NetworkInfoResponse, RpcError> {
        self.request("pyrax_getNetworkInfo", ()).await
    }

    /// Check if syncing
    pub async fn is_syncing(&self) -> Result<SyncingResponse, RpcError> {
        let info: ChainInfoResponse = self.request("pyrax_getChainInfo", ()).await?;
        Ok(SyncingResponse { syncing: info.syncing })
    }

    // ═══════════════════════════════════════════════════════════════
    // Mining Methods
    // ═══════════════════════════════════════════════════════════════

    /// Get mining info
    pub async fn get_mining_info(&self) -> Result<MiningInfoResponse, RpcError> {
        self.request("pyrax_getMiningInfo", ()).await
    }

    /// Get block template
    pub async fn get_block_template(&self) -> Result<BlockTemplateResponse, RpcError> {
        self.request("pyrax_getBlockTemplate", ()).await
    }

    /// Submit block
    pub async fn submit_block(&self, block_hex: &str) -> Result<bool, RpcError> {
        self.request("pyrax_submitBlock", (block_hex,)).await
    }

    // ═══════════════════════════════════════════════════════════════
    // Address History Methods
    // ═══════════════════════════════════════════════════════════════

    /// Get transaction history for an address
    pub async fn get_address_transactions(&self, address: &str, limit: Option<u32>) -> Result<AddressTransactionsResponse, RpcError> {
        self.request("pyrax_getAddressTransactions", (address, limit)).await
    }

    // ═══════════════════════════════════════════════════════════════
    // Explorer Helper Methods
    // ═══════════════════════════════════════════════════════════════

    /// Get peer count
    pub async fn get_peer_count(&self) -> Result<u64, RpcError> {
        let peers: Vec<PeerResponse> = self.get_peers().await.unwrap_or_default();
        Ok(peers.len() as u64)
    }

    /// Get pending transaction count from mempool
    pub async fn get_pending_transaction_count(&self) -> Result<u64, RpcError> {
        let mempool = self.get_mempool_info().await?;
        Ok(mempool.size as u64)
    }

    /// Get gas price (returns hex string for compatibility)
    pub async fn get_gas_price(&self) -> Result<String, RpcError> {
        // PYRAX uses fixed fee model, return a default
        Ok("0x3b9aca00".to_string()) // 1 gwei
    }

    /// Get contract code at address
    pub async fn get_code(&self, _address: &str) -> Result<String, RpcError> {
        // PYRAX UTXO model doesn't have contract storage
        Ok("0x".to_string())
    }

    /// Get balance as hex string
    pub async fn get_balance_hex(&self, address: &str) -> Result<String, RpcError> {
        let result: BalanceResponse = self.request("pyrax_getBalance", (address,)).await?;
        Ok(format!("0x{:x}", result.balance))
    }
}

fn parse_hex_u64(s: &str) -> Result<u64, RpcError> {
    let s = s.trim_start_matches("0x");
    u64::from_str_radix(s, 16).map_err(|e| RpcError::Parse(e.to_string()))
}

// ═══════════════════════════════════════════════════════════════
// Request/Response Types
// ═══════════════════════════════════════════════════════════════

#[derive(Serialize)]
struct RpcRequest<P: Serialize> {
    jsonrpc: &'static str,
    method: String,
    params: P,
    id: u64,
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    jsonrpc: String,
    result: Option<T>,
    error: Option<RpcErrorResponse>,
    id: u64,
}

#[derive(Deserialize)]
struct RpcErrorResponse {
    code: i32,
    message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("RPC error ({code}): {message}")]
    Rpc { code: i32, message: String },
    #[error("No result")]
    NoResult,
}

// ═══════════════════════════════════════════════════════════════
// Response Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChainInfoResponse {
    pub chain_id: u32,
    pub network: String,
    pub best_block_height: u64,
    pub best_block_hash: String,
    pub genesis_hash: String,
    pub difficulty: u64,
    pub utxo_count: u64,
    pub syncing: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BalanceResponse {
    pub address: String,
    pub balance: u64,
    pub utxo_count: u64,
    pub utxos: Vec<UtxoResponse>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SubmitResponse {
    pub accepted: bool,
    pub hash: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: String,
    pub miner: String,
    pub difficulty: String,
    pub total_difficulty: String,
    pub size: String,
    pub gas_used: String,
    pub gas_limit: String,
    pub transaction_count: u32,
    pub transactions: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    pub hash: String,
    pub nonce: String,
    pub block_hash: Option<String>,
    pub block_number: Option<String>,
    pub transaction_index: Option<String>,
    pub from: String,
    pub to: Option<String>,
    pub value: String,
    pub gas: String,
    pub gas_price: String,
    pub input: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptResponse {
    pub transaction_hash: String,
    pub block_hash: String,
    pub block_number: String,
    pub status: String,
    pub gas_used: String,
    pub cumulative_gas_used: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UtxoResponse {
    pub txid: String,
    pub vout: u32,
    pub value: u64,
    pub script_pubkey: String,
    pub height: u64,
    pub coinbase: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PeerResponse {
    pub peer_id: String,
    pub address: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub direction: String,
    pub connected_secs: u64,
    pub last_seen: u64,
    pub version: String,
    pub block_height: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MempoolResponse {
    pub size: u32,
    pub bytes: u64,
    pub pending_count: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SyncingResponse {
    pub syncing: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MiningInfoResponse {
    pub mining: bool,
    pub hashrate: f64,
    pub difficulty: f64,
    pub blocks_found: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlockTemplateResponse {
    pub previous_block_hash: String,
    pub height: u64,
    pub timestamp: u64,
    pub target: String,
    pub coinbase_value: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfoResponse {
    pub peer_count: usize,
    pub peers: Vec<PeerResponse>,
    pub local_peer_id: String,
    pub listen_addresses: Vec<String>,
    // Extended P2P stats for realtime connection monitoring
    pub inbound_peers: usize,
    pub outbound_peers: usize,
    pub target_peers: usize,
    pub max_peers: usize,
    pub dial_attempts: u64,
    pub dial_successes: u64,
    pub dial_failures: u64,
    pub average_rtt_ms: Option<u64>,
    pub network_state: String,
    pub nat_status: String,
    pub mesh_peers: usize,
    pub gossip_peers: usize,
    // Mesh topology for visualizer
    pub mesh_connections: Vec<MeshConnection>,
    pub relay_circuits: Vec<RelayCircuit>,
}

/// Mesh connection between two peers (for visualization)
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MeshConnection {
    pub peer_a: String,
    pub peer_b: String,
    pub topic: String,
    pub connection_type: String,
}

/// Active relay circuit through a node
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RelayCircuit {
    pub src_peer: String,
    pub dst_peer: String,
    pub established_at: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AddressTransactionsResponse {
    pub address: String,
    pub transactions: Vec<AddressTxResponse>,
    pub total_received: u64,
    pub total_sent: u64,
    pub tx_count: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AddressTxResponse {
    pub txid: String,
    pub block_hash: String,
    pub block_height: u64,
    pub tx_index: u32,
    pub direction: String,
    pub value: u64,
    pub timestamp: u64,
    pub is_coinbase: bool,
    pub confirmations: u64,
}
