//! JSON-RPC Server for PYRAX
//!
//! Production-ready RPC server using jsonrpsee

use std::sync::Arc;
use std::net::SocketAddr;
use jsonrpsee::server::{ServerBuilder, ServerHandle};
use jsonrpsee::core::{async_trait, RpcResult};
use jsonrpsee::proc_macros::rpc;
use tracing::info;

use super::{RpcError, RpcBlock, RpcTransaction, RpcChainInfo, RpcPeerInfo, RpcMempoolInfo, RpcBlockTemplate, RpcSubmitResult, RpcBalance, RpcUtxo, RpcAddressTransactions, RpcAddressTx, RpcMiningInfo};
use crate::storage::ChainDB;
use crate::types::{H256, Address, Transaction, TxInput, TxOutput, Block, NetworkId, OutPoint};
use crate::mempool::Mempool;

/// PYRAX JSON-RPC API
#[rpc(server)]
pub trait PyraxRpc {
    /// Get chain ID
    #[method(name = "pyrax_chainId")]
    async fn chain_id(&self) -> RpcResult<u32>;

    /// Get chain info including tip, height, UTXO count
    #[method(name = "pyrax_getChainInfo")]
    async fn get_chain_info(&self) -> RpcResult<RpcChainInfo>;

    /// Get block by hash
    #[method(name = "pyrax_getBlockByHash")]
    async fn get_block_by_hash(&self, hash: String, full_txs: bool) -> RpcResult<Option<RpcBlock>>;

    /// Get block by height
    #[method(name = "pyrax_getBlockByNumber")]
    async fn get_block_by_number(&self, height: u64, full_txs: bool) -> RpcResult<Option<RpcBlock>>;

    /// Get transaction by txid
    #[method(name = "pyrax_getTransaction")]
    async fn get_transaction(&self, txid: String) -> RpcResult<Option<RpcTransaction>>;

    /// Get UTXO balance for an address (script pubkey hash)
    #[method(name = "pyrax_getBalance")]
    async fn get_balance(&self, address: String) -> RpcResult<RpcBalance>;

    /// Get UTXOs for an address
    #[method(name = "pyrax_getUtxos")]
    async fn get_utxos(&self, address: String) -> RpcResult<Vec<RpcUtxo>>;

    /// Send raw transaction (hex encoded)
    #[method(name = "pyrax_sendRawTransaction")]
    async fn send_raw_transaction(&self, tx_hex: String) -> RpcResult<RpcSubmitResult>;

    /// Get mempool info
    #[method(name = "pyrax_getMempoolInfo")]
    async fn get_mempool_info(&self) -> RpcResult<RpcMempoolInfo>;

    /// Get block template for mining
    #[method(name = "pyrax_getBlockTemplate")]
    async fn get_block_template(&self) -> RpcResult<RpcBlockTemplate>;

    /// Submit mined block
    #[method(name = "pyrax_submitBlock")]
    async fn submit_block(&self, block_hex: String) -> RpcResult<RpcSubmitResult>;

    /// Get mining info (for desktop app compatibility)
    #[method(name = "pyrax_getMiningInfo")]
    async fn get_mining_info(&self) -> RpcResult<RpcMiningInfo>;
    
    /// Create a test transaction (devnet only) - spends from one address to another
    #[method(name = "pyrax_createTestTransaction")]
    async fn create_test_transaction(&self, from_address: String, to_address: String, amount: u64) -> RpcResult<RpcSubmitResult>;

    /// Get network peer information
    #[method(name = "pyrax_getNetworkInfo")]
    async fn get_network_info(&self) -> RpcResult<super::RpcNetworkInfo>;

    /// Get peer list (for desktop app compatibility)
    #[method(name = "pyrax_getPeers")]
    async fn get_peers(&self) -> RpcResult<Vec<RpcPeerInfo>>;

    /// Simple health check - returns immediately without database access
    #[method(name = "pyrax_health")]
    async fn health(&self) -> RpcResult<String>;

    /// Get local peer ID for P2P bootstrap discovery
    /// Desktop/CLI apps use this to dynamically discover bootnode peer IDs
    #[method(name = "pyrax_getPeerId")]
    async fn get_peer_id(&self) -> RpcResult<String>;

