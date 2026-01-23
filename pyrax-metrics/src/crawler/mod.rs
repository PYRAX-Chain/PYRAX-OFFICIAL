//! Node Discovery Crawler
//!
//! Discovers nodes in the network by querying pyrax_getPeers from seed nodes.

use crate::config::CrawlerConfig;
use crate::observer::{RpcClient, PeerInfo};
use crate::state::AppState;
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;
use tokio::time::interval;
use tracing::{info, warn, debug};

/// Discovered node information
#[derive(Debug, Clone)]
pub struct DiscoveredNode {
    /// RPC endpoint URL
    pub endpoint: String,
    /// Peer ID
    pub peer_id: String,
    /// Last seen timestamp
    pub last_seen: u64,
    /// Is reachable
    pub reachable: bool,
    /// Best block height
    pub best_height: u64,
    /// Node version
    pub version: Option<String>,
}

/// Shared discovered nodes list
pub type SharedDiscoveredNodes = Arc<RwLock<Vec<DiscoveredNode>>>;

/// Node Discovery Crawler
pub struct NodeCrawler {
    config: CrawlerConfig,
    seed_endpoints: Vec<String>,
    discovered: SharedDiscoveredNodes,
    seen_peers: Arc<RwLock<HashSet<String>>>,
    app_state: Option<AppState>,
}

impl NodeCrawler {
    /// Create a new node crawler
    pub fn new(config: CrawlerConfig, seed_endpoints: Vec<String>) -> Self {
        Self {
            config,
            seed_endpoints,
            discovered: Arc::new(RwLock::new(Vec::new())),
            seen_peers: Arc::new(RwLock::new(HashSet::new())),
            app_state: None,
        }
    }
    
    /// Set app state for alerts
    pub fn with_app_state(mut self, app_state: AppState) -> Self {
        self.app_state = Some(app_state);
        self
    }
    
    /// Get shared reference to discovered nodes
    pub fn discovered_nodes(&self) -> SharedDiscoveredNodes {
        self.discovered.clone()
    }
    
    /// Run the crawler loop
    pub async fn run(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Node crawler disabled, skipping");
            return Ok(());
        }
        
        let crawl_interval = Duration::from_millis(self.config.crawl_interval_ms);
        let mut interval = interval(crawl_interval);
        
        info!("Starting node crawler with {}ms interval", self.config.crawl_interval_ms);
        info!("Seed endpoints: {:?}", self.seed_endpoints);
        
        loop {
            interval.tick().await;
            
            if let Err(e) = self.crawl_network().await {
                warn!("Crawl error: {}", e);
            }
            
            let count = self.discovered.read().len();
            info!("Discovered {} nodes", count);
        }
    }
    
    /// Perform one crawl cycle
    async fn crawl_network(&self) -> Result<()> {
        let timeout = Duration::from_millis(self.config.probe_timeout_ms);
        
        // Start with seed endpoints
        let mut endpoints_to_probe: Vec<String> = self.seed_endpoints.clone();
        
        // Add already discovered endpoints
        {
            let discovered = self.discovered.read();
            for node in discovered.iter() {
                if !endpoints_to_probe.contains(&node.endpoint) {
                    endpoints_to_probe.push(node.endpoint.clone());
                }
            }
        }
        
        // Probe each endpoint
        for endpoint in endpoints_to_probe.iter() {
            if self.discovered.read().len() >= self.config.max_nodes {
                debug!("Reached max nodes limit");
                break;
            }
            
            let client = RpcClient::new(endpoint.clone(), timeout);
            
            // Try to get peers from this node
            match client.get_peers().await {
                Ok(peers) => {
                    debug!("Got {} peers from {}", peers.len(), endpoint);
                    self.process_peers(peers).await;
                }
                Err(e) => {
                    debug!("Failed to get peers from {}: {}", endpoint, e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Process discovered peers
    async fn process_peers(&self, peers: Vec<PeerInfo>) {
        let timestamp = chrono::Utc::now().timestamp() as u64;
        
        for peer in peers {
            // Skip if already seen
            if self.seen_peers.read().contains(&peer.id) {
                continue;
            }
            
            // Mark as seen
            self.seen_peers.write().insert(peer.id.clone());
            
            // Extract RPC endpoint from remote_addr if available
            if let Some(remote_addr) = &peer.remote_addr {
                // Try to construct RPC URL from remote address
                // Format: IP:P2P_PORT -> http://IP:RPC_PORT (assume RPC is P2P_PORT - 1)
                if let Some(endpoint) = self.addr_to_rpc_endpoint(remote_addr) {
                    let node = DiscoveredNode {
                        endpoint: endpoint.clone(),
                        peer_id: peer.id.clone(),
                        last_seen: timestamp,
                        reachable: true, // Will be verified on next probe
                        best_height: peer.best_height,
                        version: peer.version.clone(),
                    };
                    
                    // Check if not already discovered
                    let is_new;
                    {
                        let mut discovered = self.discovered.write();
                        is_new = !discovered.iter().any(|n| n.peer_id == peer.id);
                        if is_new && discovered.len() < self.config.max_nodes {
                            info!("Discovered new node: {} (height: {})", node.endpoint, node.best_height);
                            discovered.push(node.clone());
                        }
                    }
                    
                    // Send alert for new node (outside lock)
                    if is_new {
                        if let Some(ref state) = self.app_state {
                            let msg = format!(
                                "🌐 <b>New Node Discovered</b>\nEndpoint: {}\nHeight: {}\nVersion: {}",
                                endpoint,
                                peer.best_height,
                                peer.version.as_deref().unwrap_or("unknown")
                            );
                            let am = state.alert_manager();
                            // Fire and forget the alert
                            tokio::spawn(async move {
                                am.send_alert("New Node", &msg).await;
                            });
                        }
                    }
                }
            }
        }
    }
    
    /// Convert P2P address to RPC endpoint
    fn addr_to_rpc_endpoint(&self, addr: &str) -> Option<String> {
        // Parse IP:PORT format
        let parts: Vec<&str> = addr.split(':').collect();
        if parts.len() >= 2 {
            let ip = parts[0];
            // Assume RPC port is 8545 by default for discovered nodes
            // In production, this should be configurable or discovered
            Some(format!("http://{}:8545", ip))
        } else {
            None
        }
    }
}

/// Create a dummy crawler that returns an empty list (for when disabled)
pub fn create_empty_discovered_nodes() -> SharedDiscoveredNodes {
    Arc::new(RwLock::new(Vec::new()))
}
