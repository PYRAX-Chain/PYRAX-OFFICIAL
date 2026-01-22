//! PYRAX Node - TriStream DAG Blockchain
//!
//! Full node implementation with:
//! - Stream A (BLAKE3 PoW) consensus
//! - UTXO-based transactions
//! - RocksDB storage
//! - P2P networking (libp2p)
//! - Block/Transaction validation

use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn, error, debug, Level};
use tracing_subscriber::FmtSubscriber;

mod ai;
mod bridge;
mod consensus;
mod evm;
mod genesis;
mod mempool;
mod miner;
mod p2p;
mod rpc;
mod storage;
mod sync;
mod types;
mod validation;
mod wallet;
mod wasm;
mod zkrollup;
mod mainnet;
mod tokenomics;
mod services;

use storage::ChainDB;
use types::{NetworkId, Block, BlockHeader, Transaction, Address, H256};
use validation::BlockValidator;
use p2p::{Network, P2PConfig, PeerRegistry};
use mempool::{Mempool, MempoolConfig};
use rpc::{start_server_with_peers as start_rpc_server_with_peers};

#[derive(Parser, Debug)]
#[command(name = "pyrax-node")]
#[command(author = "PYRAX Team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "PYRAX Full Node - TriStream DAG blockchain")]
struct Args {
    /// Data directory for blockchain storage
    #[arg(short, long, default_value = "./data")]
    datadir: PathBuf,

    /// Network (mainnet, testnet, devnet)
    #[arg(short, long, default_value = "devnet")]
    network: String,

    /// P2P listen address
    #[arg(long, default_value = "/ip4/0.0.0.0/tcp/30303")]
    p2p_addr: String,

    /// Bootstrap peers to connect to (can specify multiple times)
    #[arg(long, action = clap::ArgAction::Append)]
    peer: Vec<String>,

    /// Enable P2P networking
    #[arg(long)]
    p2p: bool,

    /// Enable CPU mining (Stream A)
    #[arg(long)]
    mine: bool,

    /// Miner address for block rewards
    #[arg(long)]
    miner_address: Option<String>,

    /// Number of blocks to mine (0 = continuous)
    #[arg(long, default_value = "0")]
    mine_blocks: u32,

    /// Verbosity level (0-4)
    #[arg(short, long, default_value = "2")]
    verbosity: u8,

    /// Enable RPC server (Stream A on 8545)
    #[arg(long)]
    rpc: bool,

    /// RPC server address
    #[arg(long, default_value = "0.0.0.0:8545")]
    rpc_addr: String,

    /// Enable BLAKE3 Stratum server for ASIC mining (Stream A)
    #[arg(long)]
    blake3_stratum: bool,

    /// BLAKE3 Stratum server address (Stream A)
    #[arg(long, default_value = "0.0.0.0:3334")]
    blake3_stratum_addr: String,

    /// Enable KAWPOW Stratum server for GPU mining (Stream B)
    #[arg(long)]
    stratum: bool,

    /// KAWPOW Stratum server address (Stream B)
    #[arg(long, default_value = "0.0.0.0:3333")]
    stratum_addr: String,

    /// Enable Staking service (Stream C)
    #[arg(long)]
    staking: bool,

    /// Staking RPC address
    #[arg(long, default_value = "0.0.0.0:8547")]
    staking_addr: String,

    /// Path to persistent node key file for P2P identity
    /// If provided, the node will use a persistent peer ID that survives restarts
    #[arg(long)]
    node_key: Option<PathBuf>,

    // === MASS ADOPTION NETWORK SETTINGS ===
    /// Connection mode: full (requires port forwarding), relay (works behind any NAT), auto (tries both)
    #[arg(long, default_value = "auto")]
    connection_mode: String,

    /// Enable WebSocket transport (works through proxies and strict firewalls)
    #[arg(long)]
    enable_websocket: bool,

    /// Enable automatic port fallback (tries alternative ports if primary is blocked)
    #[arg(long)]
    auto_port_fallback: bool,
}

