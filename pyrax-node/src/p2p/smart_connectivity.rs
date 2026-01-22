//! Smart Connectivity Module - Next-Level P2P Experience
//!
//! Features:
//! - ISP Detection & Bypass
//! - Captive Portal Detection  
//! - Connection Quality Scoring
//! - Multi-Strategy NAT Punching
//! - DNS-based Bootstrap Discovery

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tracing::{info, warn, debug};

/// ISP types that affect connectivity
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum IspType {
    #[default]
    Standard,
    ThrottlesP2P,
    BlocksNonHttp,
    UsesDPI,
    Corporate,
    MobileCarrier,
    Educational,
    Unknown,
}

/// Transport protocols
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Quic,
    Quic443,
    WebSocket,
    WebSocket443,
    HttpTunnel,
    Relay,
}

impl Protocol {
    pub fn default_port(&self) -> u16 {
        match self {
            Protocol::Tcp => 30303,
            Protocol::Quic | Protocol::Quic443 => 443,
            Protocol::WebSocket => 30304,
            Protocol::WebSocket443 => 443,
            Protocol::HttpTunnel => 80,
            Protocol::Relay => 30303,
        }
    }
}

impl IspType {
    pub fn recommended_protocols(&self) -> Vec<Protocol> {
        match self {
            IspType::Standard => vec![Protocol::Tcp, Protocol::Quic],
            IspType::ThrottlesP2P => vec![Protocol::Quic, Protocol::WebSocket],
            IspType::BlocksNonHttp => vec![Protocol::WebSocket443, Protocol::HttpTunnel],
            IspType::UsesDPI => vec![Protocol::Quic443, Protocol::Relay],
            IspType::Corporate => vec![Protocol::WebSocket443, Protocol::Relay],
            IspType::MobileCarrier => vec![Protocol::Quic, Protocol::Relay],
            IspType::Educational => vec![Protocol::WebSocket443, Protocol::Relay],
            IspType::Unknown => vec![Protocol::Tcp, Protocol::Quic, Protocol::Relay],
        }
    }
}

/// ISP detection info
#[derive(Debug, Clone, Default)]
pub struct IspInfo {
    pub isp_type: IspType,
    pub isp_name: Option<String>,
    pub country: Option<String>,
    pub is_vpn: bool,
}

/// Captive portal status
#[derive(Debug, Clone, Default)]
pub struct CaptivePortalStatus {
    pub is_captive: bool,
    pub portal_url: Option<String>,
}

/// Connection quality metrics
#[derive(Debug, Clone)]
pub struct ConnectionQuality {
    pub latency_ms: u32,
    pub jitter_ms: u32,
    pub packet_loss_pct: f32,
    pub bandwidth_kbps: u32,
    pub stability_score: u8,
    pub last_measured: Instant,
}

impl Default for ConnectionQuality {
    fn default() -> Self {
        Self {
            latency_ms: 0,
            jitter_ms: 0,
            packet_loss_pct: 0.0,
            bandwidth_kbps: 0,
            stability_score: 50,
            last_measured: Instant::now(),
        }
    }
}

impl ConnectionQuality {
    pub fn overall_score(&self) -> u8 {
        let latency_score = (100 - (self.latency_ms / 10).min(100)) as u8;
        let loss_score = (100.0 - self.packet_loss_pct * 10.0).max(0.0) as u8;
        ((latency_score as u16 + loss_score as u16 + self.stability_score as u16) / 3) as u8
    }
}

/// Peer quality tracker
#[derive(Debug, Clone)]
pub struct PeerQuality {
    pub peer_id: String,
    pub quality: ConnectionQuality,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub connected_since: Instant,
}

impl PeerQuality {
    pub fn new(peer_id: String) -> Self {
        Self {
            peer_id,
            quality: ConnectionQuality::default(),
            successful_requests: 0,
            failed_requests: 0,
            bytes_sent: 0,
            bytes_received: 0,
            connected_since: Instant::now(),
        }
    }
    
    pub fn success_rate(&self) -> f32 {
        let total = self.successful_requests + self.failed_requests;
        if total == 0 { 1.0 } else { self.successful_requests as f32 / total as f32 }
    }
    
    pub fn reliability_score(&self) -> u8 {
        let uptime_bonus = (self.connected_since.elapsed().as_secs() / 60).min(20) as u8;
        let success_score = (self.success_rate() * 80.0) as u8;
        (success_score + uptime_bonus).min(100)
    }
}

/// Smart connectivity manager
pub struct SmartConnectivity {
    pub isp_info: IspInfo,
    pub captive_status: CaptivePortalStatus,
    pub peer_quality: HashMap<String, PeerQuality>,
    pub protocol_success: HashMap<Protocol, (u32, u32)>, // (success, fail)
    pub best_protocol: Option<Protocol>,
}

impl SmartConnectivity {
    pub fn new() -> Self {
        Self {
            isp_info: IspInfo::default(),
            captive_status: CaptivePortalStatus::default(),
            peer_quality: HashMap::new(),
            protocol_success: HashMap::new(),
            best_protocol: None,
        }
    }
    
    pub fn record_protocol_result(&mut self, protocol: Protocol, success: bool) {
        let entry = self.protocol_success.entry(protocol).or_insert((0, 0));
        if success { entry.0 += 1; } else { entry.1 += 1; }
        self.update_best_protocol();
    }
    
    fn update_best_protocol(&mut self) {
        self.best_protocol = self.protocol_success.iter()
            .filter(|(_, (s, f))| s + f >= 3)
            .max_by_key(|(_, (s, f))| s * 100 / (s + f + 1))
            .map(|(p, _)| *p);
    }
    
    pub fn get_peer_quality(&mut self, peer_id: &str) -> &mut PeerQuality {
        self.peer_quality.entry(peer_id.to_string())
            .or_insert_with(|| PeerQuality::new(peer_id.to_string()))
    }
    
    pub fn best_peers(&self, count: usize) -> Vec<&PeerQuality> {
        let mut peers: Vec<_> = self.peer_quality.values().collect();
        peers.sort_by(|a, b| b.reliability_score().cmp(&a.reliability_score()));
        peers.into_iter().take(count).collect()
    }
}

impl Default for SmartConnectivity {
    fn default() -> Self { Self::new() }
}
