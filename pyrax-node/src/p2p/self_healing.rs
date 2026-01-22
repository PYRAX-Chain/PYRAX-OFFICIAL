//! Self-Healing Network Module
//!
//! Features:
//! - Auto-reconnect with exponential backoff
//! - Relay cascade (try relay A, then B, then C)
//! - Peer resurrection (retry known-good peers)
//! - Network partition detection
//! - Hot-swap bootstrap discovery

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use tracing::{info, warn, debug, error};

/// Reconnection state for a peer
#[derive(Debug, Clone)]
pub struct ReconnectionState {
    pub peer_id: String,
    pub attempt_count: u32,
    pub last_attempt: Instant,
    pub next_attempt: Instant,
    pub backoff_secs: u64,
    pub max_attempts: u32,
    pub permanently_failed: bool,
}

impl ReconnectionState {
    pub fn new(peer_id: String) -> Self {
        Self {
            peer_id,
            attempt_count: 0,
            last_attempt: Instant::now(),
            next_attempt: Instant::now(),
            backoff_secs: 1,
            max_attempts: 10,
            permanently_failed: false,
        }
    }
    
    pub fn record_failure(&mut self) {
        self.attempt_count += 1;
        self.last_attempt = Instant::now();
        // Exponential backoff with jitter: 1s, 2s, 4s, 8s, 16s, 32s, 60s max
        self.backoff_secs = (self.backoff_secs * 2).min(60);
        let jitter = (rand::random::<u64>() % 1000) as f64 / 1000.0;
        let delay = Duration::from_secs_f64(self.backoff_secs as f64 * (1.0 + jitter * 0.2));
        self.next_attempt = Instant::now() + delay;
        
        if self.attempt_count >= self.max_attempts {
            self.permanently_failed = true;
            warn!("Peer {} permanently failed after {} attempts", self.peer_id, self.attempt_count);
        }
    }
    
    pub fn record_success(&mut self) {
        self.attempt_count = 0;
        self.backoff_secs = 1;
        self.permanently_failed = false;
    }
    
    pub fn should_retry(&self) -> bool {
        !self.permanently_failed && Instant::now() >= self.next_attempt
    }
}

/// Relay cascade configuration
#[derive(Debug, Clone)]
pub struct RelayCascade {
    pub relays: Vec<RelayInfo>,
    pub current_index: usize,
    pub last_switch: Instant,
    pub min_switch_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct RelayInfo {
    pub address: String,
    pub peer_id: String,
    pub priority: u8,
    pub is_available: bool,
    pub last_check: Instant,
    pub latency_ms: Option<u32>,
}

impl RelayCascade {
    pub fn new(relays: Vec<RelayInfo>) -> Self {
        Self {
            relays,
            current_index: 0,
            last_switch: Instant::now(),
            min_switch_interval: Duration::from_secs(30),
        }
    }
    
    pub fn current_relay(&self) -> Option<&RelayInfo> {
        self.relays.get(self.current_index)
    }
    
    pub fn switch_to_next(&mut self) -> Option<&RelayInfo> {
        if self.last_switch.elapsed() < self.min_switch_interval {
            return self.current_relay();
        }
        
        let start = self.current_index;
        loop {
            self.current_index = (self.current_index + 1) % self.relays.len();
            if self.current_index == start {
                break; // Tried all relays
            }
            if self.relays[self.current_index].is_available {
                self.last_switch = Instant::now();
                info!("Switched to relay: {}", self.relays[self.current_index].address);
                return Some(&self.relays[self.current_index]);
            }
        }
        None
    }
    
    pub fn mark_unavailable(&mut self, index: usize) {
        if let Some(relay) = self.relays.get_mut(index) {
            relay.is_available = false;
            relay.last_check = Instant::now();
        }
    }
    
    pub fn mark_available(&mut self, index: usize) {
        if let Some(relay) = self.relays.get_mut(index) {
            relay.is_available = true;
            relay.last_check = Instant::now();
        }
    }
}

/// Known good peers for resurrection
#[derive(Debug, Clone)]
pub struct KnownGoodPeer {
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub last_seen: Instant,
    pub total_uptime_secs: u64,
    pub successful_connections: u32,
}

/// Network partition detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkHealth {
    Healthy,
    Degraded,
    Partitioned,
    Isolated,
}

#[derive(Debug, Clone)]
pub struct PartitionDetector {
    pub peer_count_history: VecDeque<(Instant, usize)>,
    pub block_height_history: VecDeque<(Instant, u64)>,
    pub last_block_received: Instant,
    pub health: NetworkHealth,
}

