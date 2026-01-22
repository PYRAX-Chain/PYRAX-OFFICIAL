//! Adaptive Performance Module
//!
//! Features:
//! - Hardware detection (CPU, RAM, bandwidth)
//! - Battery mode for laptops/phones
//! - Bandwidth throttling for metered connections
//! - Disk space management
//! - Memory pressure response

use std::time::{Duration, Instant};
use tracing::{info, warn, debug};

/// System capabilities detected
#[derive(Debug, Clone)]
pub struct SystemCapabilities {
    pub cpu_cores: usize,
    pub total_memory_mb: u64,
    pub available_memory_mb: u64,
    pub disk_free_gb: u64,
    pub is_laptop: bool,
    pub is_on_battery: bool,
    pub detected_at: Instant,
}

impl Default for SystemCapabilities {
    fn default() -> Self {
        Self {
            cpu_cores: num_cpus::get(),
            total_memory_mb: 8192,
            available_memory_mb: 4096,
            disk_free_gb: 100,
            is_laptop: false,
            is_on_battery: false,
            detected_at: Instant::now(),
        }
    }
}

impl SystemCapabilities {
    pub fn detect() -> Self {
        let cpu_cores = num_cpus::get();
        
        // Try to get memory info
        let (total_mem, avail_mem) = Self::get_memory_info();
        let disk_free = Self::get_disk_free();
        let (is_laptop, on_battery) = Self::get_power_info();
        
        Self {
            cpu_cores,
            total_memory_mb: total_mem,
            available_memory_mb: avail_mem,
            disk_free_gb: disk_free,
            is_laptop,
            is_on_battery: on_battery,
            detected_at: Instant::now(),
        }
    }
    
    fn get_memory_info() -> (u64, u64) {
        // Platform-specific memory detection
        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
                let mut total = 0u64;
                let mut available = 0u64;
                for line in content.lines() {
                    if line.starts_with("MemTotal:") {
                        total = line.split_whitespace().nth(1)
                            .and_then(|s| s.parse().ok()).unwrap_or(0) / 1024;
                    }
                    if line.starts_with("MemAvailable:") {
                        available = line.split_whitespace().nth(1)
                            .and_then(|s| s.parse().ok()).unwrap_or(0) / 1024;
                    }
                }
                return (total, available);
            }
        }
        (8192, 4096) // Default fallback
    }
    
    fn get_disk_free() -> u64 {
        // Cross-platform disk space check not easily available without external crates
        // Return a reasonable default - actual disk usage is handled by storage layer
        100 // Default 100GB assumed available
    }
    
    fn get_power_info() -> (bool, bool) {
        #[cfg(target_os = "linux")]
        {
            // Check for battery
            let battery_path = std::path::Path::new("/sys/class/power_supply/BAT0");
            if battery_path.exists() {
                let on_battery = std::fs::read_to_string("/sys/class/power_supply/BAT0/status")
                    .map(|s| s.trim() == "Discharging")
                    .unwrap_or(false);
                return (true, on_battery);
            }
        }
        (false, false)
    }
    
    pub fn performance_tier(&self) -> PerformanceTier {
        if self.cpu_cores >= 8 && self.total_memory_mb >= 16384 {
            PerformanceTier::High
        } else if self.cpu_cores >= 4 && self.total_memory_mb >= 8192 {
            PerformanceTier::Medium
        } else if self.cpu_cores >= 2 && self.total_memory_mb >= 4096 {
            PerformanceTier::Low
        } else {
            PerformanceTier::Minimal
        }
    }
}

/// Performance tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceTier {
    High,
    Medium,
    Low,
    Minimal,
}

impl PerformanceTier {
    pub fn recommended_peers(&self) -> usize {
        match self {
            PerformanceTier::High => 100,
            PerformanceTier::Medium => 50,
            PerformanceTier::Low => 25,
            PerformanceTier::Minimal => 10,
        }
    }
    
    pub fn cache_size_mb(&self) -> usize {
        match self {
            PerformanceTier::High => 512,
            PerformanceTier::Medium => 256,
            PerformanceTier::Low => 128,
            PerformanceTier::Minimal => 64,
        }
    }
    
    pub fn concurrent_syncs(&self) -> usize {
        match self {
            PerformanceTier::High => 8,
            PerformanceTier::Medium => 4,
            PerformanceTier::Low => 2,
            PerformanceTier::Minimal => 1,
        }
    }
}

