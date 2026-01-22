use crate::state::AppState;
use crate::rpc::RpcClient;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tauri::State;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
    pub hash: String,
    pub height: u64,
    pub parent_hash: String,
    pub timestamp: u64,
    pub difficulty: String,
    pub nonce: String,
    pub merkle_root: String,
    pub state_root: String,
    pub beneficiary: String,
    pub transaction_count: usize,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    pub hash: String,
    pub tx_type: String,
    pub from: String,
    pub to: Option<String>,
    pub value: String,
    pub gas_price: String,
    pub gas_limit: String,
    pub gas_used: Option<String>,
    pub nonce: u64,
    pub data: String,
    pub block_hash: Option<String>,
    pub block_number: Option<u64>,
    pub transaction_index: Option<usize>,
    pub status: Option<String>,
}

#[tauri::command]
pub async fn get_block(
    identifier: String, // hash or height
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<BlockInfo, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let rpc = RpcClient::localhost(rpc_port);
    
    // Determine if identifier is a hash or height
    let block = if identifier.starts_with("0x") {
        // It's a hash
        rpc.get_block_by_hash(&identifier, false).await
            .map_err(|e| format!("Failed to get block: {}", e))?
    } else {
        // It's a height
        let height: u64 = identifier.parse()
            .map_err(|_| "Invalid block identifier".to_string())?;
        rpc.get_block_by_number(height, false).await
            .map_err(|e| format!("Failed to get block: {}", e))?
    };
    
    match block {
        Some(b) => {
            let height = u64::from_str_radix(b.number.trim_start_matches("0x"), 16).unwrap_or(0);
            let timestamp = u64::from_str_radix(b.timestamp.trim_start_matches("0x"), 16).unwrap_or(0);
            Ok(BlockInfo {
                hash: b.hash,
                height,
                parent_hash: b.parent_hash,
                timestamp,
                difficulty: b.difficulty,
                nonce: "0x0".to_string(),
                merkle_root: "0x0".to_string(),
                state_root: "0x0".to_string(),
                beneficiary: b.miner,
                transaction_count: b.transaction_count as usize,
                size: u64::from_str_radix(b.size.trim_start_matches("0x"), 16).unwrap_or(0) as usize,
            })
        }
        None => Err("Block not found".to_string()),
    }
}

#[tauri::command]
pub async fn get_transaction(
    hash: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<TransactionInfo, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let rpc = RpcClient::localhost(rpc_port);
    
    match rpc.get_transaction(&hash).await {
        Ok(Some(tx)) => {
            let block_number = tx.block_number.as_ref()
                .and_then(|n| u64::from_str_radix(n.trim_start_matches("0x"), 16).ok());
            let tx_index = tx.transaction_index.as_ref()
                .and_then(|n| u64::from_str_radix(n.trim_start_matches("0x"), 16).ok().map(|v| v as usize));
            
            Ok(TransactionInfo {
                hash: tx.hash,
                tx_type: "transfer".to_string(),
                from: tx.from,
                to: tx.to,
                value: tx.value,
                gas_price: tx.gas_price,
                gas_limit: tx.gas,
                gas_used: None,
                nonce: u64::from_str_radix(tx.nonce.trim_start_matches("0x"), 16).unwrap_or(0),
                data: tx.input,
                block_hash: tx.block_hash,
                block_number,
                transaction_index: tx_index,
                status: Some("confirmed".to_string()),
            })
        }
        Ok(None) => Err("Transaction not found".to_string()),
        Err(e) => Err(format!("Failed to get transaction: {}", e)),
    }
}