impl PartitionDetector {
    pub fn new() -> Self {
        Self {
            peer_count_history: VecDeque::with_capacity(60),
            block_height_history: VecDeque::with_capacity(60),
            last_block_received: Instant::now(),
            health: NetworkHealth::Healthy,
        }
    }
    
    pub fn record_peer_count(&mut self, count: usize) {
        let now = Instant::now();
        self.peer_count_history.push_back((now, count));
        while self.peer_count_history.len() > 60 {
            self.peer_count_history.pop_front();
        }
        self.update_health();
    }
    
    pub fn record_block(&mut self, height: u64) {
        let now = Instant::now();
        self.block_height_history.push_back((now, height));
        self.last_block_received = now;
        while self.block_height_history.len() > 60 {
            self.block_height_history.pop_front();
        }
        self.update_health();
    }
    
    fn update_health(&mut self) {
        let peer_count = self.peer_count_history.back().map(|(_, c)| *c).unwrap_or(0);
        let time_since_block = self.last_block_received.elapsed();
        
        self.health = if peer_count == 0 {
            NetworkHealth::Isolated
        } else if peer_count < 3 || time_since_block > Duration::from_secs(300) {
            NetworkHealth::Partitioned
        } else if peer_count < 10 || time_since_block > Duration::from_secs(120) {
            NetworkHealth::Degraded
        } else {
            NetworkHealth::Healthy
        };
    }
    
    pub fn is_isolated(&self) -> bool {
        matches!(self.health, NetworkHealth::Isolated | NetworkHealth::Partitioned)
    }
}

impl Default for PartitionDetector {
    fn default() -> Self { Self::new() }
}

/// Self-healing network manager
pub struct SelfHealingNetwork {
    pub reconnection_states: HashMap<String, ReconnectionState>,
    pub relay_cascade: Option<RelayCascade>,
    pub known_good_peers: Vec<KnownGoodPeer>,
    pub partition_detector: PartitionDetector,
    pub dns_bootstrap_urls: Vec<String>,
    pub last_bootstrap_refresh: Instant,
}

impl SelfHealingNetwork {
    pub fn new() -> Self {
        Self {
            reconnection_states: HashMap::new(),
            relay_cascade: None,
            known_good_peers: Vec::new(),
            partition_detector: PartitionDetector::new(),
            dns_bootstrap_urls: vec![
                "_dnsaddr.bootstrap.pyrax.org".to_string(),
                "_dnsaddr.nodes.pyrax.org".to_string(),
            ],
            last_bootstrap_refresh: Instant::now(),
        }
    }
    
    pub fn get_reconnection_state(&mut self, peer_id: &str) -> &mut ReconnectionState {
        self.reconnection_states.entry(peer_id.to_string())
            .or_insert_with(|| ReconnectionState::new(peer_id.to_string()))
    }
    
    pub fn peers_to_retry(&self) -> Vec<&ReconnectionState> {
        self.reconnection_states.values()
            .filter(|s| s.should_retry())
            .collect()
    }
    
    pub fn add_known_good_peer(&mut self, peer_id: String, addresses: Vec<String>) {
        if let Some(existing) = self.known_good_peers.iter_mut().find(|p| p.peer_id == peer_id) {
            existing.last_seen = Instant::now();
            existing.successful_connections += 1;
            for addr in addresses {
                if !existing.addresses.contains(&addr) {
                    existing.addresses.push(addr);
                }
            }
        } else {
            self.known_good_peers.push(KnownGoodPeer {
                peer_id,
                addresses,
                last_seen: Instant::now(),
                total_uptime_secs: 0,
                successful_connections: 1,
            });
        }
        // Keep only top 100 peers
        self.known_good_peers.sort_by(|a, b| b.successful_connections.cmp(&a.successful_connections));
        self.known_good_peers.truncate(100);
    }
    
    pub fn resurrection_candidates(&self) -> Vec<&KnownGoodPeer> {
        self.known_good_peers.iter()
            .filter(|p| p.last_seen.elapsed() > Duration::from_secs(300))
            .take(10)
            .collect()
    }
    
    pub fn needs_bootstrap_refresh(&self) -> bool {
        self.partition_detector.is_isolated() || 
        self.last_bootstrap_refresh.elapsed() > Duration::from_secs(3600)
    }
}

impl Default for SelfHealingNetwork {
    fn default() -> Self { Self::new() }
}
