//! Network Observability Module
//!
//! Features:
//! - Prometheus metrics export
//! - OpenTelemetry tracing support
//! - Health check endpoints
//! - Structured logging

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use serde::{Deserialize, Serialize};

/// Prometheus-style metric types
#[derive(Debug, Clone)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

/// A single metric
#[derive(Debug, Clone)]
pub struct Metric {
    pub name: String,
    pub help: String,
    pub metric_type: MetricType,
    pub labels: HashMap<String, String>,
}

/// Metrics registry for Prometheus export
pub struct MetricsRegistry {
    pub counters: HashMap<String, Arc<AtomicU64>>,
    pub gauges: HashMap<String, Arc<AtomicU64>>,
    pub histograms: HashMap<String, Vec<f64>>,
    pub labels: HashMap<String, String>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            counters: HashMap::new(),
            gauges: HashMap::new(),
            histograms: HashMap::new(),
            labels: HashMap::new(),
        };
        
        // Pre-register common metrics
        registry.register_counter("pyrax_p2p_messages_received_total", "Total P2P messages received");
        registry.register_counter("pyrax_p2p_messages_sent_total", "Total P2P messages sent");
        registry.register_counter("pyrax_p2p_bytes_received_total", "Total bytes received");
        registry.register_counter("pyrax_p2p_bytes_sent_total", "Total bytes sent");
        registry.register_counter("pyrax_p2p_connections_total", "Total connections made");
        registry.register_counter("pyrax_p2p_disconnections_total", "Total disconnections");
        registry.register_counter("pyrax_blocks_received_total", "Total blocks received");
        registry.register_counter("pyrax_txs_received_total", "Total transactions received");
        
        registry.register_gauge("pyrax_p2p_peers_connected", "Currently connected peers");
        registry.register_gauge("pyrax_p2p_mesh_peers", "Peers in gossip mesh");
        registry.register_gauge("pyrax_p2p_inbound_connections", "Inbound connections");
        registry.register_gauge("pyrax_p2p_outbound_connections", "Outbound connections");
        registry.register_gauge("pyrax_chain_height", "Current chain height");
        registry.register_gauge("pyrax_mempool_size", "Transactions in mempool");
        registry.register_gauge("pyrax_sync_progress_percent", "Sync progress percentage");
        
        registry
    }
    
    pub fn register_counter(&mut self, name: &str, _help: &str) {
        self.counters.insert(name.to_string(), Arc::new(AtomicU64::new(0)));
    }
    
    pub fn register_gauge(&mut self, name: &str, _help: &str) {
        self.gauges.insert(name.to_string(), Arc::new(AtomicU64::new(0)));
    }
    
    pub fn inc_counter(&self, name: &str) {
        if let Some(counter) = self.counters.get(name) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    pub fn add_counter(&self, name: &str, value: u64) {
        if let Some(counter) = self.counters.get(name) {
            counter.fetch_add(value, Ordering::Relaxed);
        }
    }
    
    pub fn set_gauge(&self, name: &str, value: u64) {
        if let Some(gauge) = self.gauges.get(name) {
            gauge.store(value, Ordering::Relaxed);
        }
    }
    
    pub fn get_counter(&self, name: &str) -> u64 {
        self.counters.get(name).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0)
    }
    
    pub fn get_gauge(&self, name: &str) -> u64 {
        self.gauges.get(name).map(|g| g.load(Ordering::Relaxed)).unwrap_or(0)
    }
    
    /// Export metrics in Prometheus text format
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();
        
        for (name, counter) in &self.counters {
            output.push_str(&format!("# TYPE {} counter\n", name));
            output.push_str(&format!("{} {}\n", name, counter.load(Ordering::Relaxed)));
        }
        
        for (name, gauge) in &self.gauges {
            output.push_str(&format!("# TYPE {} gauge\n", name));
            output.push_str(&format!("{} {}\n", name, gauge.load(Ordering::Relaxed)));
        }
        
        output
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self { Self::new() }
}

/// Health check status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub checks: HashMap<String, CheckResult>,
    pub version: String,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub healthy: bool,
    pub message: String,
}

/// Health checker
pub struct HealthChecker {
    pub start_time: Instant,
    pub version: String,
}

impl HealthChecker {
    pub fn new(version: String) -> Self {
        Self {
            start_time: Instant::now(),
            version,
        }
    }
    
    pub fn check(&self, peer_count: usize, chain_height: u64, synced: bool) -> HealthStatus {
        let mut checks = HashMap::new();
        
        checks.insert("peers".to_string(), CheckResult {
            healthy: peer_count >= 1,
            message: format!("{} peers connected", peer_count),
        });
        
        checks.insert("chain".to_string(), CheckResult {
            healthy: chain_height > 0,
            message: format!("Height: {}", chain_height),
        });
        
        checks.insert("sync".to_string(), CheckResult {
            healthy: synced,
            message: if synced { "Synced".to_string() } else { "Syncing".to_string() },
        });
        
        let all_healthy = checks.values().all(|c| c.healthy);
        
        HealthStatus {
            status: if all_healthy { "healthy".to_string() } else { "degraded".to_string() },
            checks,
            version: self.version.clone(),
            uptime_secs: self.start_time.elapsed().as_secs(),
        }
    }
    
    pub fn liveness(&self) -> bool {
        true // Node is alive if this code runs
    }
    
    pub fn readiness(&self, peer_count: usize, synced: bool) -> bool {
        peer_count >= 1 && synced
    }
}

impl Default for HealthChecker {
    fn default() -> Self { Self::new(env!("CARGO_PKG_VERSION").to_string()) }
}

/// Observability manager
pub struct Observability {
    pub metrics: MetricsRegistry,
    pub health: HealthChecker,
    pub tracing_enabled: bool,
}

impl Observability {
    pub fn new(version: String) -> Self {
        Self {
            metrics: MetricsRegistry::new(),
            health: HealthChecker::new(version),
            tracing_enabled: false,
        }
    }
    
    // Convenience methods for common metrics
    pub fn record_message_received(&self) {
        self.metrics.inc_counter("pyrax_p2p_messages_received_total");
    }
    
    pub fn record_message_sent(&self) {
        self.metrics.inc_counter("pyrax_p2p_messages_sent_total");
    }
    
    pub fn record_bytes_received(&self, bytes: u64) {
        self.metrics.add_counter("pyrax_p2p_bytes_received_total", bytes);
    }
    
    pub fn record_bytes_sent(&self, bytes: u64) {
        self.metrics.add_counter("pyrax_p2p_bytes_sent_total", bytes);
    }
    
    pub fn record_connection(&self) {
        self.metrics.inc_counter("pyrax_p2p_connections_total");
    }
    
    pub fn record_disconnection(&self) {
        self.metrics.inc_counter("pyrax_p2p_disconnections_total");
    }
    
    pub fn update_peer_count(&self, count: usize) {
        self.metrics.set_gauge("pyrax_p2p_peers_connected", count as u64);
    }
    
    pub fn update_chain_height(&self, height: u64) {
        self.metrics.set_gauge("pyrax_chain_height", height);
    }
    
    pub fn update_sync_progress(&self, percent: u64) {
        self.metrics.set_gauge("pyrax_sync_progress_percent", percent);
    }
}

impl Default for Observability {
    fn default() -> Self { Self::new(env!("CARGO_PKG_VERSION").to_string()) }
}
