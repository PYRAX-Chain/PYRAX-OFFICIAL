// NEURAX Mesh Network Optimizer
// Intelligent rule-based system for optimizing P2P network performance
// This runs continuously and makes decisions to improve network health

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{info, warn, debug};

/// Network optimization action that NEURAX can take
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationAction {
    /// Suggest reconnecting to a specific bootnode
    ReconnectBootnode { ip: String, reason: String },
    /// Suggest dropping a poorly performing peer
    DropPeer { peer_id: String, reason: String },
    /// Adjust connection parameters
    AdjustParameters { param: String, old_value: String, new_value: String, reason: String },
    /// Enable/disable relay mode
    ToggleRelay { enable: bool, reason: String },
    /// Suggest network restart
    RestartNetwork { reason: String },
    /// No action needed
    NoAction,
}

/// Health metrics for a single peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerHealth {
    pub peer_id: String,
    pub latency_ms: u64,
    pub latency_history: VecDeque<u64>,
    pub blocks_received: u64,
    pub failures: u64,
    pub last_seen: u64,
    pub score: f64,
    pub is_bootnode: bool,
}

impl PeerHealth {
    pub fn new(peer_id: String, is_bootnode: bool) -> Self {
        Self {
            peer_id,
            latency_ms: 0,
            latency_history: VecDeque::with_capacity(30),
            blocks_received: 0,
            failures: 0,
            last_seen: 0,
            score: 50.0, // Start neutral
            is_bootnode,
        }
    }

    pub fn update_latency(&mut self, latency: u64) {
        self.latency_ms = latency;
        self.latency_history.push_back(latency);
        if self.latency_history.len() > 30 {
            self.latency_history.pop_front();
        }
        self.recalculate_score();
    }

    pub fn record_success(&mut self) {
        self.blocks_received += 1;
        self.score = (self.score + 2.0).min(100.0);
    }

    pub fn record_failure(&mut self) {
        self.failures += 1;
        self.score = (self.score - 5.0).max(0.0);
    }

    fn recalculate_score(&mut self) {
        let avg_latency: f64 = if self.latency_history.is_empty() {
            100.0
        } else {
            self.latency_history.iter().sum::<u64>() as f64 / self.latency_history.len() as f64
        };

        // Score formula:
        // - Base: 50
        // - Low latency bonus: up to +30 (for <50ms)
        // - High latency penalty: up to -30 (for >500ms)
        // - Reliability bonus: up to +20 based on blocks_received vs failures
        let latency_score = if avg_latency < 50.0 {
            30.0
        } else if avg_latency < 100.0 {
            20.0
        } else if avg_latency < 200.0 {
            10.0
        } else if avg_latency < 500.0 {
            0.0
        } else {
            -20.0
        };

        let reliability = if self.blocks_received + self.failures == 0 {
            0.0
        } else {
            (self.blocks_received as f64 / (self.blocks_received + self.failures) as f64) * 20.0
        };

        // Bootnodes get a bonus
        let bootnode_bonus = if self.is_bootnode { 10.0 } else { 0.0 };

        self.score = (50.0 + latency_score + reliability + bootnode_bonus).clamp(0.0, 100.0);
    }

    pub fn avg_latency(&self) -> f64 {
        if self.latency_history.is_empty() {
            self.latency_ms as f64
        } else {
            self.latency_history.iter().sum::<u64>() as f64 / self.latency_history.len() as f64
        }
    }
}

/// Network-wide health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkHealth {
    pub overall_score: f64,
    pub peer_count: usize,
    pub bootnode_connected: bool,
    pub avg_latency_ms: f64,
    pub block_propagation_time_ms: Option<u64>,
    pub mesh_stability: f64, // 0-100, based on connection churn
    pub issues: Vec<NetworkIssue>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkIssue {
    pub severity: IssueSeverity,
    pub category: String,
    pub description: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IssueSeverity {
    Critical,
    Warning,
    Info,
}

/// The main mesh optimizer state
pub struct MeshOptimizer {
    pub peer_health: RwLock<HashMap<String, PeerHealth>>,
    pub network_health: RwLock<NetworkHealth>,
    pub optimization_history: RwLock<VecDeque<OptimizationAction>>,
    pub last_optimization: RwLock<Instant>,
    pub config: RwLock<OptimizerConfig>,
    pub enabled: RwLock<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    pub min_peers: usize,
    pub target_peers: usize,
    pub max_peers: usize,
    pub max_latency_ms: u64,
    pub min_peer_score: f64,
    pub optimization_interval_secs: u64,
    pub aggressive_mode: bool, // More proactive optimization
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            min_peers: 3,
            target_peers: 8,
            max_peers: 20,
            max_latency_ms: 500,
            min_peer_score: 30.0,
            optimization_interval_secs: 30,
            aggressive_mode: false,
        }
    }
}

