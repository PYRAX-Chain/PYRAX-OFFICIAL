//! P2P Networking for PYRAX using libp2p
//!
//! DESIGN DOC - Mesh Networking Architecture
//! ==========================================
//!
//! State Machine:
//! START → DIAL_BOOTNODE → IDENTIFY → BOOTSTRAP_DISCOVERY → FILL_PEERS → MAINTAIN_PEERS (loop)
//!
//! Connection Parameters:
//! - TARGET_PEERS = 50 (steady-state goal)
//! - MIN_PEERS = 30 (dial aggressively below this)
//! - MAX_PEERS = 60 (prune above this)
//! - MAX_CONCURRENT_DIALS = 5 (avoid dial storms)
//! - DIAL_TIMEOUT = 10s
//! - KEEPALIVE_PING_INTERVAL = 15s
//! - PEER_REFRESH_INTERVAL = 30s
//! - PEER_REEVALUATE_INTERVAL = 60s
//!
//! Peer Scoring:
//! - RTT bonus: + (50 - min(RTT_ms, 200)) * 0.1
//! - Uptime bonus: + uptime_minutes * 0.05 (max 10 points)
//! - Disconnect penalty: - disconnects_last_hour * 2
//! - Failure penalty: - failures_last_hour * 1
//! - Subnet diversity penalty: -5 if same /24 has >= 3 peers
//! - Score clamped to [-50, +50]
//!
//! Production-ready P2P layer with:
//! - Kademlia DHT for peer discovery (primary)
//! - GossipSub for block/transaction propagation
//! - mDNS for local peer discovery (LAN)
//! - Ping for keep-alive and RTT measurement
//! - Identify for peer information exchange
//! - Connection Manager for mesh maintenance

mod registry;
mod peer_store;
mod connection_manager;
mod upnp;
mod relay_fallback;
mod peer_cache;
mod reputation;
mod nat_traversal;

// Next-Level Node Features (v0.3.5+)
mod smart_connectivity;
mod self_healing;
mod adaptive_performance;
mod diagnostics;
mod privacy;
mod observability;
mod incentivized_relay;
mod intelligent_peers;
mod edge_computing;

pub use registry::{PeerRegistry, ConnectedPeer, PeerDirection, parse_multiaddr, RegistryMetrics, MeshConnection, RelayCircuit};
pub use peer_store::{PeerStore, PeerStoreConfig, PeerData, PeerStoreMetrics};
pub use connection_manager::{ConnectionManager, ConnectionManagerConfig, ConnectionMetrics, NetworkState, ConnectionEvent};
pub use peer_cache::{PeerCache, CachedPeer};
pub use reputation::{ReputationManager, PeerReputation, GeoRegion, ReputationMetrics, ViolationType, ViolationSeverity};
pub use nat_traversal::{NatTraversalManager, NatType, IceCandidate, stun_discover, StunResult};

// Next-Level Feature Exports
pub use smart_connectivity::{SmartConnectivity, IspType, IspInfo, Protocol, ConnectionQuality, PeerQuality, CaptivePortalStatus};
pub use self_healing::{SelfHealingNetwork, ReconnectionState, RelayCascade, RelayInfo, PartitionDetector, NetworkHealth, KnownGoodPeer};
pub use adaptive_performance::{AdaptivePerformance, SystemCapabilities, PerformanceTier, PowerMode, BandwidthManager, AdaptiveConfig};
pub use diagnostics::{DiagnosticsEngine, DiagnosticReport, DiagnosticIssue, SyncProgress, SyncState, NetworkHealthMetrics, DiagnosticContext, Severity};
pub use privacy::{PrivacyManager, PrivacyLevel, DandelionManager, DandelionConfig, DandelionPhase};
pub use observability::{Observability, MetricsRegistry, HealthChecker, HealthStatus};
pub use incentivized_relay::{IncentivizedRelay, BandwidthAccount, RelayProof, RelaySession, RelayStats};
pub use intelligent_peers::{IntelligentPeerSelector, PeerHistory, PeerFeatures};
pub use edge_computing::{EdgeNode, EdgeConfig, NodeMode, Architecture};

use libp2p::{
    autonat, dcutr, gossipsub, identify, kad, mdns, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm,
    websocket, quic,  // ISP BYPASS: WebSocket and QUIC transports
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn, debug, error};
use futures::StreamExt;

use crate::types::{Block, Transaction, H256, NetworkId, BlockHeader};
use crate::storage::ChainDB;

/// Minimum required client version for network participation
/// Format: (major, minor, patch)
/// Peers with versions below this will be disconnected with a warning
pub const MIN_REQUIRED_VERSION: (u32, u32, u32) = (0, 2, 0);

/// Parse a version string like "pyrax-node/0.2.0" or "rust-libp2p/0.44.2" into (major, minor, patch)
fn parse_version(agent_version: &str) -> Option<(u32, u32, u32)> {
    // Extract version number from agent string
    // Formats: "pyrax-node/0.2.0", "rust-libp2p/0.44.2", "Inferno/0.2.0"
    let version_part = agent_version.split('/').last()?;
    let parts: Vec<&str> = version_part.split('.').collect();
    
    if parts.len() >= 3 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch = parts[2].split('-').next()?.parse().ok()?; // Handle "0.2.0-beta"
        Some((major, minor, patch))
    } else if parts.len() == 2 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        Some((major, minor, 0))
    } else {
        None
    }
}

/// Check if a version meets the minimum required version
fn version_meets_minimum(version: (u32, u32, u32)) -> bool {
    let (major, minor, patch) = version;
    let (min_major, min_minor, min_patch) = MIN_REQUIRED_VERSION;
    
    if major > min_major {
        return true;
    }
    if major < min_major {
        return false;
    }
    // major == min_major
    if minor > min_minor {
        return true;
    }
    if minor < min_minor {
        return false;
    }
    // minor == min_minor
    patch >= min_patch
}

/// Connection mode for mass adoption - allows users behind strict NAT/firewalls to participate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    /// Full node - listens on all interfaces, accepts inbound
    Full,
    /// Relay only - outbound only, uses relay for incoming (for strict NAT/firewalls)
    Relay,
    /// Auto-detect best mode (default)
    #[default]
    Auto,
    /// RelayFirst - connect via relay FIRST, then try direct connection upgrade
    /// This is ideal for users behind ISPs that block P2P or have strict NAT
    /// Provides immediate connectivity while attempting to upgrade to direct
    RelayFirst,
}

impl ConnectionMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "full" => ConnectionMode::Full,
            "relay" => ConnectionMode::Relay,
            "relayfirst" | "relay_first" | "relay-first" => ConnectionMode::RelayFirst,
            _ => ConnectionMode::Auto,
        }
    }
    
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionMode::Full => "full",
            ConnectionMode::Relay => "relay",
            ConnectionMode::Auto => "auto",
            ConnectionMode::RelayFirst => "relayfirst",
        }
    }
}

/// P2P network configuration with mesh networking parameters
#[derive(Debug, Clone)]
pub struct P2PConfig {
    /// Listen address (multiaddr format)
    pub listen_addr: String,
    /// Bootstrap peer addresses
    pub bootstrap_peers: Vec<String>,
    /// Target number of peers (steady-state)
    pub target_peers: usize,
    /// Minimum peers (dial aggressively below this)
    pub min_peers: usize,
    /// Maximum peers (prune above this)  
    pub max_peers: usize,
    /// Maximum concurrent dial attempts
    pub max_concurrent_dials: usize,
    /// Dial timeout in seconds
    pub dial_timeout_secs: u64,
    /// Ping interval in seconds
    pub ping_interval_secs: u64,
    /// Peer refresh interval in seconds
    pub peer_refresh_interval_secs: u64,
    /// Peer reevaluation interval in seconds
    pub peer_reevaluate_interval_secs: u64,
    /// Path to persistent node key file (if None, generates ephemeral key)
    pub node_key_path: Option<PathBuf>,
    
    // === MASS ADOPTION NETWORK SETTINGS ===
    /// Connection mode: Full (requires port forwarding), Relay (works anywhere), Auto, RelayFirst
    pub connection_mode: ConnectionMode,
    /// Enable WebSocket transport (works through proxies and strict firewalls)
    pub enable_websocket: bool,
    /// Enable QUIC transport (UDP-based, hard for ISPs to block)
    pub enable_quic: bool,
    /// WebSocket listen port (default: TCP port + 1)
    pub websocket_port: Option<u16>,
    /// QUIC listen port (default: same as TCP port)
    pub quic_port: Option<u16>,
    /// Enable automatic port fallback (tries alternative ports if primary is blocked)
    pub auto_port_fallback: bool,
    /// Alternative ports to try if primary is blocked (in order of preference)
    pub fallback_ports: Vec<u16>,
}

impl Default for P2PConfig {
    fn default() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/30303".to_string(),
            bootstrap_peers: vec![],
            target_peers: 50,
            min_peers: 30,
            max_peers: 60,
            max_concurrent_dials: 5,
            // NETWORK STABILITY: Increased timeouts and intervals to prevent premature disconnections
            dial_timeout_secs: 15,
            ping_interval_secs: 45,  // Was 15s - reduced ping frequency
            peer_refresh_interval_secs: 120,  // Was 30s - less aggressive refresh
            peer_reevaluate_interval_secs: 300,  // Was 60s - less frequent re-evaluation
            node_key_path: None,
            // Mass adoption defaults - Auto mode for best compatibility
            connection_mode: ConnectionMode::Auto,
            enable_websocket: true,
            enable_quic: true,
            websocket_port: None,  // Will use TCP port + 1 by default
            quic_port: None,       // Will use same as TCP port by default
            auto_port_fallback: true,
            // Stealth ports that bypass ISP blocks (443=HTTPS, 8080=alt HTTP, 8443=alt HTTPS)
            fallback_ports: vec![443, 8080, 8443, 9999],
        }
    }
}

/// Load an existing Ed25519 keypair from file, or generate and save a new one.
/// This ensures the node has a persistent peer ID across restarts.
pub fn load_or_generate_keypair(path: &PathBuf) -> anyhow::Result<libp2p::identity::Keypair> {
    use std::fs;
    
    if path.exists() {
        // Load existing keypair
        let key_bytes = fs::read(path)?;
        let keypair = libp2p::identity::Keypair::ed25519_from_bytes(key_bytes.clone())
            .map_err(|e| anyhow::anyhow!("Failed to parse keypair from {}: {}", path.display(), e))?;
        info!("Loaded existing node key from {}", path.display());
        Ok(keypair)
    } else {
        // Generate new keypair
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        
        // Extract raw Ed25519 secret key bytes (32 bytes)
        let ed25519_keypair = keypair.clone().try_into_ed25519()
            .map_err(|e| anyhow::anyhow!("Failed to extract Ed25519 keypair: {}", e))?;
        let secret_bytes = ed25519_keypair.secret().as_ref().to_vec();
        
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Save to file
        fs::write(path, &secret_bytes)?;
        
        // Set restrictive permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_mode(0o600); // Owner read/write only
            fs::set_permissions(path, perms)?;
        }
        
        info!("Generated new node key and saved to {}", path.display());
        Ok(keypair)
    }
}

/// Messages broadcast over gossipsub
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GossipMessage {
    /// New block announcement
    NewBlock(Block),
    /// New transaction announcement
    NewTransaction(Transaction),
    /// Block header announcement (for initial sync)
    NewHeader(BlockHeader),
    /// Request blocks from a height range
    GetBlocks { start_height: u64, count: u64 },
    /// Response with blocks
    Blocks(Vec<Block>),
}

/// Request/Response protocol messages
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SyncRequest {
    /// Request block by hash
    GetBlock(H256),
    /// Request blocks by height range
    GetBlocks { start: u64, count: u32 },
    /// Request headers by height range
    GetHeaders { start: u64, count: u32 },
    /// Get peer's chain tip
    GetStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SyncResponse {
    Block(Option<Block>),
    Blocks(Vec<Block>),
    Headers(Vec<BlockHeader>),
    Status { height: u64, hash: H256, total_difficulty: u64 },
}

/// Combined network behaviour using libp2p macros
#[derive(NetworkBehaviour)]
pub struct PyraxBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub identify: identify::Behaviour,
    pub ping: ping::Behaviour,
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    pub relay_server: relay::Behaviour,
    pub relay_client: relay::client::Behaviour,
    /// AutoNAT for automatic NAT detection - determines if we're behind NAT
    pub autonat: autonat::Behaviour,
    /// DCUtR for Direct Connection Upgrade through Relay (hole-punching)
    pub dcutr: dcutr::Behaviour,
}