/// Power mode for battery optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerMode {
    #[default]
    Normal,
    BatterySaver,
    LowPower,
    Performance,
}

impl PowerMode {
    pub fn sync_interval(&self) -> Duration {
        match self {
            PowerMode::Performance => Duration::from_secs(5),
            PowerMode::Normal => Duration::from_secs(10),
            PowerMode::BatterySaver => Duration::from_secs(30),
            PowerMode::LowPower => Duration::from_secs(60),
        }
    }
    
    pub fn ping_interval(&self) -> Duration {
        match self {
            PowerMode::Performance => Duration::from_secs(30),
            PowerMode::Normal => Duration::from_secs(45),
            PowerMode::BatterySaver => Duration::from_secs(120),
            PowerMode::LowPower => Duration::from_secs(300),
        }
    }
}

/// Bandwidth management
#[derive(Debug, Clone)]
pub struct BandwidthManager {
    pub is_metered: bool,
    pub max_upload_kbps: Option<u32>,
    pub max_download_kbps: Option<u32>,
    pub bytes_sent_today: u64,
    pub bytes_received_today: u64,
    pub daily_limit_mb: Option<u64>,
    pub last_reset: Instant,
}

impl Default for BandwidthManager {
    fn default() -> Self {
        Self {
            is_metered: false,
            max_upload_kbps: None,
            max_download_kbps: None,
            bytes_sent_today: 0,
            bytes_received_today: 0,
            daily_limit_mb: None,
            last_reset: Instant::now(),
        }
    }
}

impl BandwidthManager {
    pub fn record_sent(&mut self, bytes: u64) {
        self.bytes_sent_today += bytes;
    }
    
    pub fn record_received(&mut self, bytes: u64) {
        self.bytes_received_today += bytes;
    }
    
    pub fn is_over_limit(&self) -> bool {
        if let Some(limit) = self.daily_limit_mb {
            let total_mb = (self.bytes_sent_today + self.bytes_received_today) / (1024 * 1024);
            return total_mb >= limit;
        }
        false
    }
    
    pub fn usage_percent(&self) -> f32 {
        if let Some(limit) = self.daily_limit_mb {
            let total_mb = (self.bytes_sent_today + self.bytes_received_today) / (1024 * 1024);
            return (total_mb as f32 / limit as f32 * 100.0).min(100.0);
        }
        0.0
    }
}

/// Adaptive performance manager
pub struct AdaptivePerformance {
    pub capabilities: SystemCapabilities,
    pub power_mode: PowerMode,
    pub bandwidth: BandwidthManager,
    pub auto_tune: bool,
    pub last_tune: Instant,
}

impl AdaptivePerformance {
    pub fn new() -> Self {
        Self {
            capabilities: SystemCapabilities::detect(),
            power_mode: PowerMode::Normal,
            bandwidth: BandwidthManager::default(),
            auto_tune: true,
            last_tune: Instant::now(),
        }
    }
    
    pub fn auto_tune(&mut self) {
        if !self.auto_tune { return; }
        
        // Re-detect capabilities periodically
        if self.last_tune.elapsed() > Duration::from_secs(300) {
            self.capabilities = SystemCapabilities::detect();
            self.last_tune = Instant::now();
        }
        
        // Auto-switch power mode
        if self.capabilities.is_on_battery {
            self.power_mode = PowerMode::BatterySaver;
        } else if self.capabilities.available_memory_mb < 1024 {
            self.power_mode = PowerMode::LowPower;
        } else {
            self.power_mode = PowerMode::Normal;
        }
    }
    
    pub fn recommended_config(&self) -> AdaptiveConfig {
        let tier = self.capabilities.performance_tier();
        AdaptiveConfig {
            max_peers: tier.recommended_peers(),
            cache_size_mb: tier.cache_size_mb(),
            concurrent_syncs: tier.concurrent_syncs(),
            sync_interval: self.power_mode.sync_interval(),
            ping_interval: self.power_mode.ping_interval(),
        }
    }
}

impl Default for AdaptivePerformance {
    fn default() -> Self { Self::new() }
}

/// Recommended configuration based on system
#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    pub max_peers: usize,
    pub cache_size_mb: usize,
    pub concurrent_syncs: usize,
    pub sync_interval: Duration,
    pub ping_interval: Duration,
}