impl MeshOptimizer {
    pub fn new() -> Self {
        Self {
            peer_health: RwLock::new(HashMap::new()),
            network_health: RwLock::new(NetworkHealth {
                overall_score: 0.0,
                peer_count: 0,
                bootnode_connected: false,
                avg_latency_ms: 0.0,
                block_propagation_time_ms: None,
                mesh_stability: 100.0,
                issues: Vec::new(),
                recommendations: Vec::new(),
            }),
            optimization_history: RwLock::new(VecDeque::with_capacity(100)),
            last_optimization: RwLock::new(Instant::now()),
            config: RwLock::new(OptimizerConfig::default()),
            enabled: RwLock::new(true),
        }
    }

    /// Update peer metrics from node status
    pub fn update_peer(&self, peer_id: &str, latency_ms: u64, is_bootnode: bool) {
        let mut peers = self.peer_health.write();
        let health = peers.entry(peer_id.to_string())
            .or_insert_with(|| PeerHealth::new(peer_id.to_string(), is_bootnode));
        health.update_latency(latency_ms);
        health.last_seen = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// Record a successful block/message from peer
    pub fn record_peer_success(&self, peer_id: &str) {
        let mut peers = self.peer_health.write();
        if let Some(health) = peers.get_mut(peer_id) {
            health.record_success();
        }
    }

    /// Record a failure from peer
    pub fn record_peer_failure(&self, peer_id: &str) {
        let mut peers = self.peer_health.write();
        if let Some(health) = peers.get_mut(peer_id) {
            health.record_failure();
        }
    }

    /// Remove a disconnected peer
    pub fn remove_peer(&self, peer_id: &str) {
        self.peer_health.write().remove(peer_id);
    }

    /// Analyze network and generate optimization recommendations
    pub fn analyze(&self) -> NetworkHealth {
        let peers = self.peer_health.read();
        let config = self.config.read();
        
        let mut issues = Vec::new();
        let mut recommendations = Vec::new();

        // Calculate basic metrics
        let peer_count = peers.len();
        let bootnode_connected = peers.values().any(|p| p.is_bootnode && p.score > 20.0);
        
        let avg_latency = if peers.is_empty() {
            0.0
        } else {
            peers.values().map(|p| p.avg_latency()).sum::<f64>() / peers.len() as f64
        };

        let avg_score = if peers.is_empty() {
            0.0
        } else {
            peers.values().map(|p| p.score).sum::<f64>() / peers.len() as f64
        };

        // Check for critical issues
        if peer_count == 0 {
            issues.push(NetworkIssue {
                severity: IssueSeverity::Critical,
                category: "Connectivity".to_string(),
                description: "No peers connected".to_string(),
                suggested_action: "Check internet connection and firewall settings".to_string(),
            });
            recommendations.push("Restart the node to attempt fresh connections".to_string());
        } else if peer_count < config.min_peers {
            issues.push(NetworkIssue {
                severity: IssueSeverity::Warning,
                category: "Connectivity".to_string(),
                description: format!("Low peer count: {} (minimum: {})", peer_count, config.min_peers),
                suggested_action: "Enable relay mode or check firewall".to_string(),
            });
        }

        if !bootnode_connected && peer_count > 0 {
            issues.push(NetworkIssue {
                severity: IssueSeverity::Warning,
                category: "Bootstrap".to_string(),
                description: "Not connected to any bootnode".to_string(),
                suggested_action: "Attempt to reconnect to bootnodes for better network stability".to_string(),
            });
            recommendations.push("Consider enabling RelayFirst mode for better connectivity".to_string());
        }

        if avg_latency > config.max_latency_ms as f64 {
            issues.push(NetworkIssue {
                severity: IssueSeverity::Warning,
                category: "Performance".to_string(),
                description: format!("High average latency: {:.0}ms", avg_latency),
                suggested_action: "Consider dropping high-latency peers".to_string(),
            });
        }

        // Find poorly performing peers
        let poor_peers: Vec<_> = peers.values()
            .filter(|p| p.score < config.min_peer_score && !p.is_bootnode)
            .collect();
        
        if !poor_peers.is_empty() {
            recommendations.push(format!(
                "{} peer(s) have low scores and may be degrading network performance",
                poor_peers.len()
            ));
        }

        // Calculate overall score
        let connectivity_score = if peer_count >= config.target_peers {
            30.0
        } else {
            (peer_count as f64 / config.target_peers as f64) * 30.0
        };

        let latency_score = if avg_latency == 0.0 {
            0.0
        } else if avg_latency < 100.0 {
            30.0
        } else if avg_latency < 200.0 {
            20.0
        } else if avg_latency < 500.0 {
            10.0
        } else {
            0.0
        };

        let bootnode_score = if bootnode_connected { 20.0 } else { 0.0 };
        
        let peer_quality_score = (avg_score / 100.0) * 20.0;

        let overall_score = connectivity_score + latency_score + bootnode_score + peer_quality_score;

        // Generate proactive recommendations
        if overall_score > 80.0 && issues.is_empty() {
            recommendations.push("Network health is excellent - no optimization needed".to_string());
        } else if overall_score > 60.0 {
            recommendations.push("Network health is good - minor optimizations possible".to_string());
        } else if overall_score > 40.0 {
            recommendations.push("Network health is moderate - consider the suggested actions".to_string());
        } else {
            recommendations.push("Network health needs attention - apply suggested fixes".to_string());
        }

        let health = NetworkHealth {
            overall_score,
            peer_count,
            bootnode_connected,
            avg_latency_ms: avg_latency,
            block_propagation_time_ms: None,
            mesh_stability: 100.0 - (issues.len() as f64 * 10.0).min(50.0),
            issues,
            recommendations,
        };

        *self.network_health.write() = health.clone();
        health
    }

    /// Get the next recommended optimization action
    pub fn get_optimization_action(&self) -> OptimizationAction {
        if !*self.enabled.read() {
            return OptimizationAction::NoAction;
        }

        let health = self.analyze();
        let peers = self.peer_health.read();
        let config = self.config.read();

        // Critical: No peers
        if health.peer_count == 0 {
            return OptimizationAction::RestartNetwork {
                reason: "No peers connected - network restart recommended".to_string(),
            };
        }

        // No bootnode connection
        if !health.bootnode_connected {
            return OptimizationAction::ReconnectBootnode {
                ip: "209.38.137.105".to_string(),
                reason: "Lost connection to all bootnodes".to_string(),
            };
        }

        // Find and drop worst performing non-bootnode peer if we have enough peers
        if health.peer_count > config.min_peers {
            if let Some(worst) = peers.values()
                .filter(|p| !p.is_bootnode && p.score < config.min_peer_score)
                .min_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))
            {
                return OptimizationAction::DropPeer {
                    peer_id: worst.peer_id.clone(),
                    reason: format!("Low peer score: {:.1}", worst.score),
                };
            }
        }