/// P2P Network manager with mesh networking support
pub struct Network {
    /// Local peer ID
    local_peer_id: PeerId,
    /// libp2p swarm
    swarm: Swarm<PyraxBehaviour>,
    /// Chain database
    db: Arc<ChainDB>,
    /// Network identifier
    network_id: NetworkId,
    /// Connection manager for mesh maintenance
    conn_manager: ConnectionManager,
    /// Receiver for connection manager events
    conn_event_rx: mpsc::Receiver<ConnectionEvent>,
    /// Legacy peer registry (for RPC compatibility)
    peer_registry: PeerRegistry,
    /// Bootstrap peer addresses
    bootstrap_peers: Vec<String>,
    /// Currently dialing peers (to avoid duplicate dials)
    dialing: HashSet<PeerId>,
    /// Peers pending disconnection
    pending_disconnect: HashSet<PeerId>,
    /// Configuration
    config: P2PConfig,
    /// Channels for received blocks/txs
    block_tx: mpsc::Sender<Block>,
    block_rx: Option<mpsc::Receiver<Block>>,
    tx_tx: mpsc::Sender<Transaction>,
    tx_rx: Option<mpsc::Receiver<Transaction>>,
    /// Metrics
    metrics: NetworkMetrics,
    /// MESH FIX: Track peers that have subscribed to our topics (for proper mesh formation)
    /// Key: topic hash, Value: set of peer IDs subscribed to that topic
    topic_peers: HashMap<String, HashSet<PeerId>>,
    /// STABILITY FIX: Track last successful ping time per peer for liveness detection
    last_ping_success: HashMap<PeerId, Instant>,
    /// ASIC FIX: Track last ping SENT time to deduplicate ping requests
    /// Prevents ping storms where same peer is pinged multiple times per second
    last_ping_sent: HashMap<PeerId, Instant>,
    /// STABILITY FIX: Track bootnode peer IDs for priority reconnection
    bootnode_peer_ids: HashSet<PeerId>,
    /// UPnP manager for automatic NAT port mapping
    upnp_manager: Option<upnp::UPnPManager>,
    /// TURN-like relay fallback manager for ultimate NAT traversal
    relay_manager: relay_fallback::RelayFallbackManager,
    /// MESH FIX: Track consecutive empty mesh heartbeats for subscription retry
    empty_mesh_count: u32,
    /// MESH FIX: Track if initial subscription announcement has been sent after first bootnode connection
    initial_subscription_sent: bool,
    /// STALE DATA FIX: LRU cache of recently seen block hashes to prevent duplicate processing
    seen_blocks: std::collections::VecDeque<H256>,
    /// STALE DATA FIX: LRU cache of recently seen transaction hashes
    seen_txs: std::collections::VecDeque<H256>,
    /// VISUALIZER FIX: Track active relay circuits for visualization
    /// Key: (src_peer, dst_peer), Value: established_at timestamp
    active_relay_circuits: HashMap<(PeerId, PeerId), u64>,
    /// PERSISTENT PEER CACHE: Save/load known peers for faster reconnection
    peer_cache: Option<peer_cache::PeerCache>,
}

/// NAT status for tracking reachability
#[derive(Debug, Clone, Default, PartialEq)]
pub enum NatStatus {
    #[default]
    Unknown,
    Public,
    Private,
}

/// Network metrics for monitoring
#[derive(Debug, Clone, Default)]
pub struct NetworkMetrics {
    pub connected_peers: usize,
    pub inbound_peers: usize,
    pub outbound_peers: usize,
    pub target_peers: usize,
    pub dial_attempts: u64,
    pub dial_successes: u64,
    pub dial_failures: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub blocks_received: u64,
    pub txs_received: u64,
    pub average_rtt_ms: Option<u64>,
    pub state: NetworkState,
    pub nat_status: NatStatus,
    pub relay_reservations: usize,
    pub hole_punch_successes: u64,
    pub hole_punch_failures: u64,
}

impl Network {
    /// Create a new P2P network with mesh networking and connection management
    pub async fn new(config: P2PConfig, db: Arc<ChainDB>, network_id: NetworkId, peer_registry: PeerRegistry) -> anyhow::Result<Self> {
        info!("╔═══════════════════════════════════════════════════════════════╗");
        info!("║     PYRAX P2P Network - Mesh Networking Initialized           ║");
        info!("║   Target: {} peers | Min: {} | Max: {}                    ║", 
            config.target_peers, config.min_peers, config.max_peers);
        info!("╚═══════════════════════════════════════════════════════════════╝");
        info!("Initializing P2P network for {}", network_id.name());

        // Load or generate keypair (persistent if path provided)
        let local_key = if let Some(ref key_path) = config.node_key_path {
            load_or_generate_keypair(key_path)?
        } else {
            info!("No node key path provided, generating ephemeral keypair");
            libp2p::identity::Keypair::generate_ed25519()
        };
        let local_peer_id = PeerId::from(local_key.public());
        info!("Local peer ID: {}", local_peer_id);

        // Build swarm with tokio runtime and relay client for NAT traversal
        let ping_interval = Duration::from_secs(config.ping_interval_secs);
        
        // CAPACITY FIX: Custom yamux config with increased stream limits
        // Default is 128 streams which causes "Dropping inbound stream because we are at capacity"
        // Increased to 1024 to handle high peer activity
        let yamux_config = {
            let mut cfg = yamux::Config::default();
            cfg.set_max_num_streams(1024); // Was 128, now 1024
            cfg
        };
        
        let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            // Primary transport: TCP (works on most networks)
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                move || yamux_config.clone(),
            )?
            // Add relay client transport - enables nodes behind NAT to be reachable via relay circuits
            .with_relay_client(
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|key, relay_client| {
                // GossipSub config - optimized for blockchain propagation
                // Constraints: mesh_n_low <= mesh_n <= mesh_n_high
                // MESH FIX: Increased parameters to ensure mesh forms even with few peers
                // Previous values (2,4,8) were too restrictive for small networks
                // MESH FORMATION FIX: More aggressive settings for reliable mesh
                // Previous issue: Only 1 mesh peer despite 8 gossip peers
                // CONNECTIVITY FIX v0.3.6: Lower mesh thresholds for small networks
                // Previous issue: mesh_n_low=4 caused "Mesh low" when only 3 peers available
                // This reset mesh to {} even though peers were connected
                // FIX: Allow mesh to form with as few as 2 peers
                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_millis(700)) // Fast heartbeat for quick mesh formation
                    .validation_mode(gossipsub::ValidationMode::Permissive)
                    .max_transmit_size(2 * 1024 * 1024) // 2MB for blocks
                    .mesh_n_low(2)      // CONNECTIVITY FIX: Allow mesh with 2+ peers (was 4)
                    .mesh_n(4)          // CONNECTIVITY FIX: Target 4 peers (was 6)
                    .mesh_n_high(8)     // CONNECTIVITY FIX: Cap at 8 to reduce churn (was 12)
                    .mesh_outbound_min(0) // CRITICAL: Allow inbound-only mesh (for relay connections)
                    .gossip_lazy(4)     // Lazy gossip for propagation
                    .gossip_factor(0.25) // Moderate gossip factor
                    .flood_publish(true) // Ensures delivery even with sparse mesh
                    .history_length(6)  // Keep message history
                    .history_gossip(3)  // Gossip to peers
                    .opportunistic_graft_ticks(2) // Fast mesh recovery - graft after 2 heartbeats
                    .graft_flood_threshold(Duration::from_secs(10)) // Prevent graft flooding
                    .build()
                    .expect("Valid gossipsub config");

                let gossipsub = gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                ).expect("Valid gossipsub behaviour");

