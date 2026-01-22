//! Incentivized Relay Network Module
//!
//! Features:
//! - Bandwidth accounting per peer
//! - Relay reward tracking
//! - Proof of relay generation
//! - Fair bandwidth distribution

use std::collections::HashMap;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

/// Bandwidth accounting for a peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthAccount {
    pub peer_id: String,
    pub bytes_relayed_for: u64,
    pub bytes_relayed_by: u64,
    pub relay_requests_served: u64,
    pub relay_requests_made: u64,
    pub first_seen: u64,
    pub last_activity: u64,
}

impl BandwidthAccount {
    pub fn new(peer_id: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            peer_id,
            bytes_relayed_for: 0,
            bytes_relayed_by: 0,
            relay_requests_served: 0,
            relay_requests_made: 0,
            first_seen: now,
            last_activity: now,
        }
    }
    
    pub fn record_relayed_for(&mut self, bytes: u64) {
        self.bytes_relayed_for += bytes;
        self.relay_requests_served += 1;
        self.last_activity = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    pub fn record_relayed_by(&mut self, bytes: u64) {
        self.bytes_relayed_by += bytes;
        self.relay_requests_made += 1;
        self.last_activity = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    /// Net contribution (positive = gave more, negative = took more)
    pub fn net_contribution(&self) -> i64 {
        self.bytes_relayed_for as i64 - self.bytes_relayed_by as i64
    }
    
    /// Contribution ratio
    pub fn contribution_ratio(&self) -> f64 {
        if self.bytes_relayed_by == 0 {
            if self.bytes_relayed_for > 0 { f64::INFINITY } else { 1.0 }
        } else {
            self.bytes_relayed_for as f64 / self.bytes_relayed_by as f64
        }
    }
}

/// Proof of relay for reward claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayProof {
    pub relay_node_id: String,
    pub client_node_id: String,
    pub bytes_relayed: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub message_count: u64,
    pub signature: Option<String>,
}

impl RelayProof {
    pub fn duration_secs(&self) -> u64 {
        self.end_time.saturating_sub(self.start_time)
    }
}

/// Relay session for tracking active relays
#[derive(Debug, Clone)]
pub struct RelaySession {
    pub session_id: String,
    pub client_peer_id: String,
    pub target_peer_id: String,
    pub started_at: Instant,
    pub bytes_relayed: u64,
    pub messages_relayed: u64,
    pub is_active: bool,
}

impl RelaySession {
    pub fn new(client: String, target: String) -> Self {
        Self {
            session_id: format!("{}-{}-{}", client, target, rand::random::<u32>()),
            client_peer_id: client,
            target_peer_id: target,
            started_at: Instant::now(),
            bytes_relayed: 0,
            messages_relayed: 0,
            is_active: true,
        }
    }
    
    pub fn record_relay(&mut self, bytes: u64) {
        self.bytes_relayed += bytes;
        self.messages_relayed += 1;
    }
    
    pub fn to_proof(&self, relay_node_id: String) -> RelayProof {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let start = now - self.started_at.elapsed().as_secs();
        
        RelayProof {
            relay_node_id,
            client_node_id: self.client_peer_id.clone(),
            bytes_relayed: self.bytes_relayed,
            start_time: start,
            end_time: now,
            message_count: self.messages_relayed,
            signature: None,
        }
    }
}

/// Relay reward calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayReward {
    pub epoch: u64,
    pub relay_node_id: String,
    pub total_bytes_relayed: u64,
    pub total_sessions: u64,
    pub reward_points: u64,
}

impl RelayReward {
    /// Calculate reward points (1 point per MB relayed)
    pub fn calculate_points(bytes: u64) -> u64 {
        bytes / (1024 * 1024)
    }
}

/// Incentivized relay manager
pub struct IncentivizedRelay {
    pub local_peer_id: String,
    pub accounts: HashMap<String, BandwidthAccount>,
    pub active_sessions: HashMap<String, RelaySession>,
    pub completed_proofs: Vec<RelayProof>,
    pub total_bytes_relayed: u64,
    pub total_sessions_served: u64,
    pub max_relay_bandwidth_kbps: Option<u32>,
    pub fair_sharing_enabled: bool,
}

impl IncentivizedRelay {
    pub fn new(local_peer_id: String) -> Self {
        Self {
            local_peer_id,
            accounts: HashMap::new(),
            active_sessions: HashMap::new(),
            completed_proofs: Vec::new(),
            total_bytes_relayed: 0,
            total_sessions_served: 0,
            max_relay_bandwidth_kbps: None,
            fair_sharing_enabled: true,
        }
    }
    
    pub fn get_account(&mut self, peer_id: &str) -> &mut BandwidthAccount {
        self.accounts.entry(peer_id.to_string())
            .or_insert_with(|| BandwidthAccount::new(peer_id.to_string()))
    }
    
    pub fn start_session(&mut self, client: String, target: String) -> String {
        let session = RelaySession::new(client.clone(), target);
        let session_id = session.session_id.clone();
        self.active_sessions.insert(session_id.clone(), session);
        self.total_sessions_served += 1;
        session_id
    }
    
    pub fn record_relay(&mut self, session_id: &str, bytes: u64) {
        if let Some(session) = self.active_sessions.get_mut(session_id) {
            session.record_relay(bytes);
            self.total_bytes_relayed += bytes;
            
            // Update account
            let client_id = session.client_peer_id.clone();
            self.get_account(&client_id).record_relayed_for(bytes);
        }
    }
    
    pub fn end_session(&mut self, session_id: &str) -> Option<RelayProof> {
        if let Some(mut session) = self.active_sessions.remove(session_id) {
            session.is_active = false;
            let proof = session.to_proof(self.local_peer_id.clone());
            self.completed_proofs.push(proof.clone());
            
            // Keep only last 1000 proofs
            while self.completed_proofs.len() > 1000 {
                self.completed_proofs.remove(0);
            }
            
            return Some(proof);
        }
        None
    }
    
    /// Check if peer is allowed more relay (fair sharing)
    pub fn allow_relay(&self, peer_id: &str) -> bool {
        if !self.fair_sharing_enabled {
            return true;
        }
        
        if let Some(account) = self.accounts.get(peer_id) {
            // Allow if contribution ratio > 0.1 (gives at least 10% of what they take)
            // Or if they haven't used much yet (< 100MB)
            account.contribution_ratio() > 0.1 || account.bytes_relayed_by < 100 * 1024 * 1024
        } else {
            true // New peer, allow
        }
    }
    
    /// Get relay statistics
    pub fn stats(&self) -> RelayStats {
        RelayStats {
            total_bytes_relayed: self.total_bytes_relayed,
            total_sessions: self.total_sessions_served,
            active_sessions: self.active_sessions.len(),
            unique_peers_served: self.accounts.len(),
            reward_points: RelayReward::calculate_points(self.total_bytes_relayed),
        }
    }
    
    /// Get top contributors
    pub fn top_contributors(&self, count: usize) -> Vec<&BandwidthAccount> {
        let mut accounts: Vec<_> = self.accounts.values().collect();
        accounts.sort_by(|a, b| b.bytes_relayed_for.cmp(&a.bytes_relayed_for));
        accounts.into_iter().take(count).collect()
    }
}

impl Default for IncentivizedRelay {
    fn default() -> Self { Self::new(String::new()) }
}

/// Relay statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStats {
    pub total_bytes_relayed: u64,
    pub total_sessions: u64,
    pub active_sessions: usize,
    pub unique_peers_served: usize,
    pub reward_points: u64,
}