#[tauri::command]
pub async fn get_recent_blocks(
    count: Option<u32>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<BlockInfo>, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let block_count = count.unwrap_or(10).min(50) as u64; // Cap at 50 blocks
    let rpc = RpcClient::localhost(rpc_port);
    
    // Get current chain height
    let chain_info = rpc.get_chain_info().await
        .map_err(|e| format!("Failed to get chain info: {}", e))?;
    
    let current_height = chain_info.best_block_height;
    let mut blocks = Vec::new();
    
    // Fetch recent blocks from current height down
    for i in 0..block_count {
        let height = current_height.saturating_sub(i);
        if let Ok(Some(b)) = rpc.get_block_by_number(height, false).await {
            let block_height = u64::from_str_radix(b.number.trim_start_matches("0x"), 16).unwrap_or(height);
            let timestamp = u64::from_str_radix(b.timestamp.trim_start_matches("0x"), 16).unwrap_or(0);
            blocks.push(BlockInfo {
                hash: b.hash,
                height: block_height,
                parent_hash: b.parent_hash,
                timestamp,
                difficulty: b.difficulty,
                nonce: "0x0".to_string(),
                merkle_root: "0x0".to_string(),
                state_root: "0x0".to_string(),
                beneficiary: b.miner,
                transaction_count: b.transaction_count as usize,
                size: u64::from_str_radix(b.size.trim_start_matches("0x"), 16).unwrap_or(0) as usize,
            });
        }
        
        if height == 0 {
            break; // Reached genesis
        }
    }
    
    Ok(blocks)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootnodeInfo {
    pub id: String,
    pub url: String,
    #[serde(rename = "latencyMs")]
    pub latency_ms: u64,
    pub city: String,
    pub region: String,
    pub country: String,
    #[serde(rename = "countryCode")]
    pub country_code: String,
    pub stream: String,
    pub online: bool,
}

// Bootnode configurations with location info - ACTUAL production bootnodes
fn get_bootnode_configs() -> Vec<(String, String, String, String, String, String, String)> {
    vec![
        // (id, url, city, region, country, country_code, stream)
        // Primary bootnode - DigitalOcean NYC
        ("bootnode-nyc-1".to_string(), "http://209.38.137.105:28545".to_string(), "New York".to_string(), "NY".to_string(), "United States".to_string(), "US".to_string(), "A".to_string()),
    ]
}

#[tauri::command]
pub async fn get_bootnode_info(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<BootnodeInfo>, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    info!("get_bootnode_info called: running={}, rpc_port={}", running, rpc_port);
    
    // Always return nodes even if not fully running - just mark local as offline
    let mut bootnodes = Vec::new();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();
    
    // First, add the user's own local node (THIS NODE) - ALWAYS show this
    let local_url = format!("http://127.0.0.1:{}", rpc_port);
    let start = Instant::now();
    let local_online = if running {
        match client
            .post(&local_url)
            .header("Content-Type", "application/json")
            .body(r#"{"jsonrpc":"2.0","method":"pyrax_getChainInfo","params":[],"id":1}"#)
            .send()
            .await
        {
            Ok(response) => {
                let success = response.status().is_success();
                info!("Local node RPC check: status={}, success={}", response.status(), success);
                success
            }
            Err(e) => {
                warn!("Local node RPC check failed: {}", e);
                false
            }
        }
    } else {
        info!("Node not running, marking local as offline");
        false
    };
    let local_latency = start.elapsed().as_millis() as u64;
    
    info!("Local node: online={}, latency={}ms", local_online, local_latency);
    
    bootnodes.push(BootnodeInfo {
        id: "this-node".to_string(),
        url: local_url,
        latency_ms: if local_online { local_latency } else { 0 },
        city: "Local".to_string(),
        region: "".to_string(),
        country: "This Machine".to_string(),
        country_code: "🖥️".to_string(), // Special marker for local
        stream: "A".to_string(),
        online: local_online,
    });
    
    // Then add remote bootnodes
    let configs = get_bootnode_configs();
    for (id, url, city, region, country, country_code, stream) in configs {
        let start = Instant::now();
        let (online, latency_ms) = match client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(r#"{"jsonrpc":"2.0","method":"pyrax_getChainInfo","params":[],"id":1}"#)
            .send()
            .await
        {
            Ok(response) => {
                let latency = start.elapsed().as_millis() as u64;
                let success = response.status().is_success();
                info!("Remote bootnode {}: status={}, latency={}ms", id, response.status(), latency);
                (success, latency)
            }
            Err(e) => {
                warn!("Remote bootnode {} failed: {}", id, e);
                (false, 0)
            }
        };
        
        bootnodes.push(BootnodeInfo {
            id,
            url,
            latency_ms,
            city,
            region,
            country,
            country_code,
            stream,
            online,
        });
    }
    
    info!("Returning {} bootnodes", bootnodes.len());
    Ok(bootnodes)
}

// ========== NEW EXPLORER COMMANDS FOR CUSTOM DESKTOP EXPLORER ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedBlocks {
    pub blocks: Vec<BlockInfo>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedTransactions {
    pub transactions: Vec<TransactionInfo>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    pub balance: String,
    pub balance_formatted: String,
    pub transaction_count: u64,
    pub is_contract: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub block_height: u64,
    pub difficulty: String,
    pub hash_rate: String,
    pub peer_count: u64,
    pub pending_tx_count: u64,
    pub gas_price: String,
    pub chain_id: u64,
    pub syncing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerSearchResult {
    pub result_type: String, // "block", "transaction", "address", "not_found"
    pub block: Option<BlockInfo>,
    pub transaction: Option<TransactionInfo>,
    pub address: Option<AddressInfo>,
}

/// Get paginated blocks for the explorer
#[tauri::command]
pub async fn get_blocks_paginated(
    page: u32,
    per_page: u32,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<PaginatedBlocks, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let per_page = per_page.min(50).max(1); // Clamp to 1-50
    let rpc = RpcClient::localhost(rpc_port);
    
    let chain_info = rpc.get_chain_info().await
        .map_err(|e| format!("Failed to get chain info: {}", e))?;
    
    let total = chain_info.best_block_height + 1; // Include genesis
    let total_pages = ((total as f64) / (per_page as f64)).ceil() as u32;
    let page = page.min(total_pages).max(1);
    
    let start_height = total.saturating_sub((page as u64) * (per_page as u64));
    let end_height = start_height.saturating_add(per_page as u64).min(total);
    
    let mut blocks = Vec::new();
    for height in (start_height..end_height).rev() {
        if let Ok(Some(b)) = rpc.get_block_by_number(height, false).await {
            let block_height = u64::from_str_radix(b.number.trim_start_matches("0x"), 16).unwrap_or(height);
            let timestamp = u64::from_str_radix(b.timestamp.trim_start_matches("0x"), 16).unwrap_or(0);
            blocks.push(BlockInfo {
                hash: b.hash,
                height: block_height,
                parent_hash: b.parent_hash,
                timestamp,
                difficulty: b.difficulty,
                nonce: "0x0".to_string(),
                merkle_root: "0x0".to_string(),
                state_root: "0x0".to_string(),
                beneficiary: b.miner,
                transaction_count: b.transaction_count as usize,
                size: u64::from_str_radix(b.size.trim_start_matches("0x"), 16).unwrap_or(0) as usize,
            });
        }
    }
    
    Ok(PaginatedBlocks {
        blocks,
        total,
        page,
        per_page,
        total_pages,
    })
}

/// Get network stats for explorer dashboard
#[tauri::command]
pub async fn get_network_stats(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<NetworkStats, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let rpc = RpcClient::localhost(rpc_port);
    
    let chain_info = rpc.get_chain_info().await
        .map_err(|e| format!("Failed to get chain info: {}", e))?;
    
    // Get peer count
    let peer_count = rpc.get_peer_count().await.unwrap_or(0);
    
    // Get pending transactions
    let pending_tx = rpc.get_pending_transaction_count().await.unwrap_or(0);
    
    // Get gas price
    let gas_price = rpc.get_gas_price().await.unwrap_or("0x0".to_string());
    
    Ok(NetworkStats {
        block_height: chain_info.best_block_height,
        difficulty: format!("{}", chain_info.difficulty),
        hash_rate: "0".to_string(), // Will be calculated from difficulty
        peer_count,
        pending_tx_count: pending_tx as u64,
        gas_price,
        chain_id: 7225, // Devnet chain ID
        syncing: chain_info.syncing,
    })
}

/// Get address info including balance
#[tauri::command]
pub async fn get_address_info(
    address: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<AddressInfo, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let rpc = RpcClient::localhost(rpc_port);
    
    // Get balance (returns u64 in smallest unit)
    let balance = rpc.get_balance(&address).await
        .map_err(|e| format!("Failed to get balance: {}", e))?;
    
    // Convert balance to PYRAX (8 decimals for UTXO model)
    let balance_pyrax = balance as f64 / 1e8;
    
    // Get transaction count (nonce) - returns 0 for UTXO model
    let tx_count = rpc.get_transaction_count(&address).await
        .map_err(|e| format!("Failed to get transaction count: {}", e))?;
    
    // Check if contract - UTXO model doesn't have contracts
    let code = rpc.get_code(&address).await.unwrap_or_else(|_| "0x".to_string());
    let is_contract = code.len() > 2;
    
    Ok(AddressInfo {
        address: address.clone(),
        balance: format!("0x{:x}", balance),
        balance_formatted: format!("{:.8} PYRAX", balance_pyrax),
        transaction_count: tx_count,
        is_contract,
    })
}

/// Universal search for explorer - searches blocks, transactions, addresses
#[tauri::command]
pub async fn search_explorer(
    query: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ExplorerSearchResult, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let query = query.trim();
    let rpc = RpcClient::localhost(rpc_port);
    
    // Check if it's a block number
    if let Ok(block_num) = query.parse::<u64>() {
        if let Ok(Some(b)) = rpc.get_block_by_number(block_num, false).await {
            let height = u64::from_str_radix(b.number.trim_start_matches("0x"), 16).unwrap_or(block_num);
            let timestamp = u64::from_str_radix(b.timestamp.trim_start_matches("0x"), 16).unwrap_or(0);
            return Ok(ExplorerSearchResult {
                result_type: "block".to_string(),
                block: Some(BlockInfo {
                    hash: b.hash,
                    height,
                    parent_hash: b.parent_hash,
                    timestamp,
                    difficulty: b.difficulty,
                    nonce: "0x0".to_string(),
                    merkle_root: "0x0".to_string(),
                    state_root: "0x0".to_string(),
                    beneficiary: b.miner,
                    transaction_count: b.transaction_count as usize,
                    size: u64::from_str_radix(b.size.trim_start_matches("0x"), 16).unwrap_or(0) as usize,
                }),
                transaction: None,
                address: None,
            });
        }
    }
    
    // Check if it's a hex string (hash or address)
    if query.starts_with("0x") {
        // Check if it's an address (40 hex chars + 0x = 42 chars)
        if query.len() == 42 {
            // Try to get address info
            if let Ok(balance) = rpc.get_balance(query).await {
                let balance_pyrax = balance as f64 / 1e8;
                let tx_count = rpc.get_transaction_count(query).await.unwrap_or(0);
                let code = rpc.get_code(query).await.unwrap_or_else(|_| "0x".to_string());
                
                return Ok(ExplorerSearchResult {
                    result_type: "address".to_string(),
                    block: None,
                    transaction: None,
                    address: Some(AddressInfo {
                        address: query.to_string(),
                        balance: format!("0x{:x}", balance),
                        balance_formatted: format!("{:.8} PYRAX", balance_pyrax),
                        transaction_count: tx_count,
                        is_contract: code.len() > 2,
                    }),
                });
            }
        }
        
        // Check if it's a transaction hash (64 hex chars + 0x = 66 chars)
        if query.len() == 66 {
            // Try transaction first
            if let Ok(Some(tx)) = rpc.get_transaction(query).await {
                let block_number = tx.block_number.as_ref()
                    .and_then(|n| u64::from_str_radix(n.trim_start_matches("0x"), 16).ok());
                let tx_index = tx.transaction_index.as_ref()
                    .and_then(|n| u64::from_str_radix(n.trim_start_matches("0x"), 16).ok().map(|v| v as usize));
                
                return Ok(ExplorerSearchResult {
                    result_type: "transaction".to_string(),
                    block: None,
                    transaction: Some(TransactionInfo {
                        hash: tx.hash,
                        tx_type: "transfer".to_string(),
                        from: tx.from,
                        to: tx.to,
                        value: tx.value,
                        gas_price: tx.gas_price,
                        gas_limit: tx.gas,
                        gas_used: None,
                        nonce: u64::from_str_radix(tx.nonce.trim_start_matches("0x"), 16).unwrap_or(0),
                        data: tx.input,
                        block_hash: tx.block_hash,
                        block_number,
                        transaction_index: tx_index,
                        status: Some("confirmed".to_string()),
                    }),
                    address: None,
                });
            }
            
            // Try block hash
            if let Ok(Some(b)) = rpc.get_block_by_hash(query, false).await {
                let height = u64::from_str_radix(b.number.trim_start_matches("0x"), 16).unwrap_or(0);
                let timestamp = u64::from_str_radix(b.timestamp.trim_start_matches("0x"), 16).unwrap_or(0);
                return Ok(ExplorerSearchResult {
                    result_type: "block".to_string(),
                    block: Some(BlockInfo {
                        hash: b.hash,
                        height,
                        parent_hash: b.parent_hash,
                        timestamp,
                        difficulty: b.difficulty,
                        nonce: "0x0".to_string(),
                        merkle_root: "0x0".to_string(),
                        state_root: "0x0".to_string(),
                        beneficiary: b.miner,
                        transaction_count: b.transaction_count as usize,
                        size: u64::from_str_radix(b.size.trim_start_matches("0x"), 16).unwrap_or(0) as usize,
                    }),
                    transaction: None,
                    address: None,
                });
            }
        }
    }
    
    Ok(ExplorerSearchResult {
        result_type: "not_found".to_string(),
        block: None,
        transaction: None,
        address: None,
    })
}