    /// Debug P2P state - shows both registry peers and metrics for troubleshooting
    /// Use this to diagnose peer count mismatches
    #[method(name = "pyrax_debugP2PState")]
    async fn debug_p2p_state(&self) -> RpcResult<super::RpcP2PDebugState>;

    /// Get transaction history for an address
    #[method(name = "pyrax_getAddressTransactions")]
    async fn get_address_transactions(&self, address: String, limit: Option<u32>) -> RpcResult<super::RpcAddressTransactions>;
}

/// RPC server state
pub struct RpcServerImpl {
    db: Arc<ChainDB>,
    network_id: NetworkId,
    mempool: Option<Arc<Mempool>>,
    peer_registry: Option<crate::p2p::PeerRegistry>,
}

impl RpcServerImpl {
    pub fn new(db: Arc<ChainDB>, network_id: NetworkId) -> Self {
        Self { db, network_id, mempool: None, peer_registry: None }
    }
    
    pub fn with_mempool(db: Arc<ChainDB>, network_id: NetworkId, mempool: Arc<Mempool>) -> Self {
        Self { db, network_id, mempool: Some(mempool), peer_registry: None }
    }

    pub fn with_mempool_and_peers(
        db: Arc<ChainDB>,
        network_id: NetworkId,
        mempool: Arc<Mempool>,
        peer_registry: crate::p2p::PeerRegistry,
    ) -> Self {
        Self { db, network_id, mempool: Some(mempool), peer_registry: Some(peer_registry) }
    }
}

#[async_trait]
impl PyraxRpcServer for RpcServerImpl {
    async fn chain_id(&self) -> RpcResult<u32> {
        Ok(self.network_id.0)
    }

    async fn get_chain_info(&self) -> RpcResult<RpcChainInfo> {
        let tip = self.db.get_tip();
        let utxo_count = self.db.utxo_count().unwrap_or(0);

        Ok(RpcChainInfo {
            chain_id: self.network_id.0,
            network: self.network_id.name().to_string(),
            best_block_hash: format!("0x{}", hex::encode(&tip.hash.0)),
            best_block_height: tip.height,
            genesis_hash: self.db.genesis_hash()
                .map(|h| format!("0x{}", hex::encode(&h.0)))
                .unwrap_or_else(|_| "0x0".to_string()),
            difficulty: tip.total_difficulty,
            utxo_count,
            syncing: false,
            node_version: format!("pyrax-node/{}", env!("CARGO_PKG_VERSION")),
        })
    }

