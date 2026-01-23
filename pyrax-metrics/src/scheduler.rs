//! Scheduled Tasks Module
//!
//! Handles periodic tasks like daily summaries and disk space monitoring.

use crate::state::AppState;
use std::time::Duration;
use tokio::time::{interval, Instant};
use tracing::{info, warn};

/// Scheduled task runner
pub struct Scheduler {
    app_state: AppState,
}

impl Scheduler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }
    
    /// Run all scheduled tasks
    pub async fn run(&self) -> anyhow::Result<()> {
        // Run disk check every 5 minutes
        let disk_check_interval = Duration::from_secs(5 * 60);
        // Run daily summary every 24 hours
        let daily_summary_interval = Duration::from_secs(24 * 60 * 60);
        
        let mut disk_interval = interval(disk_check_interval);
        let mut summary_interval = interval(daily_summary_interval);
        
        info!("Starting scheduler: Disk check every 5min, Daily summary every 24h");
        
        loop {
            tokio::select! {
                _ = disk_interval.tick() => {
                    self.check_disk_space().await;
                }
                _ = summary_interval.tick() => {
                    self.send_daily_summary().await;
                }
            }
        }
    }
    
    /// Check disk space and alert if low
    async fn check_disk_space(&self) {
        // Use sys-info or parse /proc/diskstats
        // For simplicity, we'll use the `df` command
        match tokio::process::Command::new("df")
            .args(["-h", "/"])
            .output()
            .await
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse df output to get usage percentage
                // Format: Filesystem Size Used Avail Use% Mounted
                for line in stdout.lines().skip(1) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let usage_str = parts[4].trim_end_matches('%');
                        if let Ok(usage) = usage_str.parse::<u8>() {
                            if usage >= 90 {
                                warn!("Disk space critically low: {}%", usage);
                                let msg = format!(
                                    "⚠️ <b>Disk Space Low</b>\nUsage: {}%\nAvailable: {}",
                                    usage,
                                    parts.get(3).unwrap_or(&"?")
                                );
                                self.app_state.alert_manager()
                                    .send_alert("Disk Space", &msg).await;
                            } else if usage >= 80 {
                                info!("Disk space warning: {}%", usage);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Failed to check disk space: {}", e);
            }
        }
    }
    
    /// Send daily network health summary
    async fn send_daily_summary(&self) {
        let chain = self.app_state.chain_state();
        let nodes = self.app_state.node_statuses();
        let discovered = self.app_state.discovered_nodes();
        
        let online_count = nodes.iter().filter(|n| n.reachable).count();
        let synced_count = nodes.iter().filter(|n| n.reachable && !n.syncing).count();
        
        let status_emoji = if chain.is_stalled {
            "🔴"
        } else if online_count < nodes.len() {
            "🟡"
        } else {
            "🟢"
        };
        
        let msg = format!(
            "📊 <b>Daily Network Summary</b>\n\n\
            {} Status: {}\n\
            📦 Block Height: {}\n\
            ⏱ Avg Latency: {}ms\n\
            🖥 Nodes Online: {}/{}\n\
            ✅ Nodes Synced: {}\n\
            🌐 Discovered Peers: {}\n\
            ⛏ Network Hashrate: {:.2} MH/s\n\n\
            {}",
            status_emoji,
            if chain.is_stalled { "Stalled" } else { "Healthy" },
            chain.head_block,
            chain.avg_latency_ms,
            online_count,
            nodes.len(),
            synced_count,
            discovered.len(),
            chain.network_hashrate / 1_000_000.0,
            chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
        );
        
        info!("Sending daily summary");
        self.app_state.alert_manager()
            .send_alert("Daily Summary", &msg).await;
    }
}