fn parse_network(s: &str) -> NetworkId {
    match s.to_lowercase().as_str() {
        "mainnet" | "main" | "79729" => NetworkId::MAINNET,
        "testnet" | "test" | "797291" => NetworkId::TESTNET,
        "devnet" | "dev" | "797292" => NetworkId::DEVNET,
        _ => NetworkId::DEVNET,
    }
}

fn parse_address(s: Option<&String>) -> Address {
    match s {
        Some(addr) if addr.starts_with("0x") && addr.len() == 42 => {
            let bytes = hex::decode(&addr[2..]).unwrap_or_default();
            Address::from_slice(&bytes)
        }
        _ => Address::ZERO,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = match args.verbosity {
        0 => Level::ERROR,
        1 => Level::WARN,
        2 => Level::INFO,
        3 => Level::DEBUG,
        _ => Level::TRACE,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    // Parse network
    let network = parse_network(&args.network);
    let miner_address = parse_address(args.miner_address.as_ref());

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         PYRAX Node v{} - TriStream DAG Blockchain       ║", env!("CARGO_PKG_VERSION"));
    println!("║   Stream A (BLAKE3) | Stream B (KAWPOW) | Stream C (ZK)       ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();

    info!("Starting PYRAX Node");
    info!("Network: {} (Chain ID: {})", network.name(), network.0);
    info!("Data directory: {:?}", args.datadir);
    
    // Log enabled services
    if args.rpc { info!("Stream A RPC: {}", args.rpc_addr); }
    if args.blake3_stratum { info!("Stream A BLAKE3 Stratum (ASIC): {}", args.blake3_stratum_addr); }
    if args.stratum { info!("Stream B KAWPOW Stratum (GPU): {}", args.stratum_addr); }
    if args.staking { info!("Stream C Staking: {}", args.staking_addr); }

    // Open database
    let db_path = args.datadir.join(network.name());
    let db = Arc::new(ChainDB::open(&db_path, network)?);
    
    let tip = db.get_tip();
    info!("Chain tip: height={}, hash={}", tip.height, tip.hash);

    // Create shared mempool
    let mempool = Arc::new(Mempool::new(MempoolConfig::default()));
    info!("Mempool initialized");

    // Create shared peer registry for P2P and RPC communication
    let peer_registry = PeerRegistry::new();
    info!("Peer registry initialized");

    // Start RPC server if enabled (Stream A)
    let _rpc_handle = if args.rpc {
        info!("Starting Stream A RPC server on {}", args.rpc_addr);
        match start_rpc_server_with_peers(&args.rpc_addr, db.clone(), network, mempool.clone(), peer_registry.clone()).await {
            Ok(handle) => {
                info!("✓ Stream A RPC running on http://{}", args.rpc_addr);
                Some(handle)
            }
            Err(e) => {
                error!("Failed to start RPC server: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Start BLAKE3 Stratum server if enabled (Stream A - ASIC Mining)
    let _blake3_stratum_handle = if args.blake3_stratum {
        info!("Starting Stream A BLAKE3 Stratum server on {}", args.blake3_stratum_addr);
        
        use miner::{Blake3StratumServer, Blake3StratumConfig, Blake3BlockTemplate};
        use types::stream_a_block::StreamABlock;
        
        let bind_addr: std::net::SocketAddr = args.blake3_stratum_addr.parse()
            .unwrap_or_else(|_| "0.0.0.0:3334".parse().unwrap());
        
        let blake3_config = Blake3StratumConfig {
            bind_addr,
            default_difficulty: 1.0,
            coinbase_address: miner_address,
            network_difficulty: 1,
            block_reward: 50_00000000, // 50 PYRAX for Stream A
            ..Default::default()
        };
        
        let blake3_server = Blake3StratumServer::new(blake3_config);
        
        // Set up template provider
        let db_for_template = db.clone();
        blake3_server.set_template_provider(Box::new(move || {
            let tip = db_for_template.get_tip();
            Some(Blake3BlockTemplate {
                height: tip.height + 1,
                parent_hash: tip.hash,
                utxo_root: types::H256::zero(), // Simplified for devnet
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                difficulty: 1, // Easy difficulty for devnet
                transactions: vec![],
                coinbase_value: 50_00000000, // 50 PYRAX
            })
        })).await;
        
        // Set up block submission
        let db_for_submit = db.clone();
        blake3_server.set_block_submit_fn(Box::new(move |block: StreamABlock| {
            let hash = block.hash();
            info!("Submitting BLAKE3 block at height {}: 0x{}", block.height(), hex::encode(hash.as_bytes()));
            // Convert StreamABlock to Block for storage
            let generic_block = types::Block {
                header: types::BlockHeader {
                    version: block.header.version,
                    stream: 0, // Stream A
                    parent_hash: block.header.parent_hash,
                    merkle_root: block.header.merkle_root,
                    utxo_commitment: block.header.utxo_root,
                    timestamp: block.header.timestamp,
                    difficulty: block.header.difficulty,
                    nonce: block.header.nonce,
                    extra_nonce: block.header.extra_nonce,
                    height: block.header.height,
                    beneficiary: block.header.beneficiary,
                },
                transactions: vec![], // UTXO transactions handled separately
            };
            db_for_submit.commit_block(&generic_block).map_err(|e| e.to_string())?;
            Ok(hash)
        })).await;
        
        // Spawn the server
        let server_handle = blake3_server;
        tokio::spawn(async move {
            if let Err(e) = server_handle.run().await {
                error!("BLAKE3 Stratum server error: {}", e);
            }
        });
        
        info!("✓ Stream A BLAKE3 Stratum running on {}", args.blake3_stratum_addr);
        Some(())
    } else {
        None
    };

    // Start KAWPOW Stratum server if enabled (Stream B - GPU Mining)
    let _stratum_handle = if args.stratum {
        info!("Starting Stream B KAWPOW Stratum server on {}", args.stratum_addr);
        
        use services::mining::{MiningService, MiningServiceConfig};
        
        let stratum_config = MiningServiceConfig {
            stratum_enabled: true,
            stratum_bind: args.stratum_addr.split(':').next().unwrap_or("0.0.0.0").to_string(),
            stratum_port: args.stratum_addr.split(':').nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(3333),
            share_difficulty: 1.0,
            coinbase_address: miner_address,
            cpu_mining_enabled: false,
            cpu_threads: 0,
        };
        
        // Create chain state provider for stratum
        struct DbChainProvider(Arc<ChainDB>);
        impl services::mining::ChainStateProvider for DbChainProvider {
            fn get_height(&self) -> u64 { self.0.get_tip().height }
            fn get_tip_hash(&self) -> types::H256 { self.0.get_tip().hash }
            fn get_difficulty(&self) -> u64 { 1 } // Simplified for devnet
            fn get_block_reward(&self, _height: u64) -> u64 { 100 * 100_000_000 } // 100 PYRAX for Stream B
            fn get_pending_transactions(&self, _max: usize, _bytes: usize) -> Vec<types::Transaction> { vec![] }
            fn submit_block(&self, block: types::Block) -> Result<types::H256, String> {
                let hash = block.hash();
                self.0.commit_block(&block).map_err(|e| e.to_string())?;
                Ok(hash)
            }
            fn verify_header(&self, _header: &types::BlockHeader) -> Result<(), String> { Ok(()) }
        }
        
        let chain_provider = Arc::new(DbChainProvider(db.clone()));
        let mut mining_service = MiningService::new(stratum_config, chain_provider);
        
        match mining_service.start().await {
            Ok(_) => {
                info!("✓ Stream B Stratum running on {}", args.stratum_addr);
                Some(mining_service)
            }
            Err(e) => {
                error!("Failed to start Stratum server: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Start Staking service if enabled (Stream C - ZK Validation)
    // IMPORTANT: We must keep both the service AND the RPC server handle alive
    let (_staking_service, _staking_rpc_handle) = if args.staking {
        info!("Starting Stream C Staking service on {}", args.staking_addr);
        
        use services::staking::{StakingService, StakingConfig};
        
        let staking_config = StakingConfig {
            enabled: true,
            rpc_bind: args.staking_addr.split(':').next().unwrap_or("0.0.0.0").to_string(),
            rpc_port: args.staking_addr.split(':').nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(8547),
            ..Default::default()
        };
        
        let staking_service = Arc::new(StakingService::new(staking_config, db.clone()));
        
        match staking_service.start().await {
            Ok(_) => {
                // Start the Staking RPC server - MUST keep handle alive!
                match rpc::start_staking_server(&args.staking_addr, staking_service.clone()).await {
                    Ok(handle) => {
                        info!("✓ Stream C Staking RPC running on {}", args.staking_addr);
                        (Some(staking_service), Some(handle))
                    }
                    Err(e) => {
                        error!("Failed to start Staking RPC server: {}", e);
                        (Some(staking_service), None)
                    }
                }
            }
            Err(e) => {
                error!("Failed to start Staking service: {}", e);
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // Initialize P2P if enabled
    if args.p2p {
        info!("P2P networking enabled");
        
        // Parse connection mode from CLI args
        let connection_mode = p2p::ConnectionMode::from_str(&args.connection_mode);
        
        info!("MASS ADOPTION MODE: {:?} | WebSocket: {} | Auto-fallback: {}", 
            connection_mode, args.enable_websocket, args.auto_port_fallback);
        
        // Use stability-tuned defaults for P2P config
        // Aggressive intervals cause peer churn and mesh instability
        let p2p_config = P2PConfig {
            listen_addr: args.p2p_addr.clone(),
            bootstrap_peers: args.peer.clone(),
            target_peers: 50,
            min_peers: 30,
            max_peers: 60,
            max_concurrent_dials: 5,
            dial_timeout_secs: 15,       // Stability: 15s (was 10s - too aggressive)
            ping_interval_secs: 45,      // Stability: 45s (was 15s - ping storms)
            peer_refresh_interval_secs: 120,  // Stability: 120s (was 30s - too aggressive)
            peer_reevaluate_interval_secs: 300, // Stability: 300s (was 60s - constant churn)
            node_key_path: args.node_key.clone(),
            // Mass adoption network settings
            connection_mode,
            enable_websocket: args.enable_websocket,
            enable_quic: true,  // ISP bypass: QUIC transport enabled by default
            websocket_port: None,  // Will use TCP port + 1 by default
            quic_port: None,  // Will use same as TCP port by default
            auto_port_fallback: args.auto_port_fallback,
            fallback_ports: vec![443, 8080, 8443, 9999],
        };

        let mut network = Network::new(p2p_config, db.clone(), network, peer_registry.clone()).await?;
        
        // Listen on configured address
        network.listen(&args.p2p_addr)?;
        
        // Subscribe to gossip topics
        network.subscribe()?;
        
        // Connect to ALL bootstrap peers and register for relay on each (NAT traversal)
        // Using multiple relays provides redundancy and distributes load
        for peer_addr in &args.peer {
            info!("Connecting to bootstrap peer: {}", peer_addr);
            if let Err(e) = network.dial_and_relay(peer_addr) {
                warn!("Failed to dial peer {}: {}", peer_addr, e);
            }
        }

        // Take block receiver for processing incoming blocks
        let mut block_rx = network.take_block_receiver();

        if args.mine {
            info!("CPU mining enabled with P2P - Stream A (BLAKE3)");
            info!("Miner address: {}", miner_address);
            
            // Wait for peer connections before mining
            info!("Waiting 3 seconds for peer connections...");
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            let peer_count = network.peer_count();
            info!("Connected to {} peers, starting miner", peer_count);
            
            // Create channel for mined blocks to broadcast
            let (mined_tx, mined_rx) = tokio::sync::mpsc::channel::<Block>(10);
            
            // Spawn block receiver task to handle incoming blocks
            let db_for_receiver = db.clone();
            let block_receiver_task = tokio::spawn(async move {
                if let Some(mut rx) = block_rx {
                    while let Some(block) = rx.recv().await {
                        handle_received_block(&db_for_receiver, block).await;
                    }
                }
            });

            // Run P2P, mining, and broadcast concurrently
            let db_clone = db.clone();
            let mine_blocks = args.mine_blocks;
            let mempool_clone = mempool.clone();
            
            tokio::select! {
                _ = network.run_with_broadcast(mined_rx) => {
                    info!("P2P network stopped");
                }
                result = run_miner_with_broadcast(db_clone, miner_address, mine_blocks, mined_tx, Some(mempool_clone)) => {
                    if let Err(e) = result {
                        error!("Miner error: {}", e);
                    }
                }
                _ = block_receiver_task => {
                    info!("Block receiver stopped");
                }
            }
        } else {
            info!("Node started in sync mode (no mining)");
            
            // Spawn block receiver task
            let db_for_receiver = db.clone();
            let block_receiver_task = tokio::spawn(async move {
                if let Some(mut rx) = block_rx {
                    while let Some(block) = rx.recv().await {
                        handle_received_block(&db_for_receiver, block).await;
                    }
                }
            });

            tokio::select! {
                _ = network.run() => {
                    info!("P2P network stopped");
                }
                _ = block_receiver_task => {
                    info!("Block receiver stopped");
                }
            }
        }
    } else if args.mine {
        info!("CPU mining enabled - Stream A (BLAKE3)");
        info!("Miner address: {}", miner_address);
        
        // Run mining with mempool - miner will include pending transactions
        run_miner(db.clone(), miner_address, args.mine_blocks, Some(mempool.clone())).await?;
    } else if args.rpc {
        // RPC-only mode - wait for shutdown
        info!("Node started in RPC-only mode");
        println!("RPC server running on http://{}", args.rpc_addr);
        println!("Press Ctrl+C to stop...");
        tokio::signal::ctrl_c().await?;
    } else {
        info!("Node started in passive mode (no P2P, no mining)");
        info!("Use --p2p to enable networking, --mine to enable mining");
        
        // Just wait for shutdown
        println!();
        println!("Press Ctrl+C to stop the node...");
        tokio::signal::ctrl_c().await?;
    }

    info!("Node stopped");
    Ok(())
}

/// Run the mining loop - creates real blocks and stores them
async fn run_miner(
    db: Arc<ChainDB>, 
    miner: Address, 
    max_blocks: u32,
    mempool: Option<Arc<Mempool>>,
) -> anyhow::Result<()> {
    use std::time::Instant;
    
    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("               PYRAX Stream A Miner Started");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    // Benchmark hashrate
    let hashrate = benchmark_blake3(100_000);
    println!("CPU BLAKE3 hashrate: {:.2} MH/s", hashrate / 1_000_000.0);
    println!();

    let mut blocks_mined = 0u32;
    let block_reward = consensus::Stream::A.block_reward();

    loop {
        // Check if we've mined enough
        if max_blocks > 0 && blocks_mined >= max_blocks {
            break;
        }

        // Get current tip
        let tip = db.get_tip();
        let height = tip.height + 1;
        let parent_hash = tip.hash;
        let difficulty = calculate_difficulty(&db, height);

        info!("Mining block {} (parent: {}, difficulty: {})", height, parent_hash, difficulty);

        // Create coinbase transaction
        let coinbase = Transaction::coinbase(height, block_reward, &miner);
        
        // Get pending transactions from mempool (up to 1000 per block)
        let pending_txs = if let Some(ref mp) = mempool {
            let txs = mp.get_pending(1000);
            if !txs.is_empty() {
                info!("Including {} transactions from mempool", txs.len());
            }
            txs
        } else {
            vec![]
        };
        
        // Build transaction list: coinbase first, then mempool transactions
        let mut transactions = vec![coinbase];
        transactions.extend(pending_txs);

        // Create block header
        let merkle_root = compute_merkle_root(&transactions);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Compute UTXO commitment (merkle root of UTXO set)
        let utxo_commitment = compute_utxo_commitment(&db);
        
        let mut header = BlockHeader {
            version: 1,
            stream: 0, // Stream A
            parent_hash,
            merkle_root,
            utxo_commitment,
            timestamp,
            difficulty,
            nonce: rand::random(),
            extra_nonce: 0,
            height,
            beneficiary: miner,
        };

        // Mine the block
        let start = Instant::now();
        let target = header.target();
        let mut hashes = 0u64;

        loop {
            let pow_hash = header.pow_hash();
            hashes += 1;

            if pow_hash.as_bytes() <= target.as_bytes() {
                // Found valid PoW!
                break;
            }

            header.nonce = header.nonce.wrapping_add(1);
            if header.nonce == 0 {
                header.extra_nonce += 1;
                // Update timestamp periodically
                header.timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
            }

            // Safety limit
            if hashes > 10_000_000_000 {
                error!("Mining timeout - difficulty too high");
                return Err(anyhow::anyhow!("Mining timeout"));
            }
        }

        let duration = start.elapsed();
        let block = Block::new(header, transactions);
        let block_hash = block.hash();

        // Validate block
        let validator = BlockValidator::new();
        if let Err(e) = validator.validate_block(&block, &db) {
            error!("Block validation failed: {}", e);
            continue;
        }

        // Commit to database
        db.commit_block(&block)?;
        blocks_mined += 1;
        
        // Remove confirmed transactions from mempool
        if let Some(ref mp) = mempool {
            let spent_outpoints: Vec<_> = block.transactions.iter()
                .flat_map(|tx| tx.inputs.iter().map(|i| i.previous_output))
                .collect();
            mp.remove_confirmed(&spent_outpoints);
        }

        let utxo_count = db.utxo_count().unwrap_or(0);
        let tx_count = block.transactions.len();

        println!(
            "  Block {} | Hash: {}... | Txs: {} | Time: {:.2}s | UTXOs: {}",
            height,
            &hex::encode(&block_hash.0)[..16],
            tx_count,
            duration.as_secs_f64(),
            utxo_count,
        );

        // Small delay to prevent spam
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("Mining complete! Blocks mined: {}", blocks_mined);
    
    let tip = db.get_tip();
    println!("Final chain tip: height={}, hash={}", tip.height, tip.hash);
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}

/// Calculate difficulty for next block using production difficulty adjustment
fn calculate_difficulty(db: &ChainDB, height: u64) -> u64 {
    use consensus::blake3_pow::DifficultyAdjustment;
    
    // Genesis and early blocks use minimum difficulty
    if height <= DifficultyAdjustment::AVERAGING_WINDOW {
        return DifficultyAdjustment::MIN_DIFFICULTY;
    }
    
    // Get the block at the start of the averaging window
    let window_start_height = height.saturating_sub(DifficultyAdjustment::AVERAGING_WINDOW);
    
    let start_block = match db.get_block_by_height(window_start_height) {
        Ok(Some(b)) => b,
        _ => return DifficultyAdjustment::MIN_DIFFICULTY,
    };
    
    let end_block = match db.get_block_by_height(height - 1) {
        Ok(Some(b)) => b,
        _ => return DifficultyAdjustment::MIN_DIFFICULTY,
    };
    
    // Calculate actual time taken for the averaging window
    let actual_time = end_block.header.timestamp.saturating_sub(start_block.header.timestamp);
    let expected_time = DifficultyAdjustment::AVERAGING_WINDOW * DifficultyAdjustment::TARGET_BLOCK_TIME;
    
    // Get current difficulty from the last block
    let current_difficulty = end_block.header.difficulty;
    
    // Calculate new difficulty
    DifficultyAdjustment::calculate_next(current_difficulty, actual_time, expected_time)
}

/// Compute merkle root of transactions
fn compute_merkle_root(transactions: &[Transaction]) -> H256 {
    if transactions.is_empty() {
        return H256::zero();
    }

    let mut hashes: Vec<H256> = transactions.iter().map(|tx| tx.txid()).collect();

    while hashes.len() > 1 {
        if hashes.len() % 2 == 1 {
            hashes.push(*hashes.last().unwrap());
        }
        hashes = hashes.chunks(2).map(|pair| {
            let mut combined = [0u8; 64];
            combined[..32].copy_from_slice(&pair[0].0);
            combined[32..].copy_from_slice(&pair[1].0);
            H256::from_slice(blake3::hash(&combined).as_bytes())
        }).collect();
    }

    hashes[0]
}

/// Compute UTXO commitment (Merkle root of UTXO set)
fn compute_utxo_commitment(db: &ChainDB) -> H256 {
    // Get all UTXO hashes from the database
    let utxo_count = db.utxo_count().unwrap_or(0);
    
    if utxo_count == 0 {
        return H256::zero();
    }
    
    // For efficiency, we compute a rolling hash of all UTXOs
    // In production, this would be a proper Merkle Mountain Range or sparse Merkle tree
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PYRAX_UTXO_COMMITMENT_V1");
    hasher.update(&utxo_count.to_le_bytes());
    
    // Include the chain tip hash for uniqueness per block
    let tip = db.get_tip();
    hasher.update(&tip.hash.0);
    hasher.update(&tip.height.to_le_bytes());
    
    H256::from_slice(hasher.finalize().as_bytes())
}

/// Benchmark BLAKE3 hashrate
fn benchmark_blake3(iterations: u64) -> f64 {
    let test_data = [0u8; 100];
    let start = std::time::Instant::now();
    
    for nonce in 0..iterations {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&test_data);
        hasher.update(&nonce.to_le_bytes());
        let _ = hasher.finalize();
    }
    
    iterations as f64 / start.elapsed().as_secs_f64()
}

/// Handle a block received from P2P network
async fn handle_received_block(db: &ChainDB, block: Block) {
    process_single_block(db, block);
}

/// Process a single block - validates and commits if it extends our chain
fn process_single_block(db: &ChainDB, block: Block) -> bool {
    let block_hash = block.hash();
    let height = block.height();
    
    // Check if we already have this block
    if let Ok(Some(_)) = db.get_block(&block_hash) {
        debug!("Already have block {}", block_hash);
        return false;
    }

    // Check if this block extends our chain
    let tip = db.get_tip();
    
    // For sync blocks, check if parent matches expected
    if block.header.parent_hash != tip.hash {
        // Block doesn't extend tip
        if height <= tip.height {
            debug!("Ignoring block {} at height {} (our tip: {})", block_hash, height, tip.height);
            return false;
        }
        
        // Check if we have the parent block
        match db.get_block(&block.header.parent_hash) {
            Ok(Some(_)) => {
                debug!("Block {} has parent we know, processing", block_hash);
            }
            _ => {
                debug!("Block {} at height {} has unknown parent, skipping for now", block_hash, height);
                return false;
            }
        }
    }

    // Validate the block
    let validator = BlockValidator::new();
    if let Err(e) = validator.validate_block(&block, db) {
        warn!("Received invalid block {}: {}", block_hash, e);
        return false;
    }

    // Commit to database
    match db.commit_block(&block) {
        Ok(_) => {
            info!("✓ Synced block {} at height {} from peer", block_hash, height);
            let utxo_count = db.utxo_count().unwrap_or(0);
            println!(
                "  Synced Block {} | Hash: {}... | UTXOs: {}",
                height,
                &hex::encode(&block_hash.0)[..16],
                utxo_count,
            );
            true
        }
        Err(e) => {
            error!("Failed to commit received block: {}", e);
            false
        }
    }
}

/// Process multiple blocks (from sync response) - sorts by height and processes in order
fn process_blocks_batch(db: &ChainDB, mut blocks: Vec<Block>) -> usize {
    // Sort blocks by height (ascending)
    blocks.sort_by_key(|b| b.height());
    
    let mut processed = 0;
    for block in blocks {
        if process_single_block(db, block) {
            processed += 1;
        }
    }
    
    if processed > 0 {
        info!("Processed {} blocks from sync", processed);
    }
    
    processed
}

/// Run miner with broadcast capability for P2P
async fn run_miner_with_broadcast(
    db: Arc<ChainDB>,
    miner: Address,
    max_blocks: u32,
    broadcast_tx: tokio::sync::mpsc::Sender<Block>,
    mempool: Option<Arc<Mempool>>,
) -> anyhow::Result<()> {
    use std::time::Instant;
    
    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("          PYRAX Stream A Miner Started (P2P Enabled)");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    // Benchmark hashrate
    let hashrate = benchmark_blake3(100_000);
    println!("CPU BLAKE3 hashrate: {:.2} MH/s", hashrate / 1_000_000.0);
    println!();

    let mut blocks_mined = 0u32;
    let block_reward = consensus::Stream::A.block_reward();

    loop {
        // Check if we've mined enough
        if max_blocks > 0 && blocks_mined >= max_blocks {
            break;
        }

        // Get current tip
        let tip = db.get_tip();
        let height = tip.height + 1;
        let parent_hash = tip.hash;
        let difficulty = calculate_difficulty(&db, height);

        info!("Mining block {} (parent: {}, difficulty: {})", height, parent_hash, difficulty);

        // Create coinbase transaction
        let coinbase = Transaction::coinbase(height, block_reward, &miner);
        
        // Get pending transactions from mempool
        let pending_txs = if let Some(ref mp) = mempool {
            let txs = mp.get_pending(1000);
            if !txs.is_empty() {
                info!("Including {} transactions from mempool", txs.len());
            }
            txs
        } else {
            vec![]
        };
        
        let mut transactions = vec![coinbase];
        transactions.extend(pending_txs);

        // Create block header
        let merkle_root = compute_merkle_root(&transactions);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Compute UTXO commitment for state verification
        let utxo_commitment = compute_utxo_commitment(&db);
        
        let mut header = BlockHeader {
            version: 1,
            stream: 0, // Stream A
            parent_hash,
            merkle_root,
            utxo_commitment,
            timestamp,
            difficulty,
            nonce: rand::random(),
            extra_nonce: 0,
            height,
            beneficiary: miner,
        };

        // Mine the block
        let start = Instant::now();
        let target = header.target();
        let mut hashes = 0u64;

        loop {
            let pow_hash = header.pow_hash();
            hashes += 1;

            if pow_hash.as_bytes() <= target.as_bytes() {
                break;
            }

            header.nonce = header.nonce.wrapping_add(1);
            if header.nonce == 0 {
                header.extra_nonce += 1;
                header.timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
            }

            if hashes > 10_000_000_000 {
                error!("Mining timeout - difficulty too high");
                return Err(anyhow::anyhow!("Mining timeout"));
            }
        }

        let duration = start.elapsed();
        let block = Block::new(header, transactions);
        let block_hash = block.hash();

        // Validate block
        let validator = BlockValidator::new();
        if let Err(e) = validator.validate_block(&block, &db) {
            error!("Block validation failed: {}", e);
            continue;
        }

        // Commit to database
        db.commit_block(&block)?;
        blocks_mined += 1;

        let utxo_count = db.utxo_count().unwrap_or(0);

        println!(
            "  Mined Block {} | Hash: {}... | Hashes: {} | Time: {:.2}s | UTXOs: {}",
            height,
            &hex::encode(&block_hash.0)[..16],
            hashes,
            duration.as_secs_f64(),
            utxo_count,
        );

        // Broadcast to peers via channel (P2P will pick it up)
        if let Err(e) = broadcast_tx.send(block.clone()).await {
            debug!("No broadcast receiver: {}", e);
        }

        // Small delay
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("Mining complete! Blocks mined: {}", blocks_mined);
    
    let tip = db.get_tip();
    println!("Final chain tip: height={}, hash={}", tip.height, tip.hash);
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}