    async fn get_block_by_hash(&self, hash: String, full_txs: bool) -> RpcResult<Option<RpcBlock>> {
        let hash = parse_hash(&hash)?;
        
        match self.db.get_block(&hash) {
            Ok(Some(block)) => {
                let mut rpc_block = RpcBlock::from_block(&block);
                if full_txs {
                    rpc_block = rpc_block.with_transactions(&block);
                }
                Ok(Some(rpc_block))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(RpcError::InternalError(e.to_string()).into()),
        }
    }

    async fn get_block_by_number(&self, height: u64, full_txs: bool) -> RpcResult<Option<RpcBlock>> {
        match self.db.get_block_by_height(height) {
            Ok(Some(block)) => {
                let mut rpc_block = RpcBlock::from_block(&block);
                if full_txs {
                    rpc_block = rpc_block.with_transactions(&block);
                }
                Ok(Some(rpc_block))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(RpcError::InternalError(e.to_string()).into()),
        }
    }

    async fn get_transaction(&self, txid: String) -> RpcResult<Option<RpcTransaction>> {
        let txid = parse_hash(&txid)?;
        
        match self.db.get_transaction(&txid) {
            Ok(Some(tx)) => {
                Ok(Some(RpcTransaction::from_tx(&tx, None, None, None)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(RpcError::InternalError(e.to_string()).into()),
        }
    }

    async fn get_balance(&self, address: String) -> RpcResult<RpcBalance> {
        let addr = parse_address(&address)?;
        
        // Get UTXOs for this address by scanning the UTXO set
        let utxos = self.db.get_utxos_for_address(&addr)
            .map_err(|e| RpcError::InternalError(e.to_string()))?;
        
        let balance: u64 = utxos.iter().map(|(_, u)| u.output.value).sum();
        let utxo_list: Vec<RpcUtxo> = utxos.iter().map(|(outpoint, utxo)| {
            RpcUtxo {
                txid: format!("0x{}", hex::encode(&outpoint.txid.0)),
                vout: outpoint.vout,
                value: utxo.output.value,
                script_pubkey: format!("0x{}", hex::encode(&utxo.output.script_pubkey)),
                height: utxo.height,
                coinbase: utxo.is_coinbase,
            }
        }).collect();
        
        Ok(RpcBalance {
            address,
            balance,
            utxo_count: utxo_list.len(),
            utxos: utxo_list,
        })
    }

    async fn get_utxos(&self, address: String) -> RpcResult<Vec<RpcUtxo>> {
        let addr = parse_address(&address)?;
        
        // Get UTXOs for this address by scanning the UTXO set
        let utxos = self.db.get_utxos_for_address(&addr)
            .map_err(|e| RpcError::InternalError(e.to_string()))?;
        
        let utxo_list: Vec<RpcUtxo> = utxos.iter().map(|(outpoint, utxo)| {
            RpcUtxo {
                txid: format!("0x{}", hex::encode(&outpoint.txid.0)),
                vout: outpoint.vout,
                value: utxo.output.value,
                script_pubkey: format!("0x{}", hex::encode(&utxo.output.script_pubkey)),
                height: utxo.height,
                coinbase: utxo.is_coinbase,
            }
        }).collect();
        
        Ok(utxo_list)
    }

    async fn send_raw_transaction(&self, tx_hex: String) -> RpcResult<RpcSubmitResult> {
        let tx_hex = tx_hex.strip_prefix("0x").unwrap_or(&tx_hex);
        let tx_bytes = hex::decode(tx_hex)
            .map_err(|_| RpcError::InvalidParams("Invalid hex".to_string()))?;
        
        let tx: Transaction = bincode::deserialize(&tx_bytes)
            .map_err(|e| RpcError::InvalidParams(format!("Invalid transaction: {}", e)))?;

        let txid = tx.txid();
        
        // Validate inputs exist in UTXO set and calculate input value
        let mut input_value = 0u64;
        for input in &tx.inputs {
            if input.is_coinbase() {
                return Ok(RpcSubmitResult {
                    accepted: false,
                    hash: None,
                    error: Some("Coinbase transactions not allowed".to_string()),
                });
            }
            
            match self.db.get_utxo(&input.previous_output) {
                Ok(Some(utxo)) => {
                    input_value += utxo.output.value;
                }
                Ok(None) => {
                    return Ok(RpcSubmitResult {
                        accepted: false,
                        hash: None,
                        error: Some(format!("UTXO not found: {}:{}", input.previous_output.txid, input.previous_output.vout)),
                    });
                }
                Err(e) => {
                    return Ok(RpcSubmitResult {
                        accepted: false,
                        hash: None,
                        error: Some(format!("Database error: {}", e)),
                    });
                }
            }
        }
        
        // Check output value doesn't exceed input
        let output_value = tx.total_output();
        if output_value > input_value {
            return Ok(RpcSubmitResult {
                accepted: false,
                hash: None,
                error: Some(format!("Output value {} exceeds input value {}", output_value, input_value)),
            });
        }
        
        // Add to mempool if available
        if let Some(ref mempool) = self.mempool {
            if let Err(e) = mempool.add(tx, input_value) {
                return Ok(RpcSubmitResult {
                    accepted: false,
                    hash: None,
                    error: Some(format!("Mempool error: {}", e)),
                });
            }
            info!("Transaction {} added to mempool", txid);
        } else {
            info!("Transaction {} validated (no mempool)", txid);
        }
        
        Ok(RpcSubmitResult {
            accepted: true,
            hash: Some(format!("0x{}", hex::encode(&txid.0))),
            error: None,
        })
    }

    async fn get_mempool_info(&self) -> RpcResult<RpcMempoolInfo> {
        let (size, bytes) = if let Some(ref mp) = self.mempool {
            (mp.len(), mp.total_bytes())
        } else {
            (0, 0)
        };
        Ok(RpcMempoolInfo {
            size,
            bytes,
        })
    }

    async fn get_block_template(&self) -> RpcResult<RpcBlockTemplate> {
        let tip = self.db.get_tip();
        let block_reward = 50 * 100_000_000; // 50 PYRAX in satoshis

        Ok(RpcBlockTemplate {
            height: tip.height + 1,
            parent_hash: format!("0x{}", hex::encode(&tip.hash.0)),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            difficulty: 1, // Devnet difficulty
            target: "0x00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            transactions: vec![],
            coinbase_value: block_reward,
        })
    }

    async fn submit_block(&self, block_hex: String) -> RpcResult<RpcSubmitResult> {
        let block_hex = block_hex.strip_prefix("0x").unwrap_or(&block_hex);
        let block_bytes = hex::decode(block_hex)
            .map_err(|_| RpcError::InvalidParams("Invalid hex".to_string()))?;

        let block: Block = bincode::deserialize(&block_bytes)
            .map_err(|e| RpcError::InvalidParams(format!("Invalid block: {}", e)))?;

        let hash = block.hash();

        match self.db.commit_block(&block) {
            Ok(()) => {
                info!("RPC: Block {} accepted at height {}", hash, block.height());
                Ok(RpcSubmitResult {
                    accepted: true,
                    hash: Some(format!("0x{}", hex::encode(&hash.0))),
                    error: None,
                })
            }
            Err(e) => Ok(RpcSubmitResult {
                accepted: false,
                hash: None,
                error: Some(e.to_string()),
            }),
        }
    }

    async fn get_mining_info(&self) -> RpcResult<RpcMiningInfo> {
        let tip = self.db.get_tip();
        Ok(RpcMiningInfo {
            mining: false, // Node doesn't mine directly, desktop app handles mining
            hashrate: 0.0,
            difficulty: 1.0, // Devnet difficulty
            blocks_found: 0,
            network_hashrate: 0.0,
            current_height: tip.height,
        })
    }
    
    async fn create_test_transaction(&self, from_address: String, to_address: String, amount: u64) -> RpcResult<RpcSubmitResult> {
        // Only allow on devnet
        if self.network_id != NetworkId::DEVNET {
            return Ok(RpcSubmitResult {
                accepted: false,
                hash: None,
                error: Some("Test transactions only allowed on devnet".to_string()),
            });
        }
        
        let from_addr = parse_address(&from_address)?;
        let to_addr = parse_address(&to_address)?;
        
        // Get UTXOs for the from address
        let utxos = self.db.get_utxos_for_address(&from_addr)
            .map_err(|e| RpcError::InternalError(e.to_string()))?;
        
        if utxos.is_empty() {
            return Ok(RpcSubmitResult {
                accepted: false,
                hash: None,
                error: Some("No UTXOs available for from_address".to_string()),
            });
        }
        
        // Select first UTXO that covers the amount + fee
        let fee = 1000u64;
        let required = amount + fee;
        
        let (outpoint, utxo) = utxos.into_iter()
            .find(|(_, u)| u.output.value >= required)
            .ok_or_else(|| RpcError::InvalidParams("No UTXO with sufficient value".to_string()))?;
        
        let change = utxo.output.value - required;
        
        // Create transaction with dummy signature (test only - real txs need proper signing)
        let input = TxInput::new(outpoint, vec![0u8; 65]); // Dummy signature for test
        
        let mut outputs = vec![TxOutput::p2pkh(amount, &to_addr)];
        if change > 0 {
            outputs.push(TxOutput::p2pkh(change, &from_addr));
        }
        
        let tx = Transaction::new(vec![input], outputs);
        let txid = tx.txid();
        
        // Add to mempool if available
        if let Some(ref mempool) = self.mempool {
            let input_value = utxo.output.value;
            if let Err(e) = mempool.add(tx, input_value) {
                return Ok(RpcSubmitResult {
                    accepted: false,
                    hash: None,
                    error: Some(format!("Mempool error: {}", e)),
                });
            }
            info!("Test transaction {} added to mempool (from: {}, to: {}, amount: {})", 
                txid, from_address, to_address, amount);
            
            Ok(RpcSubmitResult {
                accepted: true,
                hash: Some(format!("0x{}", hex::encode(&txid.0))),
                error: None,
            })
        } else {
            Ok(RpcSubmitResult {
                accepted: false,
                hash: None,
                error: Some("Mempool not available".to_string()),
            })
        }
    }

    async fn get_network_info(&self) -> RpcResult<super::RpcNetworkInfo> {
        if let Some(ref registry) = self.peer_registry {
            let peers = registry.get_peers().await;
            let local_peer_id = registry.local_peer_id().await;
            let listen_addresses = registry.listen_addresses().await;

            let rpc_peers: Vec<super::RpcPeerInfo> = peers.iter().map(|p| {
                super::RpcPeerInfo {
                    peer_id: p.peer_id.clone(),
                    address: p.address.clone(),
                    ip: p.ip.clone(),
                    port: p.port,
                    protocol: format!("/pyrax/{}/1.0.0", self.network_id.name()),
                    direction: p.direction.to_string(),
                    connected_secs: p.connected_at.elapsed().as_secs(),
                    last_seen: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64 - p.last_seen.elapsed().as_millis() as u64,
                    version: p.client_version.clone(),
                    block_height: p.best_height,
                }
            }).collect();

            // Get extended P2P stats from registry
            let metrics = registry.get_metrics().await;
            
            // Convert mesh connections to RPC format
            let rpc_mesh_connections: Vec<super::RpcMeshConnection> = metrics.mesh_connections.iter().map(|c| {
                super::RpcMeshConnection {
                    peer_a: c.peer_a.clone(),
                    peer_b: c.peer_b.clone(),
                    topic: c.topic.clone(),
                    connection_type: c.connection_type.clone(),
                }
            }).collect();
            
            let rpc_relay_circuits: Vec<super::RpcRelayCircuit> = metrics.relay_circuits.iter().map(|c| {
                super::RpcRelayCircuit {
                    src_peer: c.src_peer.clone(),
                    dst_peer: c.dst_peer.clone(),
                    established_at: c.established_at,
                }
            }).collect();
            
            // PEER COUNT FIX: Use metrics as fallback if peer list is empty but metrics show connections
            // This handles race conditions where metrics update before peer registry
            let effective_peer_count = if rpc_peers.is_empty() && (metrics.inbound_peers + metrics.outbound_peers) > 0 {
                metrics.inbound_peers + metrics.outbound_peers
            } else {
                rpc_peers.len()
            };
            
            Ok(super::RpcNetworkInfo {
                peer_count: effective_peer_count,
                peers: rpc_peers,
                local_peer_id,
                listen_addresses,
                // Extended P2P stats
                inbound_peers: metrics.inbound_peers,
                outbound_peers: metrics.outbound_peers,
                target_peers: metrics.target_peers,
                max_peers: metrics.max_peers,
                dial_attempts: metrics.dial_attempts,
                dial_successes: metrics.dial_successes,
                dial_failures: metrics.dial_failures,
                average_rtt_ms: metrics.average_rtt_ms,
                network_state: metrics.network_state.clone(),
                nat_status: metrics.nat_status.clone(),
                mesh_peers: metrics.mesh_peers,
                gossip_peers: metrics.gossip_peers,
                // Mesh topology for visualizer
                mesh_connections: rpc_mesh_connections,
                relay_circuits: rpc_relay_circuits,
            })
        } else {
            Ok(super::RpcNetworkInfo {
                peer_count: 0,
                peers: vec![],
                local_peer_id: String::new(),
                listen_addresses: vec![],
                // Default extended stats
                inbound_peers: 0,
                outbound_peers: 0,
                target_peers: 50,
                max_peers: 60,
                dial_attempts: 0,
                dial_successes: 0,
                dial_failures: 0,
                average_rtt_ms: None,
                network_state: "Disconnected".to_string(),
                nat_status: "Unknown".to_string(),
                mesh_peers: 0,
                gossip_peers: 0,
                mesh_connections: vec![],
                relay_circuits: vec![],
            })
        }
    }

    async fn get_peers(&self) -> RpcResult<Vec<RpcPeerInfo>> {
        if let Some(ref registry) = self.peer_registry {
            let peers = registry.get_peers().await;
            let rpc_peers: Vec<RpcPeerInfo> = peers.iter().map(|p| {
                RpcPeerInfo {
                    peer_id: p.peer_id.clone(),
                    address: p.address.clone(),
                    ip: p.ip.clone(),
                    port: p.port,
                    protocol: format!("/pyrax/{}/1.0.0", self.network_id.name()),
                    direction: p.direction.to_string(),
                    connected_secs: p.connected_at.elapsed().as_secs(),
                    last_seen: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64 - p.last_seen.elapsed().as_millis() as u64,
                    version: p.client_version.clone(),
                    block_height: p.best_height,
                }
            }).collect();
            Ok(rpc_peers)
        } else {
            Ok(vec![])
        }
    }

    async fn health(&self) -> RpcResult<String> {
        Ok("ok".to_string())
    }

    async fn get_peer_id(&self) -> RpcResult<String> {
        if let Some(ref registry) = self.peer_registry {
            Ok(registry.local_peer_id().await)
        } else {
            // No P2P enabled - return empty string
            Ok(String::new())
        }
    }

    async fn debug_p2p_state(&self) -> RpcResult<super::RpcP2PDebugState> {
        if let Some(ref registry) = self.peer_registry {
            let peers = registry.get_peers().await;
            let metrics = registry.get_metrics().await;
            
            let registry_peer_count = peers.len();
            let registry_peers: Vec<String> = peers.iter().map(|p| p.peer_id.clone()).collect();
            
            let metrics_total = metrics.inbound_peers + metrics.outbound_peers;
            
            // Calculate effective peer count (same logic as get_network_info)
            let effective_peer_count = if registry_peer_count > 0 {
                registry_peer_count
            } else if metrics_total > 0 {
                metrics_total
            } else {
                0
            };
            
            // Generate diagnosis
            let diagnosis = if registry_peer_count == metrics_total {
                "OK: Registry and metrics are in sync".to_string()
            } else if registry_peer_count == 0 && metrics_total > 0 {
                format!("MISMATCH: Registry empty but metrics show {} peers - using metrics fallback", metrics_total)
            } else if registry_peer_count > 0 && metrics_total == 0 {
                format!("MISMATCH: Registry has {} peers but metrics show 0 - metrics may not be updating", registry_peer_count)
            } else {
                format!("DRIFT: Registry has {} peers, metrics show {} - minor sync delay", registry_peer_count, metrics_total)
            };
            
            Ok(super::RpcP2PDebugState {
                registry_peer_count,
                registry_peers,
                metrics_inbound_peers: metrics.inbound_peers,
                metrics_outbound_peers: metrics.outbound_peers,
                metrics_mesh_peers: metrics.mesh_peers,
                metrics_gossip_peers: metrics.gossip_peers,
                metrics_dial_attempts: metrics.dial_attempts,
                metrics_dial_successes: metrics.dial_successes,
                metrics_dial_failures: metrics.dial_failures,
                metrics_network_state: metrics.network_state,
                metrics_nat_status: metrics.nat_status,
                effective_peer_count,
                diagnosis,
            })
        } else {
            Ok(super::RpcP2PDebugState {
                registry_peer_count: 0,
                registry_peers: vec![],
                metrics_inbound_peers: 0,
                metrics_outbound_peers: 0,
                metrics_mesh_peers: 0,
                metrics_gossip_peers: 0,
                metrics_dial_attempts: 0,
                metrics_dial_successes: 0,
                metrics_dial_failures: 0,
                metrics_network_state: "P2P Disabled".to_string(),
                metrics_nat_status: "Unknown".to_string(),
                effective_peer_count: 0,
                diagnosis: "P2P is not enabled - no peer registry available".to_string(),
            })
        }
    }

    async fn get_address_transactions(&self, address: String, limit: Option<u32>) -> RpcResult<RpcAddressTransactions> {
        use crate::storage::TxDirection;
        
        let addr = parse_address(&address)?;
        let max_txs = limit.unwrap_or(50).min(100) as usize;
        let tip = self.db.get_tip();
        
        // Get transactions for this address
        let txs = self.db.get_transactions_for_address(&addr, max_txs)
            .map_err(|e| RpcError::InternalError(e.to_string()))?;
        
        let mut total_received = 0u64;
        let mut total_sent = 0u64;
        
        let rpc_txs: Vec<RpcAddressTx> = txs.iter().map(|(tx, loc, direction)| {
            // Calculate value for this address in this transaction
            let value: u64 = tx.outputs.iter()
                .filter_map(|o| {
                    if let Some(out_addr) = o.get_address() {
                        if out_addr == addr {
                            return Some(o.value);
                        }
                    }
                    None
                })
                .sum();
            
            match direction {
                TxDirection::Receive | TxDirection::Mining => total_received += value,
                TxDirection::Send => total_sent += value,
                _ => {}
            }
            
            // Get block timestamp
            let timestamp = self.db.get_block(&loc.block_hash)
                .ok()
                .flatten()
                .map(|b| b.header.timestamp)
                .unwrap_or(0);
            
            let confirmations = tip.height.saturating_sub(loc.block_height) + 1;
            
            RpcAddressTx {
                txid: format!("0x{}", hex::encode(&tx.txid().0)),
                block_hash: format!("0x{}", hex::encode(&loc.block_hash.0)),
                block_height: loc.block_height,
                tx_index: loc.tx_index,
                direction: match direction {
                    TxDirection::Receive => "receive".to_string(),
                    TxDirection::Send => "send".to_string(),
                    TxDirection::Mining => "mining".to_string(),
                    TxDirection::Unknown => "unknown".to_string(),
                },
                value,
                timestamp,
                is_coinbase: tx.is_coinbase(),
                confirmations,
            }
        }).collect();
        
        Ok(RpcAddressTransactions {
            address,
            transactions: rpc_txs.clone(),
            total_received,
            total_sent,
            tx_count: rpc_txs.len(),
        })
    }
}

/// Start the RPC server
pub async fn start_server(
    addr: &str,
    db: Arc<ChainDB>,
    network_id: NetworkId,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let addr: SocketAddr = addr.parse()?;
    
    let server = ServerBuilder::default()
        .build(addr)
        .await?;

    let rpc = RpcServerImpl::new(db, network_id);
    let handle = server.start(rpc.into_rpc());

    info!("JSON-RPC server started on http://{}", addr);
    Ok(handle)
}

/// Start the RPC server with mempool support
pub async fn start_server_with_mempool(
    addr: &str,
    db: Arc<ChainDB>,
    network_id: NetworkId,
    mempool: Arc<Mempool>,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let addr: SocketAddr = addr.parse()?;
    
    let server = ServerBuilder::default()
        .build(addr)
        .await?;

    let rpc = RpcServerImpl::with_mempool(db, network_id, mempool);
    let handle = server.start(rpc.into_rpc());

    info!("JSON-RPC server started on http://{} (with mempool)", addr);
    Ok(handle)
}

/// Start the RPC server with mempool and P2P peer registry
pub async fn start_server_with_peers(
    addr: &str,
    db: Arc<ChainDB>,
    network_id: NetworkId,
    mempool: Arc<Mempool>,
    peer_registry: crate::p2p::PeerRegistry,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let addr: SocketAddr = addr.parse()?;
    
    let server = ServerBuilder::default()
        .build(addr)
        .await?;

    let rpc = RpcServerImpl::with_mempool_and_peers(db, network_id, mempool, peer_registry);
    let handle = server.start(rpc.into_rpc());

    info!("JSON-RPC server started on http://{} (with mempool + P2P)", addr);
    Ok(handle)
}

/// Start the Staking RPC server for Stream C
pub async fn start_staking_server(
    addr: &str,
    staking_service: Arc<crate::services::staking::StakingService>,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    use super::staking_rpc::{StakingRpcImpl, StakingRpcServer};
    
    let addr: SocketAddr = addr.parse()?;
    
    let server = ServerBuilder::default()
        .build(addr)
        .await?;

    let rpc = StakingRpcImpl::new(staking_service);
    let handle = server.start(rpc.into_rpc());

    info!("Stream C Staking RPC server started on http://{}", addr);
    Ok(handle)
}

fn parse_hash(s: &str) -> Result<H256, RpcError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s)
        .map_err(|_| RpcError::InvalidParams("Invalid hash hex".to_string()))?;
    if bytes.len() != 32 {
        return Err(RpcError::InvalidParams("Hash must be 32 bytes".to_string()));
    }
    Ok(H256::from_slice(&bytes))
}

fn parse_address(s: &str) -> Result<Address, RpcError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s)
        .map_err(|_| RpcError::InvalidParams("Invalid address hex".to_string()))?;
    if bytes.len() != 20 {
        return Err(RpcError::InvalidParams("Address must be 20 bytes".to_string()));
    }
    Ok(Address::from_slice(&bytes))
}
