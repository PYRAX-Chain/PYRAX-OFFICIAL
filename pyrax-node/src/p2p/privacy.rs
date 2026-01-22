//! Privacy-First Networking Module
//!
//! Features:
//! - Dandelion++ transaction propagation
//! - Encrypted gossip options
//! - IP obfuscation via relay
//! - Metadata minimization

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use rand::Rng;
use tracing::{info, debug};

/// Dandelion++ phases
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DandelionPhase {
    /// Stem phase - propagate to one random peer
    Stem,
    /// Fluff phase - broadcast to all peers
    Fluff,
}

/// Dandelion++ configuration
#[derive(Debug, Clone)]
pub struct DandelionConfig {
    /// Probability of transitioning from stem to fluff (per hop)
    pub fluff_probability: f64,
    /// Maximum stem hops before forced fluff
    pub max_stem_hops: u8,
    /// Embargo timeout before fluff if not seen
    pub embargo_timeout: Duration,
    /// Enable Dandelion++ (can be disabled)
    pub enabled: bool,
}

impl Default for DandelionConfig {
    fn default() -> Self {
        Self {
            fluff_probability: 0.1, // 10% chance per hop
            max_stem_hops: 10,
            embargo_timeout: Duration::from_secs(39), // ~average 10 hops at 4s each
            enabled: true,
        }
    }
}

/// Transaction state in Dandelion++
#[derive(Debug, Clone)]
pub struct DandelionTx {
    pub txid: String,
    pub phase: DandelionPhase,
    pub stem_hops: u8,
    pub stem_peer: Option<String>,
    pub received_at: Instant,
    pub fluffed_at: Option<Instant>,
}

impl DandelionTx {
    pub fn new_stem(txid: String) -> Self {
        Self {
            txid,
            phase: DandelionPhase::Stem,
            stem_hops: 0,
            stem_peer: None,
            received_at: Instant::now(),
            fluffed_at: None,
        }
    }
    
    pub fn should_fluff(&self, config: &DandelionConfig) -> bool {
        if self.stem_hops >= config.max_stem_hops {
            return true;
        }
        if self.received_at.elapsed() > config.embargo_timeout {
            return true;
        }
        rand::thread_rng().gen::<f64>() < config.fluff_probability
    }
    
    pub fn fluff(&mut self) {
        self.phase = DandelionPhase::Fluff;
        self.fluffed_at = Some(Instant::now());
    }
}

/// Dandelion++ manager
pub struct DandelionManager {
    pub config: DandelionConfig,
    pub transactions: HashMap<String, DandelionTx>,
    pub stem_peers: Vec<String>,
    pub last_stem_rotation: Instant,
    pub stem_rotation_interval: Duration,
}

impl DandelionManager {
    pub fn new(config: DandelionConfig) -> Self {
        Self {
            config,
            transactions: HashMap::new(),
            stem_peers: Vec::new(),
            last_stem_rotation: Instant::now(),
            stem_rotation_interval: Duration::from_secs(600), // 10 min
        }
    }
    
    pub fn select_stem_peer(&mut self, available_peers: &[String]) -> Option<String> {
        // Rotate stem peers periodically
        if self.last_stem_rotation.elapsed() > self.stem_rotation_interval {
            self.stem_peers.clear();
            self.last_stem_rotation = Instant::now();
        }
        
        // Select 2 outbound stem peers
        if self.stem_peers.is_empty() && !available_peers.is_empty() {
            let mut rng = rand::thread_rng();
            let count = available_peers.len().min(2);
            let mut selected = HashSet::new();
            while selected.len() < count {
                let idx = rng.gen_range(0..available_peers.len());
                selected.insert(available_peers[idx].clone());
            }
            self.stem_peers = selected.into_iter().collect();
            debug!("Selected {} stem peers for Dandelion++", self.stem_peers.len());
        }
        
        if self.stem_peers.is_empty() {
            return None;
        }
        
        let idx = rand::thread_rng().gen_range(0..self.stem_peers.len());
        Some(self.stem_peers[idx].clone())
    }
    
    pub fn process_tx(&mut self, txid: String, from_stem: bool) -> DandelionPhase {
        if !self.config.enabled {
            return DandelionPhase::Fluff;
        }
        
        let tx = self.transactions.entry(txid.clone())
            .or_insert_with(|| DandelionTx::new_stem(txid));
        
        if from_stem {
            tx.stem_hops += 1;
        }
        
        if tx.should_fluff(&self.config) {
            tx.fluff();
        }
        
        tx.phase
    }
    
    pub fn cleanup_old(&mut self) {
        let cutoff = Duration::from_secs(300);
        self.transactions.retain(|_, tx| tx.received_at.elapsed() < cutoff);
    }
}

impl Default for DandelionManager {
    fn default() -> Self { Self::new(DandelionConfig::default()) }
}

/// Privacy mode levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrivacyLevel {
    /// Standard - normal operation
    #[default]
    Standard,
    /// Enhanced - Dandelion++, reduced metadata
    Enhanced,
    /// Maximum - relay-only, no direct connections
    Maximum,
}

impl PrivacyLevel {
    pub fn use_dandelion(&self) -> bool {
        matches!(self, PrivacyLevel::Enhanced | PrivacyLevel::Maximum)
    }
    
    pub fn relay_only(&self) -> bool {
        matches!(self, PrivacyLevel::Maximum)
    }
    
    pub fn minimize_metadata(&self) -> bool {
        !matches!(self, PrivacyLevel::Standard)
    }
}

/// IP obfuscation via relay
#[derive(Debug, Clone)]
pub struct IpObfuscation {
    pub enabled: bool,
    pub relay_address: Option<String>,
    pub real_ip_hidden: bool,
}

impl Default for IpObfuscation {
    fn default() -> Self {
        Self {
            enabled: false,
            relay_address: None,
            real_ip_hidden: false,
        }
    }
}

/// Privacy manager
pub struct PrivacyManager {
    pub level: PrivacyLevel,
    pub dandelion: DandelionManager,
    pub ip_obfuscation: IpObfuscation,
}

impl PrivacyManager {
    pub fn new(level: PrivacyLevel) -> Self {
        let mut dandelion_config = DandelionConfig::default();
        dandelion_config.enabled = level.use_dandelion();
        
        Self {
            level,
            dandelion: DandelionManager::new(dandelion_config),
            ip_obfuscation: IpObfuscation {
                enabled: level.relay_only(),
                ..Default::default()
            },
        }
    }
}

impl Default for PrivacyManager {
    fn default() -> Self { Self::new(PrivacyLevel::Standard) }
}