                // mDNS for local/LAN discovery
                let mdns = mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    local_peer_id,
                ).expect("Valid mDNS behaviour");

                // Identify protocol - learn peer info
                // CRITICAL: Limit cache size to prevent AutoNAT "len > max when encoding" errors
                // When nodes accumulate many addresses (especially long relay addresses),
                // the AutoNAT dial-back request can exceed protocol message size limits
                // VERSION FIX: Set proper agent version for network version tracking
                let identify = identify::Behaviour::new(
                    identify::Config::new(
                        format!("/pyrax/{}/1.0.0", network_id.name()),
                        key.public(),
                    )
                    .with_agent_version(format!("pyrax-node/{}", env!("CARGO_PKG_VERSION")))
                    .with_cache_size(10) // Limit cached addresses to prevent message overflow
                );

                // Ping for keep-alive and RTT measurement
                // NETWORK STABILITY FIX: Aggressive interval for NAT keepalive
                // - Interval: 20s - keeps NAT mappings alive (most NAT tables timeout at 30-60s)
                // - Timeout: 60s - reasonable timeout for failure detection
                // This prevents NAT-induced disconnections while detecting dead connections promptly
                let ping = ping::Behaviour::new(
                    ping::Config::new()
                        .with_interval(Duration::from_secs(20))  // STABILITY: 20s for aggressive NAT keepalive
                        .with_timeout(Duration::from_secs(60))   // STABILITY: 60s timeout for failure detection
                );

                // Kademlia DHT for peer discovery - primary discovery mechanism
                let store = kad::store::MemoryStore::new(local_peer_id);
                let mut kademlia_config = kad::Config::default();
                kademlia_config.set_protocol_names(vec![
                    libp2p::StreamProtocol::try_from_owned(format!("/pyrax/{}/kad/1.0.0", network_id.name())).unwrap()
                ]);
                // WRONGPEERID FIX: Reduced from 30s to 15s for faster stale entry detection
                // Stale DHT entries cause WrongPeerId errors - faster timeout = faster cleanup
                kademlia_config.set_query_timeout(Duration::from_secs(15));
                kademlia_config.set_replication_factor(std::num::NonZeroUsize::new(20).unwrap());
                // STABILITY FIX: Increased parallelism from 5 to 8 for faster discovery
                kademlia_config.set_parallelism(std::num::NonZeroUsize::new(8).unwrap());
                // WRONGPEERID FIX: Enable record republishing to push fresh data to DHT
                kademlia_config.set_record_ttl(Some(Duration::from_secs(3600))); // 1 hour TTL
                kademlia_config.set_publication_interval(Some(Duration::from_secs(1800))); // Republish every 30 min
                let mut kademlia = kad::Behaviour::with_config(local_peer_id, store, kademlia_config);
                
                // CRITICAL: Set Kademlia to Server mode so nodes can respond to DHT queries
                // Without this, nodes only act as DHT clients and won't serve routing info
                kademlia.set_mode(Some(kad::Mode::Server));

                // Relay SERVER behaviour - allows this node to act as a relay for others
                // BOOTNODE RELAY HARDENING: Massively increased limits for mass adoption
                // Bootnodes need to support thousands of concurrent users behind NAT/firewalls
                // These settings are tuned for production with 5000+ concurrent users
                let relay_config = relay::Config {
                    max_reservations: 4096,           // HARDENED: Slots for peers to register (was 2048)
                    max_circuits: 2048,               // HARDENED: Active relay circuits (was 1024)
                    max_circuits_per_peer: 32,        // HARDENED: Circuits per peer (was 16)
                    reservation_duration: Duration::from_secs(14400), // HARDENED: 4 hours (was 2 hours)
                    max_circuit_duration: Duration::from_secs(14400), // HARDENED: 4 hours
                    max_circuit_bytes: 1024 * 1024 * 50, // HARDENED: 50MB per circuit (was 10MB)
                    ..Default::default()
                };
                let relay_server = relay::Behaviour::new(local_peer_id, relay_config);
                
                // AutoNAT for automatic NAT detection
                // This allows the node to discover if it's behind NAT by asking other peers to dial it
                // HOLE-PUNCH FIX: Optimized settings for faster NAT detection and better hole-punch prep
                let autonat = autonat::Behaviour::new(
                    local_peer_id,
                    autonat::Config {
                        // HOLE-PUNCH FIX: Faster retry for quicker NAT status determination (was 60s)
                        retry_interval: Duration::from_secs(30),
                        // HOLE-PUNCH FIX: Faster refresh for responsive NAT changes (was 30s)
                        refresh_interval: Duration::from_secs(15),
                        // HOLE-PUNCH FIX: Lower confidence for faster status determination (was 3)
                        // 2 probes is enough to confirm NAT status in most cases
                        confidence_max: 2,
                        // Only use public addresses for probes
                        only_global_ips: true,
                        // HOLE-PUNCH FIX: Faster throttle for more responsive probing (was 5s)
                        throttle_server_period: Duration::from_secs(3),
                        // HOLE-PUNCH FIX: Limit boot delay for faster startup
                        boot_delay: Duration::from_secs(5),
                        ..Default::default()
                    },
                );
                
                // DCUtR (Direct Connection Upgrade through Relay) for hole-punching
                // After establishing a relayed connection, this attempts to upgrade to a direct connection
                // HOLE-PUNCH FIX: DCUtR works best when we have accurate NAT status from AutoNAT
                // The faster AutoNAT settings above help DCUtR make better decisions
                let dcutr = dcutr::Behaviour::new(local_peer_id);
                
                info!("P2P behaviours initialized with full NAT traversal (relay + autonat + dcutr)");

                PyraxBehaviour { gossipsub, mdns, identify, ping, kademlia, relay_server, relay_client, autonat, dcutr }
            })?
            // NETWORK STABILITY FIX: Increased idle timeout from 30 minutes to 2 hours
            // This prevents connections from being dropped during low-activity periods
            // Combined with regular ping keepalives, this ensures stable long-running connections
            // Users were experiencing disconnections after ~20 minutes - this fix addresses that
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(7200)))
            .build();

        // Create block/tx channels
        let (block_tx, block_rx) = mpsc::channel(100);
        let (tx_tx, tx_rx) = mpsc::channel(1000);

        // Create connection manager event channel
        let (conn_event_tx, conn_event_rx) = mpsc::channel(100);

        // Create connection manager with proper config
        let conn_manager_config = ConnectionManagerConfig {
            target_peers: config.target_peers,
            min_peers: config.min_peers,
            max_peers: config.max_peers,
            max_concurrent_dials: config.max_concurrent_dials,
            dial_timeout: Duration::from_secs(config.dial_timeout_secs),
            peer_refresh_interval: Duration::from_secs(config.peer_refresh_interval_secs),
            peer_reevaluate_interval: Duration::from_secs(config.peer_reevaluate_interval_secs),
            liveness_check_interval: Duration::from_secs(config.ping_interval_secs),
            min_prune_interval: Duration::from_secs(60),  // NETWORK STABILITY: Increased from 30s
            min_connection_age: Duration::from_secs(180), // NETWORK STABILITY: Increased from 60s - don't prune new connections
        };

        let peer_store_config = PeerStoreConfig::default();
        let mut conn_manager = ConnectionManager::new(conn_manager_config, peer_store_config, conn_event_tx);

        // Register bootstrap peers in connection manager and track their peer IDs
        let mut bootnode_peer_ids = HashSet::new();
        for peer_addr in &config.bootstrap_peers {
            if let Some(peer_id) = Self::extract_peer_id_from_str(peer_addr) {
                conn_manager.add_bootnode(peer_id, vec![peer_addr.clone()]);
                bootnode_peer_ids.insert(peer_id);
                info!("Registered bootnode: {} at {}", peer_id, peer_addr);
            }
        }

        // Set local peer ID in registry
        peer_registry.set_local_peer_id(local_peer_id.to_string()).await;

        let metrics = NetworkMetrics {
            target_peers: config.target_peers,
            state: NetworkState::Starting,
            ..Default::default()
        };
        
        // METRICS FIX: Initialize registry with startup state immediately
        // This ensures RPC/desktop shows "Initializing" instead of "Unknown" on startup
        let initial_metrics = RegistryMetrics {
            inbound_peers: 0,
            outbound_peers: 0,
            target_peers: config.target_peers,
            max_peers: config.max_peers,
            dial_attempts: 0,
            dial_successes: 0,
            dial_failures: 0,
            average_rtt_ms: None,
            network_state: "Starting".to_string(),
            nat_status: "Probing".to_string(),
            mesh_peers: 0,
            gossip_peers: 0,
            mesh_connections: vec![],
            relay_circuits: vec![],
        };
        peer_registry.update_metrics(initial_metrics).await;

        // Initialize UPnP manager for automatic NAT port mapping
        let upnp_manager = Some(upnp::UPnPManager::from_listen_addr(&config.listen_addr));
        
        // Initialize relay fallback manager with bootstrap peers as relays
        let mut relay_manager = relay_fallback::RelayFallbackManager::new();
        for (idx, peer_addr) in config.bootstrap_peers.iter().enumerate() {
            if let Some(peer_id) = Self::extract_peer_id_from_str(peer_addr) {
                relay_manager.add_relay(
                    peer_addr.clone(),
                    peer_id.to_string(),
                    idx == 0, // First bootnode is primary
                );
            }
        }
        
        // PERSISTENT PEER CACHE: Initialize with data directory
        let peer_cache = if let Some(ref key_path) = config.node_key_path {
            if let Some(data_dir) = key_path.parent() {
                let mut cache = peer_cache::PeerCache::new(&data_dir.to_path_buf());
                if let Err(e) = cache.load() {
                    warn!("Failed to load peer cache: {}", e);
                }
                Some(cache)
            } else {
                None
            }
        } else {
            None
        };
        
        Ok(Self {
            local_peer_id,
            swarm,
            db,
            network_id,
            conn_manager,
            conn_event_rx,
            peer_registry,
            bootstrap_peers: config.bootstrap_peers.clone(),
            dialing: HashSet::new(),
            pending_disconnect: HashSet::new(),
            config,
            block_tx,
            block_rx: Some(block_rx),
            tx_tx,
            tx_rx: Some(tx_rx),
            metrics,
            topic_peers: HashMap::new(),
            last_ping_success: HashMap::new(),
            last_ping_sent: HashMap::new(),
            bootnode_peer_ids,
            upnp_manager,
            relay_manager,
            empty_mesh_count: 0,
            initial_subscription_sent: false,
            seen_blocks: std::collections::VecDeque::with_capacity(5000),
            seen_txs: std::collections::VecDeque::with_capacity(10000),
            active_relay_circuits: HashMap::new(),
            peer_cache,
        })
    }

    /// Extract peer ID from multiaddr string
    fn extract_peer_id_from_str(addr: &str) -> Option<PeerId> {
        addr.parse::<Multiaddr>().ok().and_then(|ma| Self::extract_peer_id(&ma))
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Start listening based on connection mode
    /// - Full: Listen on direct address + WebSocket + QUIC + relay fallback
    /// - Relay: Only use relay (no direct listening) - works behind any NAT/firewall
    /// - Auto: Try direct + WebSocket + QUIC, auto-detect if blocked, fallback to relay
    /// - RelayFirst: Connect via relay immediately, attempt direct upgrade in background
    pub fn listen(&mut self, addr: &str) -> anyhow::Result<()> {
        // Extract port from address for WebSocket/QUIC listen addresses
        let tcp_port = Self::extract_port_from_addr(addr).unwrap_or(30303);
        let ws_port = self.config.websocket_port.unwrap_or(tcp_port + 1);
        let quic_port = self.config.quic_port.unwrap_or(tcp_port);
        
        match self.config.connection_mode {
            ConnectionMode::Full => {
                info!("MASS ADOPTION: Full node mode - listening on multiple transports");
                
                // Primary TCP transport
                let multiaddr: Multiaddr = addr.parse()?;
                self.swarm.listen_on(multiaddr)?;
                info!("  ✓ TCP listening on {}", addr);
                
                // WebSocket transport (ISP bypass - looks like HTTP)
                if self.config.enable_websocket {
                    self.listen_websocket(ws_port);
                }
                
                // QUIC transport (ISP bypass - UDP-based, hard to fingerprint)
                if self.config.enable_quic {
                    self.listen_quic(quic_port);
                }
            }
            ConnectionMode::Relay => {
                info!("MASS ADOPTION: Relay-only mode - no direct listening (works behind any NAT/firewall)");
                // Don't listen directly - we'll only connect outbound and use relay
                // This is perfect for users behind strict NAT/firewalls (Xfinity, Comcast, etc.)
            }
            ConnectionMode::RelayFirst => {
                info!("MASS ADOPTION: RelayFirst mode - prioritizing relay for immediate connectivity");
                info!("  → Will connect via relay first for instant network access");
                info!("  → Direct connection upgrade will be attempted in background");
                
                // In RelayFirst mode, we still try to listen on alternative transports
                // as they may work even when TCP is blocked
                if self.config.enable_websocket {
                    self.listen_websocket(ws_port);
                }
                if self.config.enable_quic {
                    self.listen_quic(quic_port);
                }
                
                // TCP listen is attempted but not required
                let multiaddr: Multiaddr = addr.parse()?;
                if let Err(e) = self.swarm.listen_on(multiaddr) {
                    info!("  → TCP listen failed (expected for restricted networks): {:?}", e);
                }
            }
            ConnectionMode::Auto => {
                info!("MASS ADOPTION: Auto mode - trying all transports");
                let multiaddr: Multiaddr = addr.parse()?;
                
                // Try TCP first
                match self.swarm.listen_on(multiaddr) {
                    Ok(_) => {
                        info!("  ✓ TCP direct listen successful on {}", addr);
                    }
                    Err(e) => {
                        warn!("  ✗ TCP direct listen failed: {:?}", e);
                    }
                }
                
                // Always try WebSocket (works through many firewalls)
                if self.config.enable_websocket {
                    self.listen_websocket(ws_port);
                }
                
                // Always try QUIC (UDP-based, different blocking profile)
                if self.config.enable_quic {
                    self.listen_quic(quic_port);
                }
            }
        }
        Ok(())
    }
    
    /// Extract port number from multiaddr string
    fn extract_port_from_addr(addr: &str) -> Option<u16> {
        // Parse /ip4/0.0.0.0/tcp/30303 format
        let parts: Vec<&str> = addr.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            if *part == "tcp" || *part == "udp" {
                if let Some(port_str) = parts.get(i + 1) {
                    return port_str.parse().ok();
                }
            }
        }
        None
    }
    
    /// Listen on WebSocket transport (ISP bypass - looks like HTTP traffic)
    fn listen_websocket(&mut self, port: u16) {
        let ws_addr = format!("/ip4/0.0.0.0/tcp/{}/ws", port);
        match ws_addr.parse::<Multiaddr>() {
            Ok(multiaddr) => {
                match self.swarm.listen_on(multiaddr) {
                    Ok(_) => {
                        info!("  ✓ WebSocket listening on port {} (ISP bypass)", port);
                    }
                    Err(e) => {
                        debug!("  ✗ WebSocket listen failed on port {}: {:?}", port, e);
                    }
                }
            }
            Err(e) => {
                debug!("  ✗ Invalid WebSocket address: {:?}", e);
            }
        }
    }
    
    /// Listen on QUIC transport (ISP bypass - UDP-based, hard to fingerprint)
    fn listen_quic(&mut self, port: u16) {
        let quic_addr = format!("/ip4/0.0.0.0/udp/{}/quic-v1", port);
        match quic_addr.parse::<Multiaddr>() {
            Ok(multiaddr) => {
                match self.swarm.listen_on(multiaddr) {
                    Ok(_) => {
                        info!("  ✓ QUIC listening on UDP port {} (ISP bypass)", port);
                    }
                    Err(e) => {
                        debug!("  ✗ QUIC listen failed on port {}: {:?}", port, e);
                    }
                }
            }
            Err(e) => {
                debug!("  ✗ Invalid QUIC address: {:?}", e);
            }
        }
    }
    
    /// Start listening with automatic port fallback
    /// Tries primary port, then falls back to stealth ports if blocked
    pub fn listen_with_fallback(&mut self, primary_addr: &str) -> anyhow::Result<()> {
        if !self.config.auto_port_fallback {
            return self.listen(primary_addr);
        }
        
        // Try primary address first
        let primary: Multiaddr = primary_addr.parse()?;
        if self.swarm.listen_on(primary.clone()).is_ok() {
            info!("Listening on primary address: {}", primary_addr);
            return Ok(());
        }
        
        // Try fallback ports
        for port in &self.config.fallback_ports.clone() {
            let fallback_addr = format!("/ip4/0.0.0.0/tcp/{}", port);
            if let Ok(multiaddr) = fallback_addr.parse::<Multiaddr>() {
                if self.swarm.listen_on(multiaddr.clone()).is_ok() {
                    info!("MASS ADOPTION: Listening on fallback port {} (primary was blocked)", port);
                    return Ok(());
                }
            }
        }
        
        warn!("All ports blocked - using relay-only mode");
        Ok(())
    }

    /// Listen on relay circuit address to be reachable via relay
    /// This allows nodes behind NAT to receive incoming connections through the relay
    pub fn listen_on_relay(&mut self, relay_addr: &str) -> anyhow::Result<()> {
        let relay_multiaddr: Multiaddr = relay_addr.parse()?;
        
        // Extract the relay peer ID from the address
        if let Some(relay_peer_id) = Self::extract_peer_id(&relay_multiaddr) {
            // Build circuit relay address: /relay_addr/p2p/relay_id/p2p-circuit
            let circuit_addr = relay_multiaddr
                .clone()
                .with(libp2p::multiaddr::Protocol::P2pCircuit);
            
            info!("Requesting relay reservation from {} for NAT traversal", relay_peer_id);
            
            match self.swarm.listen_on(circuit_addr.clone()) {
                Ok(_) => {
                    info!("Listening on relay circuit: {}", circuit_addr);
                }
                Err(e) => {
                    warn!("Failed to listen on relay circuit {}: {:?}", circuit_addr, e);
                }
            }
        } else {
            warn!("Could not extract peer ID from relay address: {}", relay_addr);
        }
        Ok(())
    }

    /// Connect to a peer
    pub fn dial(&mut self, addr: &str) -> anyhow::Result<()> {
        let multiaddr: Multiaddr = addr.parse()?;
        self.swarm.dial(multiaddr.clone())?;
        
        // Extract peer ID from multiaddr if present and add to Kademlia
        // Only add routable addresses to prevent localhost/private IP pollution
        if let Some(peer_id) = Self::extract_peer_id(&multiaddr) {
            if Self::is_routable_address(&multiaddr) {
                self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr);
                info!("Added bootstrap peer {} to Kademlia", peer_id);
            } else {
                debug!("Skipping non-routable bootstrap address for {}: {}", peer_id, multiaddr);
            }
        }
        Ok(())
    }
    
    /// Connect to bootnode and register for relay (for NAT traversal)
    pub fn dial_and_relay(&mut self, addr: &str) -> anyhow::Result<()> {
        // First dial the bootnode
        self.dial(addr)?;
        
        // Then listen on relay circuit through this bootnode
        // This makes us reachable via the relay even if we're behind NAT
        self.listen_on_relay(addr)?;
        
        Ok(())
    }

    /// Extract peer ID from a multiaddr containing /p2p/<peer_id>
    fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
        addr.iter().find_map(|p| {
            if let libp2p::multiaddr::Protocol::P2p(peer_id) = p {
                Some(peer_id)
            } else {
                None
            }
        })
    }

    /// Check if a multiaddr is publicly routable (not localhost or private network)
    /// This prevents address pollution where nodes advertise non-routable addresses
    fn is_routable_address(addr: &Multiaddr) -> bool {
        let mut has_valid_ip = false;
        let mut has_transport = false;
        let mut port: Option<u16> = None;
        
        for protocol in addr.iter() {
            match protocol {
                libp2p::multiaddr::Protocol::Ip4(ip) => {
                    // Reject localhost
                    if ip.is_loopback() {
                        return false;
                    }
                    // Reject private networks (RFC 1918)
                    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                    if ip.is_private() {
                        return false;
                    }
                    // Reject link-local (169.254.0.0/16)
                    if ip.is_link_local() {
                        return false;
                    }
                    // Reject unspecified (0.0.0.0)
                    if ip.is_unspecified() {
                        return false;
                    }
                    has_valid_ip = true;
                }
                libp2p::multiaddr::Protocol::Ip6(ip) => {
                    // Reject localhost
                    if ip.is_loopback() {
                        return false;
                    }
                    // Reject unspecified (::)
                    if ip.is_unspecified() {
                        return false;
                    }
                    has_valid_ip = true;
                }
                libp2p::multiaddr::Protocol::Tcp(p) => {
                    port = Some(p);
                    has_transport = true;
                }
                libp2p::multiaddr::Protocol::Udp(_) => {
                    has_transport = true;
                }
                libp2p::multiaddr::Protocol::Quic | libp2p::multiaddr::Protocol::QuicV1 => {
                    has_transport = true;
                }
                _ => {}
            }
        }
        
        // CRITICAL: Must have BOTH valid IP AND transport protocol
        // Pure /p2p/PEERID addresses without transport info are NOT dialable
        // This was causing MultiaddrNotSupported errors
        if !has_valid_ip || !has_transport {
            return false;
        }
        
        // Reject suspicious ports that are likely ephemeral/random
        // Standard P2P ports are typically in specific ranges
        if let Some(p) = port {
            // Reject ephemeral ports (32768-65535 on most systems)
            // These are outbound connection ports, not listen ports
            if p >= 32768 {
                return false;
            }
            // Also reject very low ports (< 1024) except well-known ones
            // as they require root and are rarely used for P2P
            if p < 1024 && p != 443 && p != 80 {
                return false;
            }
        }
        
        true
    }
    
    /// Check if address is a relay circuit address (always valid for NAT traversal)
    fn is_relay_address(addr: &Multiaddr) -> bool {
        addr.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit))
    }
    
    /// Check if address is a VALID single-hop relay (reject double-hop which causes MultipleCircuitRelayProtocolsUnsupported)
    /// Double-hop: /ip4/.../p2p/.../p2p-circuit/p2p/.../p2p-circuit/p2p/... (two p2p-circuit segments)
    fn is_valid_relay_address(addr: &Multiaddr) -> bool {
        let circuit_count = addr.iter().filter(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit)).count();
        // Valid: 0 (direct) or 1 (single-hop relay)
        // Invalid: 2+ (double-hop relay - not supported)
        circuit_count <= 1
    }
    
    /// WRONGPEERID FIX: Check if an address points to a known bootnode IP
    /// This prevents DHT pollution where peers advertise bootnode IPs with their own peer ID
    fn is_bootnode_ip(addr: &Multiaddr, bootnode_ips: &[std::net::IpAddr]) -> bool {
        for protocol in addr.iter() {
            match protocol {
                libp2p::multiaddr::Protocol::Ip4(ip) => {
                    if bootnode_ips.contains(&std::net::IpAddr::V4(ip)) {
                        return true;
                    }
                }
                libp2p::multiaddr::Protocol::Ip6(ip) => {
                    if bootnode_ips.contains(&std::net::IpAddr::V6(ip)) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    
    /// WRONGPEERID FIX: Validate that an address is safe to add to the DHT
    /// Rejects addresses that point to bootnode IPs with non-bootnode peer IDs
    fn is_valid_dht_address(&self, addr: &Multiaddr, peer_id: &PeerId) -> bool {
        // DOUBLE-HOP FIX: Reject multi-hop relay addresses that cause MultipleCircuitRelayProtocolsUnsupported
        if !Self::is_valid_relay_address(addr) {
            debug!("Rejecting double-hop relay address for {}: {}", peer_id, addr);
            return false;
        }
        
        // Circuit addresses (single-hop) are valid - they go through relay
        if Self::is_relay_address(addr) {
            return true;
        }
        
        // Get bootnode IPs from our bootstrap peers
        let bootnode_ips: Vec<std::net::IpAddr> = self.bootstrap_peers.iter()
            .filter_map(|peer_addr| {
                peer_addr.parse::<Multiaddr>().ok().and_then(|ma| {
                    ma.iter().find_map(|p| match p {
                        libp2p::multiaddr::Protocol::Ip4(ip) => Some(std::net::IpAddr::V4(ip)),
                        libp2p::multiaddr::Protocol::Ip6(ip) => Some(std::net::IpAddr::V6(ip)),
                        _ => None,
                    })
                })
            })
            .collect();
        
        // If address points to a bootnode IP, the peer ID MUST be a bootnode
        if Self::is_bootnode_ip(addr, &bootnode_ips) {
            if !self.bootnode_peer_ids.contains(peer_id) {
                // This is DHT pollution - a non-bootnode peer claiming a bootnode address
                debug!("WRONGPEERID FIX: Rejecting address {} for peer {} - points to bootnode IP but peer is not a bootnode", 
                    addr, peer_id);
                return false;
            }
        }
        
        // Must be routable
        Self::is_routable_address(addr)
    }
    
    /// Filter and prioritize addresses: prefer direct connections over relay
    /// This prevents ResourceLimitExceeded errors by reducing relay usage when direct is available
    fn prioritize_direct_addresses(addrs: Vec<Multiaddr>) -> Vec<Multiaddr> {
        let mut direct_addrs: Vec<Multiaddr> = Vec::new();
        let mut relay_addrs: Vec<Multiaddr> = Vec::new();
        
        for addr in addrs {
            // DOUBLE-HOP FIX: Skip invalid multi-hop relay addresses
            if !Self::is_valid_relay_address(&addr) {
                continue;
            }
            
            if Self::is_relay_address(&addr) {
                relay_addrs.push(addr);
            } else if Self::is_routable_address(&addr) {
                direct_addrs.push(addr);
            }
        }
        
        // If we have direct addresses, use those (limit to 3 best)
        // Only fall back to relay addresses if no direct ones available
        if !direct_addrs.is_empty() {
            // Limit direct addresses to prevent flooding
            direct_addrs.truncate(3);
            direct_addrs
        } else {
            // No direct addresses available, use relay (limit to 2)
            relay_addrs.truncate(2);
            relay_addrs
        }
    }

    /// Bootstrap Kademlia DHT for peer discovery
    pub fn bootstrap_kademlia(&mut self) {
        info!("Starting Kademlia DHT bootstrap for peer discovery...");
        if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
            warn!("Kademlia bootstrap failed: {:?}", e);
        }
    }

    /// Subscribe to gossipsub topics
    pub fn subscribe(&mut self) -> anyhow::Result<()> {
        let blocks_topic = gossipsub::IdentTopic::new(format!("pyrax/{}/blocks", self.network_id.name()));
        let txs_topic = gossipsub::IdentTopic::new(format!("pyrax/{}/txs", self.network_id.name()));

        self.swarm.behaviour_mut().gossipsub.subscribe(&blocks_topic)?;
        self.swarm.behaviour_mut().gossipsub.subscribe(&txs_topic)?;

        info!("Subscribed to gossipsub topics: blocks, txs");
        Ok(())
    }

    /// Log mesh state for debugging - DO NOT modify subscriptions!
    /// CRITICAL: Calling unsubscribe() CLEARS the mesh for that topic!
    /// GossipSub mesh formation happens automatically via heartbeat GRAFT.
    /// We just need to wait for the 1s heartbeat to form the mesh.
    fn log_mesh_state(&self) {
        let connected_count = self.conn_manager.peer_store().connected_peers().len();
        let all_mesh_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_mesh_peers().collect();
        let all_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_peers().collect();
        
        info!("MESH STATE: Connected={}, GossipSub peers={}, Mesh peers={}", 
            connected_count, all_peers.len(), all_mesh_peers.len());
    }

    /// MESH FIX: Check mesh health and trigger subscription re-announcement if needed
    /// Returns true if mesh is healthy (has peers in topics), false otherwise
    fn check_mesh_health(&mut self) -> bool {
        // Use ACTUAL GossipSub mesh state, not our manual tracking
        let all_mesh_peers: Vec<PeerId> = self.swarm.behaviour().gossipsub.all_mesh_peers().cloned().collect();
        let all_gossip_peers: Vec<(PeerId, Vec<gossipsub::TopicHash>)> = self.swarm.behaviour().gossipsub
            .all_peers()
            .map(|(p, topics)| (*p, topics.into_iter().cloned().collect()))
            .collect();
        
        let mesh_size = all_mesh_peers.len();
        let gossip_peer_count = all_gossip_peers.len();
        let connected_count = self.peer_count();
        
        // Log detailed mesh state for debugging
        info!("MESH HEALTH CHECK: mesh={}, gossip_peers={}, connected={}", 
            mesh_size, gossip_peer_count, connected_count);
        
        // Log which peers are subscribed to which topics
        if gossip_peer_count > 0 {
            for (peer, topics) in &all_gossip_peers {
                let topic_names: Vec<String> = topics.iter().map(|t| t.to_string()).collect();
                debug!("  GossipSub peer {}: topics={:?}", peer, topic_names);
            }
        }
        
        // Mesh is healthy if we have at least 1 peer in mesh
        let mesh_healthy = mesh_size > 0;
        
        if mesh_healthy {
            // Reset empty mesh counter when healthy
            self.empty_mesh_count = 0;
            info!("✓ Mesh is HEALTHY with {} peers", mesh_size);
            true
        } else if connected_count > 0 {
            // We have TCP connections but empty GossipSub mesh - this is the bug!
            self.empty_mesh_count += 1;
            
            warn!("⚠ MESH EMPTY despite {} TCP connections, {} GossipSub peers (attempt #{})", 
                connected_count, gossip_peer_count, self.empty_mesh_count);
            
            // DO NOT call force_mesh - unsubscribe() clears the mesh!
            // GossipSub heartbeat (every 1s) will automatically GRAFT peers into mesh
            // Just log the state and wait for natural mesh formation
            if self.empty_mesh_count >= 5 {
                self.log_mesh_state();
                // Reset counter to avoid log spam
                self.empty_mesh_count = 0;
            }
            false
        } else {
            // No peers connected, mesh can't form yet
            debug!("No peers connected yet, mesh cannot form");
            false
        }
    }

    /// Broadcast a block
    pub fn broadcast_block(&mut self, block: &Block) -> anyhow::Result<()> {
        let topic = gossipsub::IdentTopic::new(format!("pyrax/{}/blocks", self.network_id.name()));
        let msg = GossipMessage::NewBlock(block.clone());
        let data = bincode::serialize(&msg)?;
        
        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
            warn!("Failed to publish block: {:?}", e);
        } else {
            debug!("Broadcast block {} at height {}", block.hash(), block.height());
        }
        Ok(())
    }

    /// Request blocks from peers starting at a given height
    pub fn request_blocks(&mut self, start_height: u64, count: u64) -> anyhow::Result<()> {
        let topic = gossipsub::IdentTopic::new(format!("pyrax/{}/blocks", self.network_id.name()));
        let msg = GossipMessage::GetBlocks { start_height, count };
        let data = bincode::serialize(&msg)?;
        
        info!("Sending GetBlocks request: start={}, count={}, topic={}", start_height, count, topic.hash());
        
        match self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
            Ok(_) => {
                info!("Successfully sent GetBlocks request for blocks {}-{}", start_height, start_height + count - 1);
            }
            Err(e) => {
                warn!("Failed to request blocks: {:?}", e);
            }
        }
        Ok(())
    }

    /// Broadcast a transaction
    pub fn broadcast_tx(&mut self, tx: &Transaction) -> anyhow::Result<()> {
        let topic = gossipsub::IdentTopic::new(format!("pyrax/{}/txs", self.network_id.name()));
        let msg = GossipMessage::NewTransaction(tx.clone());
        let data = bincode::serialize(&msg)?;
        
        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
            warn!("Failed to publish tx: {:?}", e);
        }
        Ok(())
    }

    /// Get connected peer count
    pub fn peer_count(&self) -> usize {
        self.conn_manager.peer_store().connected_count()
    }

    /// Get network metrics
    pub fn get_metrics(&self) -> NetworkMetrics {
        self.metrics.clone()
    }

    /// Get network state
    pub fn get_state(&self) -> NetworkState {
        self.conn_manager.state()
    }

    /// Take block receiver channel
    pub fn take_block_receiver(&mut self) -> Option<mpsc::Receiver<Block>> {
        self.block_rx.take()
    }

    /// Take tx receiver channel
    pub fn take_tx_receiver(&mut self) -> Option<mpsc::Receiver<Transaction>> {
        self.tx_rx.take()
    }

    /// Run the network event loop with block broadcast capability
    /// This is the main mesh networking loop with connection management
    pub async fn run_with_broadcast(mut self, mut mined_rx: tokio::sync::mpsc::Receiver<Block>) {
        info!("╔═══════════════════════════════════════════════════════════════╗");
        info!("║     PYRAX P2P Mesh Network Starting (with broadcast)          ║");
        info!("╚═══════════════════════════════════════════════════════════════╝");
        
        // UPnP: Attempt automatic port mapping for NAT traversal
        if let Some(ref mut upnp) = self.upnp_manager {
            if let Some(mapping) = upnp.setup_port_mapping().await {
                info!("✓ UPnP NAT traversal enabled: external {}:{}", 
                    mapping.external_ip, mapping.external_port);
                // Add external address to our listen addresses for advertising
                if let Some(ext_addr) = upnp.get_external_address() {
                    if let Ok(ma) = ext_addr.parse::<Multiaddr>() {
                        self.swarm.add_external_address(ma);
                        info!("Added UPnP external address to swarm");
                    }
                }
            } else {
                info!("UPnP not available - using relay for NAT traversal");
            }
        }
        
        // PERSISTENT PEER CACHE: Dial cached peers before bootnodes for faster reconnection
        if let Some(ref cache) = self.peer_cache {
            let startup_peers = cache.get_startup_peers(20); // Dial up to 20 cached peers
            if !startup_peers.is_empty() {
                info!("Dialing {} cached peers for faster reconnection", startup_peers.len());
                for (peer_id_str, addresses) in startup_peers {
                    if let Ok(peer_id) = peer_id_str.parse::<PeerId>() {
                        if peer_id != self.local_peer_id {
                            for addr in addresses {
                                if let Ok(ma) = addr.parse::<Multiaddr>() {
                                    if Self::is_routable_address(&ma) {
                                        let _ = self.swarm.dial(ma);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Start connection manager - this will dial bootnodes
        self.conn_manager.start().await;
        
        let mut initial_sync_done = false;
        let mut last_requested_height: u64 = 0;
        
        // Timers for mesh maintenance
        let mut conn_manager_tick = tokio::time::interval(Duration::from_secs(1));
        conn_manager_tick.tick().await;
        
        let mut sync_timer = tokio::time::interval(Duration::from_secs(self.config.peer_refresh_interval_secs));
        sync_timer.tick().await;
        
        // METRICS FIX: Fast metrics update (every 3 seconds) for responsive UI
        // This ensures desktop/RPC sees near-realtime network state
        // Combined with immediate updates on connection events for best responsiveness
        let mut metrics_timer = tokio::time::interval(Duration::from_secs(3));
        metrics_timer.tick().await;
        
        // STABILITY FIX: Bootnode connectivity check timer (every 60 seconds)
        let mut bootnode_check_timer = tokio::time::interval(Duration::from_secs(60));
        bootnode_check_timer.tick().await;
        
        // ASIC FIX: UPnP renewal timer (every 15 minutes for better NAT stability)
        // Reduced from 30 minutes to prevent NAT mapping expiration issues
        let mut upnp_renewal_timer = tokio::time::interval(Duration::from_secs(900));
        upnp_renewal_timer.tick().await;
        
        // PERSISTENT PEER CACHE: Save cache every 5 minutes
        let mut peer_cache_timer = tokio::time::interval(Duration::from_secs(300));
        peer_cache_timer.tick().await;
        
        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
                
                // Handle connection manager events
                Some(conn_event) = self.conn_event_rx.recv() => {
                    self.handle_conn_manager_event(conn_event).await;
                }
                
                // Broadcast mined blocks
                Some(block) = mined_rx.recv() => {
                    self.metrics.messages_sent += 1;
                    if let Err(e) = self.broadcast_block(&block) {
                        warn!("Failed to broadcast block {}: {}", block.hash(), e);
                    } else {
                        info!("Broadcast block {} (height {}) to {} peers", 
                            block.hash(), block.height(), self.peer_count());
                    }
                }
                
                // Connection manager tick - maintains mesh health
                _ = conn_manager_tick.tick() => {
                    self.conn_manager.tick().await;
                    
                    // Process any pending disconnects
                    self.process_pending_disconnects();
                }
                
                // Periodic sync check
                _ = sync_timer.tick() => {
                    let peer_count = self.peer_count();
                    if peer_count > 0 {
                        let our_height = self.db.get_tip().height;
                        if !initial_sync_done || our_height >= last_requested_height {
                            let start_height = our_height + 1;
                            debug!("Sync check: requesting blocks from height {} (peers: {})", start_height, peer_count);
                            let _ = self.request_blocks(start_height, 100);
                            last_requested_height = start_height;
                            initial_sync_done = true;
                        }
                    }
                }
                
                // Metrics logging and mesh health check
                _ = metrics_timer.tick() => {
                    self.log_metrics().await;
                    
                    // MESH FIX: Check mesh health and trigger subscription retry if needed
                    self.check_mesh_health();
                }
                
                // STABILITY FIX: Periodic bootnode connectivity check
                _ = bootnode_check_timer.tick() => {
                    self.ensure_bootnode_connectivity().await;
                }
                
                // UPnP renewal - renew port mappings before lease expires
                _ = upnp_renewal_timer.tick() => {
                    if let Some(ref mut upnp) = self.upnp_manager {
                        upnp.renew_mappings().await;
                    }
                }
                
                // PERSISTENT PEER CACHE: Save cache periodically
                _ = peer_cache_timer.tick() => {
                    if let Some(ref mut cache) = self.peer_cache {
                        if let Err(e) = cache.save() {
                            warn!("Failed to save peer cache: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Run the network event loop (without broadcast)
    /// This is the main mesh networking loop with connection management
    pub async fn run(mut self) {
        info!("╔═══════════════════════════════════════════════════════════════╗");
        info!("║     PYRAX P2P Mesh Network Starting                            ║");
        info!("╚═══════════════════════════════════════════════════════════════╝");
        
        // UPnP: Attempt automatic port mapping for NAT traversal
        if let Some(ref mut upnp) = self.upnp_manager {
            if let Some(mapping) = upnp.setup_port_mapping().await {
                info!("✓ UPnP NAT traversal enabled: external {}:{}", 
                    mapping.external_ip, mapping.external_port);
                if let Some(ext_addr) = upnp.get_external_address() {
                    if let Ok(ma) = ext_addr.parse::<Multiaddr>() {
                        self.swarm.add_external_address(ma);
                        info!("Added UPnP external address to swarm");
                    }
                }
            } else {
                info!("UPnP not available - using relay for NAT traversal");
            }
        }
        
        // Start connection manager - this will dial bootnodes
        self.conn_manager.start().await;
        
        let mut initial_sync_done = false;
        let mut last_requested_height: u64 = 0;
        
        // Timers for mesh maintenance
        let mut conn_manager_tick = tokio::time::interval(Duration::from_secs(1));
        conn_manager_tick.tick().await;
        
        let mut sync_timer = tokio::time::interval(Duration::from_secs(self.config.peer_refresh_interval_secs));
        sync_timer.tick().await;
        
        // METRICS FIX: Fast metrics update (every 3 seconds) for responsive UI
        // This ensures desktop/RPC sees near-realtime network state
        // Combined with immediate updates on connection events for best responsiveness
        let mut metrics_timer = tokio::time::interval(Duration::from_secs(3));
        metrics_timer.tick().await;
        
        // STABILITY FIX: Bootnode connectivity check timer (every 60 seconds)
        let mut bootnode_check_timer = tokio::time::interval(Duration::from_secs(60));
        bootnode_check_timer.tick().await;
        
        // ASIC FIX: UPnP renewal timer (every 15 minutes for better NAT stability)
        let mut upnp_renewal_timer = tokio::time::interval(Duration::from_secs(900));
        upnp_renewal_timer.tick().await;
        
        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
                
                // Handle connection manager events
                Some(conn_event) = self.conn_event_rx.recv() => {
                    self.handle_conn_manager_event(conn_event).await;
                }
                
                // Connection manager tick - maintains mesh health
                _ = conn_manager_tick.tick() => {
                    self.conn_manager.tick().await;
                    
                    // Process any pending disconnects
                    self.process_pending_disconnects();
                }
                
                // Periodic sync check
                _ = sync_timer.tick() => {
                    let peer_count = self.peer_count();
                    if peer_count > 0 {
                        let our_height = self.db.get_tip().height;
                        if !initial_sync_done || our_height >= last_requested_height {
                            let start_height = our_height + 1;
                            debug!("Sync check: requesting blocks from height {} (peers: {})", start_height, peer_count);
                            let _ = self.request_blocks(start_height, 100);
                            last_requested_height = start_height;
                            initial_sync_done = true;
                        }
                    }
                }
                
                // Metrics logging and mesh health check
                _ = metrics_timer.tick() => {
                    self.log_metrics().await;
                    
                    // MESH FIX: Check mesh health and trigger subscription retry if needed
                    self.check_mesh_health();
                }
                
                // STABILITY FIX: Periodic bootnode connectivity check
                _ = bootnode_check_timer.tick() => {
                    self.ensure_bootnode_connectivity().await;
                }
                
                // UPnP renewal
                _ = upnp_renewal_timer.tick() => {
                    if let Some(ref mut upnp) = self.upnp_manager {
                        upnp.renew_mappings().await;
                    }
                }
            }
        }
    }

    /// Handle swarm events and update connection manager
    async fn handle_swarm_event(&mut self, event: SwarmEvent<PyraxBehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(behaviour_event) => {
                self.handle_behaviour_event(behaviour_event).await;
            }
            SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                let addr_str = endpoint.get_remote_address().to_string();
                let (ip, port) = parse_multiaddr(&addr_str);
                let is_inbound = !endpoint.is_dialer();
                let direction = if is_inbound { PeerDirection::Inbound } else { PeerDirection::Outbound };
                
                // Update connection manager
                self.conn_manager.on_connection_established(peer_id, addr_str.clone(), is_inbound).await;
                self.dialing.remove(&peer_id);
                
                // Update legacy registry for RPC compatibility
                self.peer_registry.add_peer(ConnectedPeer {
                    peer_id: peer_id.to_string(),
                    address: addr_str.clone(),
                    ip,
                    port,
                    direction,
                    connected_at: Instant::now(),
                    last_seen: Instant::now(),
                    client_version: format!("pyrax-node/{}", env!("CARGO_PKG_VERSION")),
                    best_height: 0,
                }).await;
                
                // Add to Kademlia for discovery (only routable or valid single-hop relay addresses)
                // This prevents localhost/private IP pollution that causes WrongPeerId errors
                // Also prevents double-hop relay addresses that cause MultipleCircuitRelayProtocolsUnsupported
                if let Ok(addr) = addr_str.parse::<Multiaddr>() {
                    // DOUBLE-HOP FIX: Reject multi-hop relay addresses
                    if !Self::is_valid_relay_address(&addr) {
                        debug!("Skipping double-hop relay address for peer {}: {}", peer_id, addr_str);
                    } else if Self::is_routable_address(&addr) || Self::is_relay_address(&addr) {
                        self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                    } else {
                        debug!("Skipping non-routable address for peer {}: {}", peer_id, addr_str);
                    }
                }
                
                // Check if this is a bootnode
                let is_bootnode = self.bootnode_peer_ids.contains(&peer_id);
                
                // HEALTH MONITORING: Track if this is a relay connection
                let is_relay = Self::is_relay_address(&addr_str.parse::<Multiaddr>().unwrap_or_else(|_| "/ip4/0.0.0.0/tcp/0".parse().unwrap()));
                if let Some(peer) = self.conn_manager.peer_store_mut().get_peer_mut(&peer_id) {
                    peer.is_relay_connection = is_relay;
                }
                
                // MESH FIX v2: Do NOT add ANY peers as explicit peers!
                // Explicit peers are intentionally OUTSIDE the mesh - GossipSub ignores GRAFT from them.
                // This was the ROOT CAUSE of mesh not forming: "GRAFT: ignoring request from direct peer"
                // Let ALL peers (including bootnodes) join mesh naturally via SUBSCRIBE → GRAFT flow.
                if is_bootnode {
                    info!("✓ BOOTNODE {} connected - will join mesh via normal GRAFT protocol", peer_id);
                }
                
                // STABILITY FIX: Initialize last ping success time
                self.last_ping_success.insert(peer_id, Instant::now());
                
                // PERSISTENT PEER CACHE: Update cache with successful connection
                // We'll update with addresses from the Identify protocol later when we receive them
                if let Some(ref mut cache) = self.peer_cache {
                    // For now, just record the peer with empty addresses
                    // Addresses will be added when Identify protocol completes
                    cache.upsert_peer(peer_id.to_string(), vec![], None, is_bootnode);
                }
                
                // Update metrics - now async for real-time UI updates
                self.update_metrics().await;
                
                let peer_count = self.peer_count();
                info!("✓ Peer {} connected ({}) [{}/{}]{}", 
                    peer_id, direction, peer_count, self.config.target_peers,
                    if is_bootnode { " [BOOTNODE]" } else { "" });
            }
            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                // STABILITY FIX: Connection manager now handles deduplication internally
                // This prevents cascade effects from multiple ConnectionClosed events for same peer
                self.conn_manager.on_connection_closed(peer_id).await;
                
                // Only process if this is a real disconnect (not a duplicate event)
                // Check if peer is still marked as connected in conn_manager
                let is_still_connected = self.conn_manager.peer_store().get_peer(&peer_id)
                    .map(|p| p.state == peer_store::PeerState::Connected)
                    .unwrap_or(false);
                
                if is_still_connected {
                    // This was a duplicate event, connection manager already handled it
                    debug!("Skipping duplicate disconnect cleanup for {}", peer_id);
                    return;
                }
                
                // Update legacy registry
                self.peer_registry.remove_peer(&peer_id.to_string()).await;
                
                // MESH FIX: Remove peer from all topic_peers sets
                for peers in self.topic_peers.values_mut() {
                    peers.remove(&peer_id);
                }
                
                // STABILITY FIX: Clean up ping tracking
                self.last_ping_success.remove(&peer_id);
                
                // Update metrics - now async for real-time UI updates
                self.update_metrics().await;
                
                let peer_count = self.peer_count();
                let is_bootnode = self.bootnode_peer_ids.contains(&peer_id);
                
                if is_bootnode {
                    warn!("⚠ BOOTNODE {} disconnected! Will attempt reconnection. ({:?})", peer_id, cause);
                } else {
                    info!("✗ Peer {} disconnected ({:?}) [{}/{}]", 
                        peer_id, cause, peer_count, self.config.target_peers);
                }
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, connection_id } => {
                if let Some(peer_id) = peer_id {
                    self.conn_manager.on_dial_failure(peer_id).await;
                    self.dialing.remove(&peer_id);
                    self.metrics.dial_failures += 1;
                    
                    // WRONGPEERID FIX: Detect WrongPeerId errors and clean up stale Kademlia entries
                    // This happens when a peer moved to a different IP but Kademlia still has old mapping
                    let error_str = format!("{:?}", error);
                    if error_str.contains("WrongPeerId") {
                        warn!("STALE ENTRY: WrongPeerId for {} - removing from Kademlia", peer_id);
                        // Remove ALL addresses for this peer from Kademlia since the mapping is stale
                        self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                        // Also increase backoff significantly for this peer
                        self.conn_manager.on_wrong_peer_id(peer_id).await;
                    } else if error_str.contains("ResourceLimitExceeded") {
                        // Relay circuit limit hit - don't penalize the peer, just back off
                        debug!("Relay limit exceeded for {} - will retry later", peer_id);
                    } else {
                        debug!("Dial to {} failed: {:?}", peer_id, error);
                    }
                }
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                // CRITICAL: Only advertise routable addresses to prevent localhost/private IP pollution
                // Non-routable addresses cause WrongPeerId errors when other peers try to dial them
                if Self::is_routable_address(&address) {
                    info!("Listening on {}/p2p/{}", address, self.local_peer_id);
                    self.peer_registry.add_listen_address(format!("{}/p2p/{}", address, self.local_peer_id)).await;
                } else {
                    debug!("Not advertising non-routable listen address: {}", address);
                }
            }
            SwarmEvent::IncomingConnection { local_addr, send_back_addr, .. } => {
                debug!("Incoming connection from {} to {}", send_back_addr, local_addr);
            }
            SwarmEvent::IncomingConnectionError { local_addr, send_back_addr, error, .. } => {
                debug!("Incoming connection error from {} to {}: {:?}", send_back_addr, local_addr, error);
            }
            _ => {}
        }
    }

    /// Handle connection manager events (dial requests, prune requests, etc.)
    async fn handle_conn_manager_event(&mut self, event: ConnectionEvent) {
        match event {
            ConnectionEvent::DialPeers(peers) => {
                for (peer_id, addr) in peers {
                    // CRITICAL: Prevent self-dial (LocalPeerId error)
                    if peer_id == self.local_peer_id {
                        debug!("Skipping self-dial attempt");
                        continue;
                    }
                    if self.dialing.contains(&peer_id) {
                        continue;
                    }
                    
                    // FIX: Pre-dial health check - skip peers with recent dial failures
                    // This prevents wasting connection slots on stale Kademlia entries
                    if self.conn_manager.should_skip_dial(&peer_id).await {
                        debug!("Skipping dial to {} - recent failures (stale entry)", peer_id);
                        continue;
                    }
                    
                    if let Ok(multiaddr) = addr.parse::<Multiaddr>() {
                        // CRITICAL: Final validation - reject non-routable addresses
                        // This is defense-in-depth against WrongPeerId errors
                        if !Self::is_routable_address(&multiaddr) {
                            debug!("Rejecting non-routable dial address for {}: {}", peer_id, addr);
                            continue;
                        }
                        
                        self.dialing.insert(peer_id);
                        self.metrics.dial_attempts += 1;
                        if let Err(e) = self.swarm.dial(multiaddr) {
                            debug!("Failed to dial {}: {:?}", peer_id, e);
                            self.dialing.remove(&peer_id);
                            self.conn_manager.on_dial_failure(peer_id).await;
                        }
                    }
                }
            }
            ConnectionEvent::DisconnectPeers(peers) => {
                for peer_id in peers {
                    self.pending_disconnect.insert(peer_id);
                }
            }
            ConnectionEvent::TriggerKademliaBootstrap => {
                info!("Triggering Kademlia DHT bootstrap...");
                if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
                    warn!("Kademlia bootstrap failed: {:?}", e);
                }
            }
            ConnectionEvent::TriggerKademliaQuery => {
                // Query for random peer IDs to discover more peers
                let random_peer_id = PeerId::random();
                self.swarm.behaviour_mut().kademlia.get_closest_peers(random_peer_id);
            }
            ConnectionEvent::StateChanged(new_state) => {
                info!("Network state: {:?}", new_state);
                self.metrics.state = new_state;
            }
            ConnectionEvent::MetricsUpdate(conn_metrics) => {
                self.metrics.connected_peers = conn_metrics.connected_peers;
                self.metrics.inbound_peers = conn_metrics.inbound_peers;
                self.metrics.outbound_peers = conn_metrics.outbound_peers;
                self.metrics.average_rtt_ms = conn_metrics.average_rtt_ms;
            }
        }
    }

    /// Process pending disconnect requests
    fn process_pending_disconnects(&mut self) {
        for peer_id in self.pending_disconnect.drain() {
            if let Err(e) = self.swarm.disconnect_peer_id(peer_id) {
                debug!("Failed to disconnect {}: {:?}", peer_id, e);
            } else {
                info!("Pruned peer {} (score-based)", peer_id);
            }
        }
    }

    /// Update metrics from connection manager and push to peer registry immediately
    /// METRICS FIX: Now pushes to peer_registry on every connection event for real-time UI updates
    async fn update_metrics(&mut self) {
        let conn_metrics = self.conn_manager.metrics();
        self.metrics.connected_peers = conn_metrics.connected_peers;
        self.metrics.inbound_peers = conn_metrics.inbound_peers;
        self.metrics.outbound_peers = conn_metrics.outbound_peers;
        self.metrics.dial_attempts = conn_metrics.dial_attempts;
        self.metrics.dial_successes = conn_metrics.dial_successes;
        self.metrics.dial_failures = conn_metrics.dial_failures;
        self.metrics.average_rtt_ms = conn_metrics.average_rtt_ms;
        self.metrics.state = conn_metrics.state;
        
        // METRICS FIX: Push to peer registry immediately so RPC/desktop sees updates in real-time
        // Previously this only happened every 10 seconds in log_metrics()
        let nat_str = match self.metrics.nat_status {
            NatStatus::Public => "PUBLIC",
            NatStatus::Private => "PRIVATE",
            NatStatus::Unknown => "UNKNOWN",
        };
        
        // Get mesh/gossip peer counts from GossipSub
        let mesh_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_mesh_peers().collect();
        let gossip_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_peers().collect();
        
        let registry_metrics = RegistryMetrics {
            inbound_peers: self.metrics.inbound_peers,
            outbound_peers: self.metrics.outbound_peers,
            target_peers: self.metrics.target_peers,
            max_peers: self.config.max_peers,
            dial_attempts: self.metrics.dial_attempts,
            dial_successes: self.metrics.dial_successes,
            dial_failures: self.metrics.dial_failures,
            average_rtt_ms: self.metrics.average_rtt_ms,
            network_state: format!("{:?}", self.metrics.state),
            nat_status: nat_str.to_string(),
            mesh_peers: mesh_peers.len(),
            gossip_peers: gossip_peers.len(),
            // Mesh connections updated in full log_metrics() for performance
            mesh_connections: vec![],
            relay_circuits: vec![],
        };
        self.peer_registry.update_metrics(registry_metrics).await;
    }

    /// STABILITY FIX: Ensure we maintain connectivity to at least one bootnode
    /// This prevents the network from disappearing if all bootnodes disconnect
    async fn ensure_bootnode_connectivity(&mut self) {
        // Check if we're connected to any bootnode
        let connected_bootnodes: Vec<PeerId> = self.bootnode_peer_ids.iter()
            .filter(|peer_id| {
                self.conn_manager.peer_store().get_peer(peer_id)
                    .map(|p| p.state == peer_store::PeerState::Connected)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        
        if connected_bootnodes.is_empty() && !self.bootnode_peer_ids.is_empty() {
            warn!("⚠ Lost connection to ALL bootnodes! Initiating reconnection...");
            
            // Re-dial all bootnodes
            for bootnode_addr in &self.bootstrap_peers.clone() {
                if let Some(peer_id) = Self::extract_peer_id_from_str(bootnode_addr) {
                    // SELF-DIAL FIX: Skip if this is our own peer ID
                    if peer_id == self.local_peer_id {
                        debug!("Skipping self in bootnode reconnection");
                        continue;
                    }
                    
                    // Skip if already dialing
                    if self.dialing.contains(&peer_id) {
                        continue;
                    }
                    
                    info!("Reconnecting to bootnode {} at {}", peer_id, bootnode_addr);
                    
                    // First try direct dial
                    if let Ok(multiaddr) = bootnode_addr.parse::<Multiaddr>() {
                        self.dialing.insert(peer_id);
                        if let Err(e) = self.swarm.dial(multiaddr.clone()) {
                            warn!("Failed to dial bootnode {}: {:?}", peer_id, e);
                            self.dialing.remove(&peer_id);
                        }
                    }
                    
                    // Also try to listen on relay through this bootnode (for NAT traversal)
                    let _ = self.listen_on_relay(bootnode_addr);
                }
            }
            
            // Trigger Kademlia bootstrap to rediscover the network
            if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
                warn!("Kademlia re-bootstrap failed: {:?}", e);
            }
        } else if !connected_bootnodes.is_empty() {
            debug!("Bootnode connectivity OK: {} bootnodes connected", connected_bootnodes.len());
        }
        
        // NETWORK STABILITY: Check for stale connections (no successful ping in 15 minutes)
        // Extended from 5 minutes to be more tolerant of slow/relay connections
        let stale_threshold = Duration::from_secs(900);
        let now = Instant::now();
        let mut stale_peers = Vec::new();
        
        for (peer_id, last_ping) in &self.last_ping_success {
            if now.duration_since(*last_ping) > stale_threshold {
                // Don't disconnect bootnodes, just log warning
                if self.bootnode_peer_ids.contains(peer_id) {
                    warn!("Bootnode {} has stale connection (no ping in {:?})", peer_id, stale_threshold);
                } else {
                    stale_peers.push(*peer_id);
                }
            }
        }
        
        // Schedule stale non-bootnode peers for disconnection
        for peer_id in stale_peers {
            debug!("Scheduling stale peer {} for disconnection", peer_id);
            self.pending_disconnect.insert(peer_id);
        }
    }

    /// Log current network metrics with detailed mesh state
    /// Also updates the peer registry with current metrics for RPC access
    async fn log_metrics(&mut self) {
        let m = &self.metrics;
        let nat_str = match m.nat_status {
            NatStatus::Public => "PUBLIC (directly reachable)",
            NatStatus::Private => "PRIVATE (using relay)",
            NatStatus::Unknown => "UNKNOWN (probing...)",
        };
        
        // Get actual GossipSub mesh state
        let mesh_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_mesh_peers().collect();
        let gossip_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_peers().collect();
        let topics: Vec<_> = self.swarm.behaviour().gossipsub.topics().collect();
        
        // Build mesh connections for visualizer
        // This shows which peers are connected via shared mesh topics
        let mut mesh_connections = Vec::new();
        let local_peer_id = self.swarm.local_peer_id().to_string();
        
        // For each topic, create connections between this node and mesh peers
        for topic in &topics {
            let topic_str = topic.to_string();
            // Get peers in mesh for this specific topic
            let topic_mesh: Vec<_> = self.swarm.behaviour().gossipsub.mesh_peers(topic).collect();
            
            for peer_id in topic_mesh {
                mesh_connections.push(MeshConnection {
                    peer_a: local_peer_id.clone(),
                    peer_b: peer_id.to_string(),
                    topic: topic_str.clone(),
                    connection_type: "mesh".to_string(),
                });
            }
        }
        
        // Also add gossip-only connections (subscribed but not in mesh)
        for (peer_id, _topics) in &gossip_peers {
            let peer_str = peer_id.to_string();
            // Check if already in mesh connections
            let in_mesh = mesh_connections.iter().any(|c| c.peer_b == peer_str);
            if !in_mesh {
                mesh_connections.push(MeshConnection {
                    peer_a: local_peer_id.clone(),
                    peer_b: peer_str,
                    topic: "gossip".to_string(),
                    connection_type: "gossip".to_string(),
                });
            }
        }
        
        // Update registry with extended P2P metrics for RPC/dashboard
        let registry_metrics = RegistryMetrics {
            inbound_peers: m.inbound_peers,
            outbound_peers: m.outbound_peers,
            target_peers: m.target_peers,
            max_peers: self.config.max_peers,
            dial_attempts: m.dial_attempts,
            dial_successes: m.dial_successes,
            dial_failures: m.dial_failures,
            average_rtt_ms: m.average_rtt_ms,
            network_state: format!("{:?}", m.state),
            nat_status: nat_str.to_string(),
            mesh_peers: mesh_peers.len(),
            gossip_peers: gossip_peers.len(),
            mesh_connections,
            relay_circuits: self.active_relay_circuits.iter().map(|((src, dst), established_at)| {
                registry::RelayCircuit {
                    src_peer: src.to_string(),
                    dst_peer: dst.to_string(),
                    established_at: *established_at,
                }
            }).collect(),
        };
        self.peer_registry.update_metrics(registry_metrics).await;
        
        info!("╔══════════════════════════════════════════════════════════════════╗");
        info!("║  P2P MESH STATUS                                                 ║");
        info!("╠══════════════════════════════════════════════════════════════════╣");
        info!("║  TCP Peers: {}/{} (in: {}, out: {})                              ║", 
            m.connected_peers, m.target_peers, m.inbound_peers, m.outbound_peers);
        info!("║  GossipSub: {} mesh peers, {} total peers, {} topics             ║",
            mesh_peers.len(), gossip_peers.len(), topics.len());
        info!("║  State: {:?}                                              ║", m.state);
        info!("║  NAT: {}                                              ║", nat_str);
        info!("║  Dials: {} attempts, {} success, {} failed                   ║",
            m.dial_attempts, m.dial_successes, m.dial_failures);
        if m.hole_punch_successes > 0 || m.hole_punch_failures > 0 {
            info!("║  Hole-punch: {} success, {} failed                           ║",
                m.hole_punch_successes, m.hole_punch_failures);
        }
        if let Some(rtt) = m.average_rtt_ms {
            info!("║  Avg RTT: {}ms                                              ║", rtt);
        }
        info!("║  Messages: {} sent, {} received                              ║", 
            m.messages_sent, m.messages_received);
        
        // Warn if mesh is empty but we have connections
        if mesh_peers.is_empty() && m.connected_peers > 0 {
            info!("║  ⚠️  WARNING: MESH EMPTY - Messages will use flood_publish    ║");
        }
        info!("╚══════════════════════════════════════════════════════════════════╝");
    }

    /// Handle behaviour events with connection manager integration
    async fn handle_behaviour_event(&mut self, event: PyraxBehaviourEvent) {
        match event {
            PyraxBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message_id: _,
                message,
            }) => {
                self.metrics.messages_received += 1;
                self.handle_gossip_message(propagation_source, &message.data).await;
                
                // HEALTH MONITORING: Record successful data exchange for connection health tracking
                if let Some(peer) = self.conn_manager.peer_store_mut().get_peer_mut(&propagation_source) {
                    peer.record_success();
                    peer.record_data_exchange(); // Track actual data exchange, not just ping
                }
            }
            PyraxBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic }) => {
                let topic_str = topic.to_string();
                info!("Peer {} subscribed to topic {}", peer_id, topic_str);
                
                // Track subscribed peers in topic_peers map for our own management
                self.topic_peers
                    .entry(topic_str.clone())
                    .or_insert_with(HashSet::new)
                    .insert(peer_id);
                
                let our_blocks_topic = format!("pyrax/{}/blocks", self.network_id.name());
                let our_txs_topic = format!("pyrax/{}/txs", self.network_id.name());
                
                // TRUE MESH FIX: Do NOT add as explicit peer - explicit peers are OUTSIDE mesh!
                // GossipSub mesh formation: SUBSCRIBE event → peer added to peer_topics → heartbeat GRAFTs
                // By NOT using add_explicit_peer(), we let the normal GRAFT/PRUNE protocol work
                if topic_str == our_blocks_topic || topic_str == our_txs_topic {
                    let topic_peer_count = self.topic_peers.get(&topic_str).map(|s| s.len()).unwrap_or(0);
                    info!("✓ Peer {} subscribed to {} - mesh eligible ({} peers in topic)", 
                        peer_id, topic_str, topic_peer_count);
                    
                    // Log mesh state after subscription
                    let mesh_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_mesh_peers().collect();
                    let all_peers: Vec<_> = self.swarm.behaviour().gossipsub.all_peers().collect();
                    info!("  MESH STATE: {} in mesh, {} total GossipSub peers", mesh_peers.len(), all_peers.len());
                }
            }
            PyraxBehaviourEvent::Gossipsub(gossipsub::Event::Unsubscribed { peer_id, topic }) => {
                let topic_str = topic.to_string();
                debug!("Peer {} unsubscribed from topic {}", peer_id, topic_str);
                
                // Remove from topic_peers tracking
                if let Some(peers) = self.topic_peers.get_mut(&topic_str) {
                    peers.remove(&peer_id);
                }
                
                // TRUE MESH FIX: No explicit peer management needed
                // GossipSub handles mesh membership via GRAFT/PRUNE automatically
                let our_blocks_topic = format!("pyrax/{}/blocks", self.network_id.name());
                let our_txs_topic = format!("pyrax/{}/txs", self.network_id.name());
                
                if topic_str == our_blocks_topic || topic_str == our_txs_topic {
                    debug!("Peer {} unsubscribed from {} - mesh will auto-adjust via PRUNE", peer_id, topic_str);
                }
            }
            PyraxBehaviourEvent::Mdns(mdns::Event::Discovered(peers)) => {
                // mDNS discovery - for LOCAL network peers only
                // Don't add to Kademlia since mDNS addresses are always private/local
                // and would pollute the DHT when propagated to external peers
                for (peer_id, addr) in peers {
                    if peer_id == self.local_peer_id {
                        continue;
                    }
                    info!("mDNS discovered (LAN): {} at {}", peer_id, addr);
                    
                    // Only notify connection manager for local mesh (don't propagate to DHT)
                    self.conn_manager.on_peer_discovered(peer_id, vec![addr.to_string()]);
                    
                    // NOTE: Don't add mDNS addresses to Kademlia - they are private IPs
                    // that would cause WrongPeerId errors when propagated to external peers
                }
            }
            PyraxBehaviourEvent::Mdns(mdns::Event::Expired(peers)) => {
                for (peer_id, _) in peers {
                    debug!("mDNS peer expired: {}", peer_id);
                }
            }
            PyraxBehaviourEvent::Identify(identify::Event::Received { peer_id, info }) => {
                info!("Identified peer {}: {} ({})", 
                    peer_id, info.protocol_version, info.agent_version);
                
                // VERSION CHECK: Enforce minimum version requirement
                // This prevents outdated clients from connecting to the network
                let peer_version = parse_version(&info.agent_version);
                let is_pyrax_client = info.agent_version.to_lowercase().contains("pyrax") || 
                                      info.agent_version.to_lowercase().contains("inferno");
                
                if is_pyrax_client {
                    match peer_version {
                        Some(version) => {
                            if !version_meets_minimum(version) {
                                let (min_maj, min_min, min_pat) = MIN_REQUIRED_VERSION;
                                warn!("⚠️ VERSION MISMATCH: Peer {} has version {}.{}.{}, minimum required is {}.{}.{}",
                                    peer_id, version.0, version.1, version.2, min_maj, min_min, min_pat);
                                warn!("⚠️ DISCONNECTING peer {} - please update your Inferno Node app!", peer_id);
                                
                                // Schedule disconnect for outdated peer
                                self.pending_disconnect.insert(peer_id);
                                
                                // Don't process this peer further
                                return;
                            }
                            debug!("✓ Peer {} version {}.{}.{} meets minimum requirement", 
                                peer_id, version.0, version.1, version.2);
                        }
                        None => {
                            // Can't parse version, allow connection but log warning
                            debug!("Could not parse version from '{}' for peer {}", info.agent_version, peer_id);
                        }
                    }
                }
                
                // DEFINITIVE FIX: Partition addresses into valid (routable or relay) and invalid
                let mut valid_addrs: Vec<Multiaddr> = Vec::new();
                let mut bad_addrs: Vec<Multiaddr> = Vec::new();
                
                for addr in info.listen_addrs.iter() {
                    // DOUBLE-HOP FIX: Reject multi-hop relay addresses (causes MultipleCircuitRelayProtocolsUnsupported)
                    if !Self::is_valid_relay_address(addr) {
                        bad_addrs.push(addr.clone());
                        continue;
                    }
                    
                    // Accept routable addresses OR single-hop relay circuit addresses (for NAT traversal)
                    if Self::is_routable_address(addr) || Self::is_relay_address(addr) {
                        valid_addrs.push(addr.clone());
                    } else {
                        bad_addrs.push(addr.clone());
                    }
                }
                
                // CRITICAL: Remove invalid addresses from Kademlia to prevent WrongPeerId errors
                // These addresses may have been added from other sources (DHT propagation, etc.)
                for bad_addr in &bad_addrs {
                    self.swarm.behaviour_mut().kademlia.remove_address(&peer_id, bad_addr);
                }
                
                if !bad_addrs.is_empty() {
                    debug!("Filtered {} invalid addresses from peer {} (localhost/private/no-transport)", bad_addrs.len(), peer_id);
                }
                
                // RELAY FIX: Prioritize direct addresses over relay to prevent ResourceLimitExceeded
                // Only use relay addresses when no direct addresses are available
                let prioritized_addrs = Self::prioritize_direct_addresses(valid_addrs);
                
                // Update connection manager with peer info (only prioritized addresses)
                let listen_addrs: Vec<String> = prioritized_addrs.iter().map(|a| a.to_string()).collect();
                self.conn_manager.on_peer_identified(peer_id, info.agent_version.clone(), listen_addrs.clone());
                
                // PERSISTENT PEER CACHE: Update cache with actual addresses from Identify
                if let Some(ref mut cache) = self.peer_cache {
                    let is_bootnode = self.bootnode_peer_ids.contains(&peer_id);
                    cache.upsert_peer(peer_id.to_string(), listen_addrs.clone(), None, is_bootnode);
                }
                
                // Update legacy registry
                self.peer_registry.update_peer_version(&peer_id.to_string(), &info.agent_version).await;
                
                // SELF-DIAL FIX: Never add our own peer ID to Kademlia (causes WrongPeerId errors)
                if peer_id == self.local_peer_id {
                    debug!("Skipping self in Kademlia (peer_id == local_peer_id)");
                } else {
                    // Add only prioritized addresses to Kademlia (direct preferred over relay)
                    for addr in &prioritized_addrs {
                        self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                    }
                }
                
                if prioritized_addrs.iter().any(|a| Self::is_relay_address(a)) {
                    debug!("Peer {} only has relay addresses (no direct connectivity)", peer_id);
                }
                
                // TRUE MESH FIX: When a BOOTNODE is identified, just log it
                // DO NOT call unsubscribe/resubscribe - that CLEARS the mesh!
                // GossipSub automatically sends SUBSCRIBE on new connections
                // Mesh formation happens via heartbeat GRAFT (1s interval)
                if self.bootnode_peer_ids.contains(&peer_id) {
                    info!("✓ Bootnode {} identified - mesh will form via heartbeat GRAFT", peer_id);
                    self.log_mesh_state();
                }
            }
            PyraxBehaviourEvent::Ping(ping::Event { peer, result, .. }) => {
                // ASIC FIX: Deduplicate ping event processing to prevent ping storms
                // Skip if we processed a ping for this peer within 5 seconds
                let now = Instant::now();
                let should_process = match self.last_ping_sent.get(&peer) {
                    Some(last_sent) => now.duration_since(*last_sent) >= Duration::from_secs(5),
                    None => true,
                };
                
                if !should_process {
                    // Skip duplicate ping processing
                    if result.is_ok() {
                        // Still update ping success time silently
                        self.last_ping_success.insert(peer, now);
                    }
                    return;
                }
                
                self.last_ping_sent.insert(peer, now);
                
                match result {
                    Ok(rtt) => {
                        debug!("Ping to {} successful: {:?}", peer, rtt);
                        
                        // STABILITY FIX: Track last successful ping time
                        self.last_ping_success.insert(peer, now);
                        
                        // Update RTT in connection manager for scoring
                        self.conn_manager.on_ping_result(peer, Some(rtt));
                        
                        // Update legacy registry
                        self.peer_registry.update_peer_seen(&peer.to_string()).await;
                    }
                    Err(e) => {
                        warn!("Ping to {} failed: {:?}", peer, e);
                        self.conn_manager.on_ping_result(peer, None);
                        
                        // STABILITY FIX: If ping fails and it's been too long since last success,
                        // the connection might be stale - connection manager will handle reconnection
                    }
                }
            }
            PyraxBehaviourEvent::Kademlia(event) => {
                match event {
                    kad::Event::RoutingUpdated { peer, addresses, .. } => {
                        // DEFINITIVE FIX: Remove invalid addresses from Kademlia's routing table
                        // This prevents localhost/private IPs/pure-p2p from being used in dial attempts
                        // WRONGPEERID FIX: Also reject addresses that point to bootnode IPs with wrong peer ID
                        let mut valid_addrs: Vec<String> = Vec::new();
                        let mut removed_count = 0;
                        let mut bootnode_pollution_count = 0;
                        
                        for addr in addresses.iter() {
                            // WRONGPEERID FIX: Use comprehensive DHT address validation
                            // This catches bootnode IP pollution that causes WrongPeerId errors
                            if self.is_valid_dht_address(addr, &peer) {
                                valid_addrs.push(addr.to_string());
                            } else {
                                // Check if this was bootnode pollution specifically
                                if Self::is_routable_address(addr) && !Self::is_relay_address(addr) {
                                    bootnode_pollution_count += 1;
                                }
                                // CRITICAL: Actually REMOVE bad addresses from Kademlia
                                // Just filtering isn't enough - Kademlia will still use them for dials
                                self.swarm.behaviour_mut().kademlia.remove_address(&peer, addr);
                                removed_count += 1;
                            }
                        }
                        
                        if bootnode_pollution_count > 0 {
                            warn!("WRONGPEERID FIX: Removed {} bootnode-polluted addresses from peer {} (peer claiming bootnode IP)", 
                                bootnode_pollution_count, peer);
                        }
                        if removed_count > 0 {
                            debug!("Kademlia: REMOVED {} invalid addresses from peer {} (localhost/private/no-transport/bootnode-pollution)", 
                                removed_count, peer);
                        }
                        
                        if valid_addrs.is_empty() {
                            debug!("Kademlia: Peer {} has NO valid addresses after filtering", peer);
                        } else {
                            debug!("Kademlia: Routing updated for peer {} ({} valid addresses)", peer, valid_addrs.len());
                            self.conn_manager.on_peer_discovered(peer, valid_addrs);
                        }
                    }
                    kad::Event::OutboundQueryProgressed { result, .. } => {
                        match result {
                            kad::QueryResult::GetClosestPeers(Ok(ok)) => {
                                info!("Kademlia: Found {} closest peers", ok.peers.len());
                                
                                // Notify connection manager of discovered peers
                                // IMPORTANT: Do NOT dial by peer_id directly - this bypasses address filtering
                                // and causes WrongPeerId errors when swarm uses cached non-routable addresses
                                // Addresses will come via RoutingUpdated events which are already filtered
                                for peer_id in &ok.peers {
                                    if *peer_id == self.local_peer_id {
                                        continue;
                                    }
                                    // Peer discovered - addresses will arrive via RoutingUpdated event
                                    // which already has filtering. Don't dial here without addresses.
                                    debug!("Kademlia: Discovered peer {}, waiting for RoutingUpdated with addresses", peer_id);
                                }
                            }
                            kad::QueryResult::Bootstrap(Ok(ok)) => {
                                info!("Kademlia: Bootstrap step completed ({} remaining)", ok.num_remaining);
                                if ok.num_remaining == 0 {
                                    self.conn_manager.on_kademlia_bootstrap_complete();
                                }
                            }
                            kad::QueryResult::Bootstrap(Err(e)) => {
                                warn!("Kademlia: Bootstrap failed: {:?}", e);
                            }
                            _ => {}
                        }
                    }
                    kad::Event::InboundRequest { request } => {
                        debug!("Kademlia: Inbound request: {:?}", request);
                    }
                    _ => {}
                }
            }
            PyraxBehaviourEvent::RelayServer(event) => {
                // Track relay server events for visualization (when we act as relay for others)
                match &event {
                    relay::Event::ReservationReqAccepted { src_peer_id, .. } => {
                        info!("Relay Server: Accepted reservation from {}", src_peer_id);
                    }
                    relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id, .. } => {
                        info!("Relay Server: Circuit established {} <-> {} (via us)", src_peer_id, dst_peer_id);
                        // Track this circuit for visualizer with timestamp
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        self.active_relay_circuits.insert((*src_peer_id, *dst_peer_id), timestamp);
                    }
                    relay::Event::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                        info!("Relay Server: Circuit closed {} <-> {}", src_peer_id, dst_peer_id);
                        // Remove from tracking
                        self.active_relay_circuits.remove(&(*src_peer_id, *dst_peer_id));
                    }
                    _ => {
                        debug!("Relay Server: {:?}", event);
                    }
                }
            }
            PyraxBehaviourEvent::RelayClient(event) => {
                // Log relay client events (when we use relay for NAT traversal)
                // Also track relay health for TURN-like fallback
                match &event {
                    relay::client::Event::ReservationReqAccepted { relay_peer_id, renewal, limit } => {
                        info!("✓ Relay reservation accepted by {} (renewal: {}, limit: {:?})", 
                            relay_peer_id, renewal, limit);
                        info!("  → NAT traversal: Nodes can now reach us via /p2p/{}/p2p-circuit/p2p/{}", 
                            relay_peer_id, self.local_peer_id);
                        if !*renewal {
                            self.metrics.relay_reservations += 1;
                        }
                        // Track relay success
                        self.relay_manager.record_success(&relay_peer_id.to_string(), None);
                    }
                    relay::client::Event::OutboundCircuitEstablished { relay_peer_id, limit } => {
                        info!("✓ Outbound circuit established via relay {} (limit: {:?})", 
                            relay_peer_id, limit);
                        // Track relay success
                        self.relay_manager.record_success(&relay_peer_id.to_string(), None);
                    }
                    relay::client::Event::InboundCircuitEstablished { src_peer_id, limit } => {
                        info!("✓ Inbound circuit established from {} via relay (limit: {:?})", 
                            src_peer_id, limit);
                    }
                }
            }
            PyraxBehaviourEvent::Autonat(event) => {
                // AutoNAT events - NAT status detection
                match event {
                    autonat::Event::InboundProbe(probe) => {
                        debug!("AutoNAT: Inbound probe from {:?}", probe);
                    }
                    autonat::Event::OutboundProbe(probe) => {
                        match probe {
                            autonat::OutboundProbeEvent::Request { peer, .. } => {
                                debug!("AutoNAT: Sending probe request to {}", peer);
                            }
                            autonat::OutboundProbeEvent::Response { peer, address, .. } => {
                                info!("AutoNAT: Probe response from {} - our observed address: {}", peer, address);
                            }
                            autonat::OutboundProbeEvent::Error { peer, error, .. } => {
                                debug!("AutoNAT: Probe to {} failed: {:?}", peer.unwrap_or(PeerId::random()), error);
                            }
                        }
                    }
                    autonat::Event::StatusChanged { old, new } => {
                        info!("╔══════════════════════════════════════════════════════════════════╗");
                        info!("║  AutoNAT STATUS CHANGED: {:?} → {:?}", old, new);
                        info!("╚══════════════════════════════════════════════════════════════════╝");
                        
                        match new {
                            autonat::NatStatus::Public(addr) => {
                                info!("✓ NAT Status: PUBLIC - We are directly reachable at {}", addr);
                                info!("  → No relay needed for incoming connections");
                                self.metrics.nat_status = NatStatus::Public;
                            }
                            autonat::NatStatus::Private => {
                                info!("⚠ NAT Status: PRIVATE - We are behind NAT");
                                info!("  → Using relay for incoming connections");
                                info!("  → DCUtR will attempt hole-punching when possible");
                                self.metrics.nat_status = NatStatus::Private;
                                
                                // NORESERVATION FIX: When we detect we're behind NAT, 
                                // AGGRESSIVELY request relay reservations from ALL bootnodes
                                // This ensures other nodes can connect to us via relay circuit
                                info!("  → Requesting relay reservations from {} bootnodes...", self.bootstrap_peers.len());
                                let mut reservation_count = 0;
                                for bootnode in &self.bootstrap_peers.clone() {
                                    match self.listen_on_relay(bootnode) {
                                        Ok(_) => {
                                            reservation_count += 1;
                                            info!("  ✓ Requested relay reservation from {}", bootnode);
                                        }
                                        Err(e) => {
                                            warn!("  ✗ Failed to request relay from {}: {:?}", bootnode, e);
                                        }
                                    }
                                }
                                if reservation_count > 0 {
                                    info!("  → Relay reservations requested from {} bootnodes", reservation_count);
                                } else {
                                    warn!("  ⚠ Could not request relay from ANY bootnode - connectivity may be limited!");
                                }
                            }
                            autonat::NatStatus::Unknown => {
                                info!("? NAT Status: UNKNOWN - Still probing...");
                                self.metrics.nat_status = NatStatus::Unknown;
                            }
                        }
                    }
                }
            }
            PyraxBehaviourEvent::Dcutr(event) => {
                // DCUtR events - hole-punching attempts
                // dcutr::Event is a struct with remote_peer_id and result fields
                let dcutr::Event { remote_peer_id, result } = event;
                match result {
                    Ok(connection_id) => {
                        info!("✓ DCUtR: Hole-punch SUCCESS! Direct connection to {} established (conn: {:?})", 
                            remote_peer_id, connection_id);
                        info!("  → Relay no longer needed for this peer");
                        self.metrics.hole_punch_successes += 1;
                    }
                    Err(error) => {
                        warn!("✗ DCUtR: Hole-punch FAILED to {}: {:?}", remote_peer_id, error);
                        info!("  → Continuing to use relay for this peer");
                        self.metrics.hole_punch_failures += 1;
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle incoming gossip message
    async fn handle_gossip_message(&mut self, source: PeerId, data: &[u8]) {
        let msg: GossipMessage = match bincode::deserialize(data) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to deserialize gossip from {}: {}", source, e);
                return;
            }
        };

        match msg {
            GossipMessage::NewBlock(block) => {
                let block_hash = block.hash();
                let block_height = block.height();
                
                // STALE DATA FIX: Check for duplicate blocks
                if self.seen_blocks.contains(&block_hash) {
                    debug!("DUPLICATE BLOCK: already seen {} (height {})", block_hash, block_height);
                    return;
                }
                
                // STALE DATA FIX: Block height validation
                let current_tip = self.db.get_tip().height;
                
                // Reject blocks too far behind (stale data reintroduction)
                if block_height + 100 < current_tip {
                    warn!("STALE BLOCK REJECTED: height {} is {} blocks behind tip {} from {}", 
                        block_height, current_tip - block_height, current_tip, source);
                    return;
                }
                
                // Reject blocks too far ahead (spam prevention)
                if block_height > current_tip + 50 {
                    warn!("FUTURE BLOCK REJECTED: height {} is {} blocks ahead of tip {} from {}",
                        block_height, block_height - current_tip, current_tip, source);
                    return;
                }
                
                // Add to seen cache (LRU behavior - remove oldest if full)
                if self.seen_blocks.len() >= 5000 {
                    self.seen_blocks.pop_front();
                }
                self.seen_blocks.push_back(block_hash);
                
                info!("Received block {} (height {}) from {}", block_hash, block_height, source);
                
                // Forward to block processor
                if let Err(e) = self.block_tx.send(block).await {
                    error!("Failed to forward block: {}", e);
                }
            }
            GossipMessage::NewTransaction(tx) => {
                let tx_hash = tx.txid();
                
                // STALE DATA FIX: Check for duplicate transactions
                if self.seen_txs.contains(&tx_hash) {
                    return; // Silently ignore duplicates (very common)
                }
                
                // Add to seen cache (LRU behavior)
                if self.seen_txs.len() >= 10000 {
                    self.seen_txs.pop_front();
                }
                self.seen_txs.push_back(tx_hash);
                
                debug!("Received tx {} from {}", tx_hash, source);
                
                if let Err(e) = self.tx_tx.send(tx).await {
                    error!("Failed to forward tx: {}", e);
                }
            }
            GossipMessage::NewHeader(header) => {
                debug!("Received header {} from {}", header.hash(), source);
            }
            GossipMessage::GetBlocks { start_height, count } => {
                info!("Received GetBlocks request from {}: start={}, count={}", source, start_height, count);
                
                let current_tip = self.db.get_tip().height;
                
                // STALE DATA FIX: Validate the request
                // If peer is requesting blocks we don't have (stale chain), log warning
                if start_height > current_tip + 1 {
                    warn!("STALE DATA WARNING: Peer {} requesting blocks from height {} but our tip is {} - peer may have stale data",
                        source, start_height, current_tip);
                    // Don't serve - we don't have these blocks
                    return;
                }
                
                // Fetch blocks from our database and respond
                let mut blocks = Vec::new();
                let max_count = std::cmp::min(count, 100); // Limit to 100 blocks per request
                
                for height in start_height..(start_height + max_count) {
                    // STALE DATA FIX: Don't serve blocks beyond our current tip
                    if height > current_tip {
                        break;
                    }
                    
                    match self.db.get_block_by_height(height) {
                        Ok(Some(block)) => blocks.push(block),
                        Ok(None) => break, // No more blocks
                        Err(e) => {
                            warn!("Error fetching block at height {}: {}", height, e);
                            break;
                        }
                    }
                }
                
                if !blocks.is_empty() {
                    info!("Sending {} blocks (heights {}-{}) to peer {} (our tip: {})", 
                        blocks.len(), start_height, start_height + blocks.len() as u64 - 1, source, current_tip);
                    
                    // Broadcast the blocks response
                    let topic = gossipsub::IdentTopic::new(format!("pyrax/{}/blocks", self.network_id.name()));
                    let msg = GossipMessage::Blocks(blocks);
                    if let Ok(data) = bincode::serialize(&msg) {
                        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
                            warn!("Failed to send blocks response: {:?}", e);
                        }
                    }
                } else {
                    debug!("No blocks to send for request from {} (start={}, our tip={})", 
                        source, start_height, current_tip);
                }
            }
            GossipMessage::Blocks(mut blocks) => {
                info!("Received {} blocks from {} for sync", blocks.len(), source);
                
                // Sort blocks by height and process in order
                blocks.sort_by_key(|b| b.height());
                
                let first_height = blocks.first().map(|b| b.height()).unwrap_or(0);
                let last_height = blocks.last().map(|b| b.height()).unwrap_or(0);
                let current_tip = self.db.get_tip().height;
                
                // STALE DATA FIX: Validate blocks before processing
                let mut valid_blocks = Vec::new();
                let mut rejected_count = 0;
                
                for block in blocks {
                    let block_hash = block.hash();
                    let block_height = block.height();
                    
                    // Skip duplicates
                    if self.seen_blocks.contains(&block_hash) {
                        continue;
                    }
                    
                    // STALE DATA FIX: Reject blocks too far behind current tip
                    // But allow blocks if we're syncing from scratch (current_tip near 0)
                    if current_tip > 100 && block_height + 100 < current_tip {
                        warn!("STALE SYNC BLOCK REJECTED: height {} is {} blocks behind tip {} from {}", 
                            block_height, current_tip - block_height, current_tip, source);
                        rejected_count += 1;
                        continue;
                    }
                    
                    // STALE DATA FIX: Reject blocks too far ahead
                    if block_height > current_tip + 500 {
                        warn!("FUTURE SYNC BLOCK REJECTED: height {} is {} blocks ahead of tip {} from {}",
                            block_height, block_height - current_tip, current_tip, source);
                        rejected_count += 1;
                        continue;
                    }
                    
                    // Add to seen cache
                    if self.seen_blocks.len() >= 5000 {
                        self.seen_blocks.pop_front();
                    }
                    self.seen_blocks.push_back(block_hash);
                    
                    valid_blocks.push(block);
                }
                
                if rejected_count > 0 {
                    warn!("STALE DATA FIX: Rejected {} stale/future blocks from {} (accepted {})", 
                        rejected_count, source, valid_blocks.len());
                }
                
                for block in &valid_blocks {
                    // Forward each validated block to processor
                    if let Err(e) = self.block_tx.send(block.clone()).await {
                        error!("Failed to forward synced block: {}", e);
                    }
                }
                
                // Always request next batch if we got valid blocks and last_height indicates more to come
                if !valid_blocks.is_empty() && last_height > 0 {
                    // Wait for blocks to be processed
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    let our_height = self.db.get_tip().height;
                    
                    // Request more if we haven't caught up yet
                    if our_height < last_height || valid_blocks.len() >= 100 {
                        let next_height = last_height + 1;
                        info!("Requesting next sync batch from height {}", next_height);
                        let _ = self.request_blocks(next_height, 100);
                    } else {
                        info!("Sync complete! Our height: {}", our_height);
                    }
                }
            }
        }
    }
}

/// Peer information
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: String,
    pub address: String,
    pub best_height: u64,
}