        // Suggest relay mode if peer count is low
        if health.peer_count < config.min_peers {
            return OptimizationAction::ToggleRelay {
                enable: true,
                reason: "Low peer count - relay mode can help establish more connections".to_string(),
            };
        }

        OptimizationAction::NoAction
    }

    /// Generate a summary for NEURAX chat
    pub fn get_summary(&self) -> String {
        let health = self.network_health.read();
        let peers = self.peer_health.read();

        let mut summary = format!(
            "**Network Health: {:.0}%**\n\n",
            health.overall_score
        );

        summary.push_str(&format!("- Connected Peers: {}\n", health.peer_count));
        summary.push_str(&format!("- Bootnode Connected: {}\n", if health.bootnode_connected { "Yes ✓" } else { "No ✗" }));
        summary.push_str(&format!("- Average Latency: {:.0}ms\n", health.avg_latency_ms));
        summary.push_str(&format!("- Mesh Stability: {:.0}%\n", health.mesh_stability));

        if !health.issues.is_empty() {
            summary.push_str("\n**Issues:**\n");
            for issue in &health.issues {
                let icon = match issue.severity {
                    IssueSeverity::Critical => "🔴",
                    IssueSeverity::Warning => "🟡",
                    IssueSeverity::Info => "🔵",
                };
                summary.push_str(&format!("{} {}: {}\n", icon, issue.category, issue.description));
            }
        }

        if !health.recommendations.is_empty() {
            summary.push_str("\n**Recommendations:**\n");
            for rec in &health.recommendations {
                summary.push_str(&format!("• {}\n", rec));
            }
        }

        // Top peers by score
        let mut top_peers: Vec<_> = peers.values().collect();
        top_peers.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
        if !top_peers.is_empty() {
            summary.push_str("\n**Top Peers:**\n");
            for peer in top_peers.iter().take(3) {
                let label = if peer.is_bootnode { " (bootnode)" } else { "" };
                summary.push_str(&format!(
                    "• ...{}: {:.0}ms, score {:.0}{}\n",
                    &peer.peer_id[peer.peer_id.len().saturating_sub(6)..],
                    peer.avg_latency(),
                    peer.score,
                    label
                ));
            }
        }

        summary
    }
}

impl Default for MeshOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tauri Commands for Mesh Optimizer
// ============================================================================

use tauri::State;

/// Wrapper for mesh optimizer state
pub struct MeshOptimizerWrapper(pub Arc<MeshOptimizer>);

#[tauri::command]
pub async fn neurax_get_mesh_health(
    state: State<'_, MeshOptimizerWrapper>,
) -> Result<NetworkHealth, String> {
    Ok(state.0.analyze())
}

#[tauri::command]
pub async fn neurax_get_optimization_action(
    state: State<'_, MeshOptimizerWrapper>,
) -> Result<OptimizationAction, String> {
    Ok(state.0.get_optimization_action())
}

#[tauri::command]
pub async fn neurax_get_mesh_summary(
    state: State<'_, MeshOptimizerWrapper>,
) -> Result<String, String> {
    Ok(state.0.get_summary())
}

#[tauri::command]
pub async fn neurax_set_optimizer_config(
    state: State<'_, MeshOptimizerWrapper>,
    config: OptimizerConfig,
) -> Result<(), String> {
    *state.0.config.write() = config;
    Ok(())
}

#[tauri::command]
pub async fn neurax_toggle_optimizer(
    state: State<'_, MeshOptimizerWrapper>,
    enabled: bool,
) -> Result<(), String> {
    *state.0.enabled.write() = enabled;
    Ok(())
}
