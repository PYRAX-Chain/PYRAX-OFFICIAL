//! Intelligent Peer Selection - ML-inspired scoring
use std::collections::HashMap;
use std::time::{Duration, Instant};
use libp2p::PeerId;

#[derive(Debug, Clone, Default)]
pub struct PeerFeatures {
    pub connection_success_rate: f64,
    pub avg_latency_ms: f64,
    pub message_success_rate: f64,
    pub responsiveness: f64,
    pub is_bootnode: bool,
}

impl PeerFeatures {
    pub fn score(&self) -> f64 {
        let mut s = 0.0;
        s += self.connection_success_rate * 25.0;
        s += (1000.0 / (self.avg_latency_ms + 1.0)).min(1.0) * 25.0;
        s += self.message_success_rate * 25.0;
        s += self.responsiveness * 20.0;
        s += if self.is_bootnode { 5.0 } else { 0.0 };
        s.clamp(0.0, 100.0)
    }
}

#[derive(Debug, Clone)]
pub struct PeerHistory {
    pub peer_id: PeerId,
    pub features: PeerFeatures,
    pub conn_attempts: u32,
    pub conn_successes: u32,
    pub last_seen: Instant,
}

impl PeerHistory {
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id, features: PeerFeatures::default(), conn_attempts: 0, conn_successes: 0, last_seen: Instant::now() }
    }
    
    pub fn record_connection(&mut self, success: bool, latency_ms: Option<u64>) {
        self.conn_attempts += 1;
        if success { self.conn_successes += 1; }
        self.features.connection_success_rate = self.conn_successes as f64 / self.conn_attempts as f64;
        if let Some(l) = latency_ms {
            self.features.avg_latency_ms = (self.features.avg_latency_ms * 0.9) + (l as f64 * 0.1);
        }
        self.last_seen = Instant::now();
    }
    
    pub fn predict_success(&self) -> f64 {
        if self.conn_attempts < 3 { 0.5 } else { self.features.connection_success_rate }
    }
}

pub struct IntelligentPeerSelector {
    pub histories: HashMap<PeerId, PeerHistory>,
}

impl IntelligentPeerSelector {
    pub fn new() -> Self { Self { histories: HashMap::new() } }
    
    pub fn get_history(&mut self, id: PeerId) -> &mut PeerHistory {
        self.histories.entry(id).or_insert_with(|| PeerHistory::new(id))
    }
    
    pub fn select_best(&self, candidates: &[PeerId], count: usize) -> Vec<PeerId> {
        let mut scored: Vec<_> = candidates.iter()
            .map(|id| (*id, self.histories.get(id).map(|h| h.features.score()).unwrap_or(50.0)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.into_iter().take(count).map(|(id, _)| id).collect()
    }
    
    pub fn best_candidates(&self, count: usize) -> Vec<PeerId> {
        let mut peers: Vec<_> = self.histories.iter()
            .map(|(id, h)| (*id, h.predict_success()))
            .filter(|(_, p)| *p > 0.3)
            .collect();
        peers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        peers.into_iter().take(count).map(|(id, _)| id).collect()
    }
}

impl Default for IntelligentPeerSelector { fn default() -> Self { Self::new() } }
