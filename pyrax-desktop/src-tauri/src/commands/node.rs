use crate::state::AppState;
use crate::rpc::RpcClient;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::process::{Command, Stdio};
use std::fs;
use std::io::{BufRead, BufReader};
use tauri::{State, Manager, AppHandle};
use tracing::{info, error, warn, debug};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Remote server log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteLogEntry {
    pub timestamp: String,
    pub level: String,
    pub category: String,
    pub message: String,
}

/// Remote server status including logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteServerStatus {
    pub online: bool,
    pub streams: StreamStatus,
    pub peer_count: u32,
    pub block_height: u64,
    pub logs: Vec<RemoteLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamStatus {
    pub stream_a_rpc: bool,
    pub stream_b_stratum: bool,
    pub stream_c_staking: bool,
}

#[derive(Clone, Serialize)]
struct LogPayload {
    level: String,
    category: String,
    message: String,
}

fn emit_log(app: &AppHandle, level: &str, category: &str, message: &str) {
    let _ = app.emit_all("node-log", LogPayload {
        level: level.to_string(),
        category: category.to_string(),
        message: message.to_string(),
    });
}

/// Check if a log line should be filtered out (noise reduction)
fn should_filter_log(line: &str) -> bool {
    // Filter out repetitive dial failure messages for NAT'd peers
    // These create noise when peers behind NAT are discovered but unreachable
    if line.contains("Connection attempt to peer failed") && line.contains("ConnectionRefused") {
        return true;
    }
    if line.contains("Dial to") && line.contains("failed") && !line.contains("bootnode") {
        return true;
    }
    if line.contains("Dial failure for") && !line.contains("bootnode") {
        return true;
    }
    // Filter out noisy swarm polling messages
    if line.contains("dialing address") && !line.contains("209.38.137.105") && !line.contains("137.184.118.228") {
        return true;
    }
    // Filter out ANSI escape codes spam
    if line.contains("[1mSwarm::poll[0m") && (line.contains("ConnectionRefused") || line.contains("Timeout")) {
        return true;
    }
    false
}

/// Parse pyrax-node tracing log format
/// Example: "2026-01-16T05:12:43.406091Z  INFO Connected to peer: 12D3KooW..."
/// Returns (level, category, message) - returns empty strings if should be filtered
fn parse_node_log(line: &str) -> (String, String, String) {
    // Tracing format: TIMESTAMP LEVEL [target] message
    // or: TIMESTAMP LEVEL message
    let line = line.trim();
    
    // Filter out noisy logs
    if should_filter_log(line) {
        return (String::new(), String::new(), String::new());
    }
    
    // Skip timestamp (ISO 8601 format)
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return ("info".to_string(), "node".to_string(), line.to_string());
    }
    
    // Extract level (INFO, WARN, ERROR, DEBUG, TRACE)
    let level_str = parts.get(1).unwrap_or(&"INFO").trim();
    let level = match level_str.to_uppercase().as_str() {
        "INFO" => "info",
        "WARN" | "WARNING" => "warn",
        "ERROR" => "error",
        "DEBUG" | "TRACE" => "debug",
        _ => "info",
    };
    
    // Get message (rest of line after level)
    let message = parts.get(2).unwrap_or(&line).trim().to_string();
    
    // Detect category from message content
    // Categories: p2p, block, rpc, mining, staking, node (default)
    let category = if message.contains("peer") || message.contains("Peer") || 
                      message.contains("P2P") || message.contains("Kademlia") ||
                      message.contains("Connected to") || message.contains("Disconnected") ||
                      message.contains("mDNS") || message.contains("DHT") ||
                      message.contains("GossipSub") || message.contains("mesh") ||
                      message.contains("MESH") || message.contains("relay") ||
                      message.contains("Relay") || message.contains("NAT") ||
                      message.contains("AutoNAT") || message.contains("bootnode") ||
                      message.contains("Bootnode") || message.contains("dial") ||
                      message.contains("Dial") || message.contains("listen") ||
                      message.contains("Listen") || message.contains("swarm") ||
                      message.contains("Swarm") || message.contains("circuit") ||
                      message.contains("reservation") || message.contains("UPnP") ||
                      message.contains("DCUtR") || message.contains("hole punch") {
        "p2p"
    } else if message.contains("block") || message.contains("Block") || 
              message.contains("height") || message.contains("sync") ||
              message.contains("Sync") || message.contains("chain") ||
              message.contains("Chain") || message.contains("genesis") ||
              message.contains("Genesis") || message.contains("tip") ||
              message.contains("orphan") || message.contains("reorg") ||
              message.contains("fork") || message.contains("UTXO") {
        "block"
    } else if message.contains("RPC") || message.contains("rpc") ||
              message.contains("JSON") || message.contains("request") ||
              message.contains("endpoint") || message.contains("API") ||
              message.contains("WebSocket") || message.contains("ws://") {
        "rpc"
    } else if message.contains("mining") || message.contains("Mining") ||
              message.contains("Stratum") || message.contains("worker") ||
              message.contains("Worker") || message.contains("KAWPOW") ||
              message.contains("BLAKE3") || message.contains("hashrate") ||
              message.contains("nonce") || message.contains("difficulty") ||
              message.contains("target") || message.contains("share") ||
              message.contains("mined") || message.contains("Mined") {
        "mining"
    } else if message.contains("staking") || message.contains("Staking") ||
              message.contains("stake") || message.contains("ZK") ||
              message.contains("validator") || message.contains("Validator") ||
              message.contains("checkpoint") || message.contains("slash") ||
              message.contains("delegation") || message.contains("reward") {
        "staking"
    } else {
        "node"
    };
    
    (level.to_string(), category.to_string(), message)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatus {
    pub running: bool,
    pub connected: bool,
    pub syncing: bool,
    pub sync_progress: f64,
    pub peer_count: u32,
    pub block_height: u64,
    pub block_hash: String,
    pub network: String,
    pub version: String,
    // Extended P2P stats for realtime connection monitoring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inbound_peers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outbound_peers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_peers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_peers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dial_attempts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dial_successes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dial_failures: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_rtt_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_peers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gossip_peers: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    pub chain_id: u64,
    pub best_block_hash: String,
    pub best_block_height: u64,
    pub genesis_hash: String,
    pub difficulty: String,
    pub total_difficulty: String,
    pub peer_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub id: String,
    pub address: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub direction: String,
    pub connected_secs: u64,
    pub version: String,
    pub block_height: u64,
}

fn get_rpc_port(network: &crate::state::Network) -> u16 {
    match network {
        crate::state::Network::Mainnet => 8545,
        crate::state::Network::Testnet => 18545,
        crate::state::Network::Devnet => 28545,
    }
}

/// Get the remote RPC URL for a network (fallback only)
fn get_remote_rpc_url(network: &crate::state::Network) -> &'static str {
    match network {
        crate::state::Network::Mainnet => "https://rpc.pyrax.org",
        crate::state::Network::Testnet => "https://rpc.pyrax-testnet.org",
        crate::state::Network::Devnet => "http://209.38.137.105:28545", // Digital Ocean bootnode
    }
}

/// Bootnode configuration with geolocation for proximity-based selection
#[derive(Clone)]
struct BootnodeConfig {
    ip: &'static str,
    rpc_port: u16,
    p2p_port: u16,
    lat: f64,
    lon: f64,
    region: &'static str,
}

/// Get bootnode configurations for a network with geolocation data
fn get_bootnode_configs(network: &crate::state::Network) -> Vec<BootnodeConfig> {
    match network {
        crate::state::Network::Mainnet => vec![
            BootnodeConfig { ip: "bootstrap.pyrax.org", rpc_port: 8545, p2p_port: 30303, lat: 40.7128, lon: -74.0060, region: "NYC" },
        ],
        crate::state::Network::Testnet => vec![
            BootnodeConfig { ip: "bootstrap.pyrax-testnet.org", rpc_port: 18545, p2p_port: 30303, lat: 40.7128, lon: -74.0060, region: "NYC" },
        ],
        crate::state::Network::Devnet => vec![
            // Bootnode 1: Digital Ocean NYC
            BootnodeConfig { ip: "209.38.137.105", rpc_port: 28545, p2p_port: 30303, lat: 40.7128, lon: -74.0060, region: "NYC" },
            // Bootnode 2: Digital Ocean SFO
            BootnodeConfig { ip: "137.184.118.228", rpc_port: 28545, p2p_port: 30303, lat: 37.7749, lon: -122.4194, region: "SFO" },
        ],
    }
}

/// User's geolocation from IP lookup
#[derive(Debug, Clone)]
struct UserLocation {
    lat: f64,
    lon: f64,
    city: String,
    country: String,
}

/// Fetch user's geolocation using ip-api.com (free, no API key required)
async fn get_user_location() -> Option<UserLocation> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    
    // Use ip-api.com - free tier, no API key, 45 requests/minute
    let response = client
        .get("http://ip-api.com/json/?fields=status,lat,lon,city,country")
        .send()
        .await
        .ok()?;
    
    let json: serde_json::Value = response.json().await.ok()?;
    
    if json.get("status")?.as_str()? != "success" {
        return None;
    }
    
    Some(UserLocation {
        lat: json.get("lat")?.as_f64()?,
        lon: json.get("lon")?.as_f64()?,
        city: json.get("city")?.as_str()?.to_string(),
        country: json.get("country")?.as_str()?.to_string(),
    })
}

/// Calculate distance between two points using Haversine formula
/// Returns distance in kilometers
fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();
    
    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    
    EARTH_RADIUS_KM * c
}

/// Sort bootnodes by distance from user and return the closest ones first
fn sort_bootnodes_by_distance(configs: Vec<BootnodeConfig>, user_loc: &UserLocation) -> Vec<BootnodeConfig> {
    let mut configs_with_distance: Vec<(BootnodeConfig, f64)> = configs
        .into_iter()
        .map(|config| {
            let distance = haversine_distance(user_loc.lat, user_loc.lon, config.lat, config.lon);
            (config, distance)
        })
        .collect();
    
    // Sort by distance (closest first)
    configs_with_distance.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    
    configs_with_distance.into_iter().map(|(config, _)| config).collect()
}

/// Fetch peer ID from a bootnode's RPC endpoint
async fn fetch_peer_id(ip: &str, rpc_port: u16) -> Option<String> {
    let url = format!("http://{}:{}", ip, rpc_port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(r#"{"jsonrpc":"2.0","method":"pyrax_getPeerId","params":[],"id":1}"#)
        .send()
        .await
        .ok()?;
    
    let json: serde_json::Value = response.json().await.ok()?;
    let peer_id = json.get("result")?.as_str()?;
    
    if peer_id.is_empty() || !peer_id.starts_with("12D3KooW") {
        return None;
    }
    
    Some(peer_id.to_string())
}

/// Dynamically discover bootstrap peers by fetching peer IDs from bootnodes
/// Connects to the 2 closest bootnodes first based on user's geolocation
/// This eliminates hardcoded peer IDs - bootnodes can regenerate keys freely
async fn discover_bootstrap_peers(network: &crate::state::Network) -> Vec<String> {
    let mut configs = get_bootnode_configs(network);
    
    // Get user's location and sort bootnodes by proximity
    match get_user_location().await {
        Some(user_loc) => {
            info!("User location: {}, {} (lat: {:.2}, lon: {:.2})", 
                user_loc.city, user_loc.country, user_loc.lat, user_loc.lon);
            
            // Sort bootnodes by distance (closest first)
            configs = sort_bootnodes_by_distance(configs, &user_loc);
            
            // Log the order of bootnodes by proximity
            for (i, config) in configs.iter().enumerate() {
                let distance = haversine_distance(user_loc.lat, user_loc.lon, config.lat, config.lon);
                info!("Bootnode #{}: {} ({}) - {:.0} km away", 
                    i + 1, config.ip, config.region, distance);
            }
        }
        None => {
            warn!("Could not determine user location - using default bootnode order");
        }
    }
    
    let mut peers = Vec::new();
    
    // Connect to bootnodes in order (closest first)
    for (i, config) in configs.iter().enumerate() {
        match fetch_peer_id(config.ip, config.rpc_port).await {
            Some(peer_id) => {
                let multiaddr = format!("/ip4/{}/tcp/{}/p2p/{}", config.ip, config.p2p_port, peer_id);
                if i < 2 {
                    info!("✓ Priority bootnode #{} ({}): {}", i + 1, config.region, multiaddr);
                } else {
                    info!("Discovered bootnode #{} ({}): {}", i + 1, config.region, multiaddr);
                }
                peers.push(multiaddr);
            }
            None => {
                warn!("Failed to discover peer ID for bootnode {} ({})", config.ip, config.region);
            }
        }
    }
    
    if peers.is_empty() {
        warn!("No bootnodes discovered via RPC - trying cached fallback peer IDs");
        
        // FALLBACK FIX: Use last known working peer IDs if dynamic discovery fails
        // These are periodically updated from successful discoveries
        let fallback_peers = get_fallback_peer_ids(network);
        if !fallback_peers.is_empty() {
            info!("Using {} fallback peer IDs", fallback_peers.len());
            return fallback_peers;
        }
        
        warn!("No fallback peer IDs available - P2P may not work correctly");
    } else {
        info!("Connecting to {} bootnodes (closest {} first)", peers.len(), std::cmp::min(2, peers.len()));
        
        // Cache successful discovery for future fallback
        cache_peer_ids(network, &peers);
    }
    
    peers
}

/// Get cached fallback peer IDs from local storage
/// These are saved from the last successful dynamic discovery
fn get_fallback_peer_ids(network: &crate::state::Network) -> Vec<String> {
    let cache_file = get_peer_cache_path(network);
    
    if let Ok(content) = std::fs::read_to_string(&cache_file) {
        let peers: Vec<String> = content.lines()
            .filter(|line| !line.is_empty() && line.contains("/p2p/"))
            .map(|s| s.to_string())
            .collect();
        
        if !peers.is_empty() {
            info!("Loaded {} cached peer IDs from {:?}", peers.len(), cache_file);
            return peers;
        }
    }
    
    Vec::new()
}

/// Cache successfully discovered peer IDs for fallback
fn cache_peer_ids(network: &crate::state::Network, peers: &[String]) {
    let cache_file = get_peer_cache_path(network);
    
    // Ensure parent directory exists
    if let Some(parent) = cache_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    
    let content = peers.join("\n");
    if let Err(e) = std::fs::write(&cache_file, content) {
        warn!("Failed to cache peer IDs: {}", e);
    } else {
        debug!("Cached {} peer IDs to {:?}", peers.len(), cache_file);
    }
}

/// Get path to peer ID cache file
fn get_peer_cache_path(network: &crate::state::Network) -> std::path::PathBuf {
    let network_name = match network {
        crate::state::Network::Mainnet => "mainnet",
        crate::state::Network::Testnet => "testnet",
        crate::state::Network::Devnet => "devnet",
    };
    
    // Use directories crate for cross-platform data directory
    directories::ProjectDirs::from("com", "pyrax", "pyrax-desktop")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(format!("peer_cache_{}.txt", network_name))
}

/// LINUX/MAC FIX: Ensure binary has executable permission
#[cfg(unix)]
fn ensure_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        let mode = perms.mode();
        // Add execute permission for owner if not already set
        if mode & 0o100 == 0 {
            perms.set_mode(mode | 0o755);
            if let Err(e) = std::fs::set_permissions(path, perms) {
                warn!("Failed to set executable permission on {:?}: {}", path, e);
            } else {
                info!("Set executable permission on {:?}", path);
            }
        }
    }
}

#[cfg(not(unix))]
fn ensure_executable(_path: &std::path::Path) {
    // No-op on Windows
}

fn get_node_binary_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let binary_name = "pyrax-node.exe";
    #[cfg(not(target_os = "windows"))]
    let binary_name = "pyrax-node";
    
    // Check current directory (where the app executable is)
    let current_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();
    
    info!("Looking for {} binary, current_dir: {:?}", binary_name, current_dir);
    
    // Check in same directory as executable
    let local_path = current_dir.join(binary_name);
    if local_path.exists() {
        info!("Found pyrax-node at: {:?}", local_path);
        ensure_executable(&local_path);
        return Some(local_path);
    }
    
    // Check in resources directory (Tauri bundle location)
    let resource_path = current_dir.join("resources").join(binary_name);
    if resource_path.exists() {
        info!("Found pyrax-node in resources: {:?}", resource_path);
        ensure_executable(&resource_path);
        return Some(resource_path);
    }
    
    // LINUX/MAC FIX: Check inside app bundle (macOS .app/Contents/Resources)
    #[cfg(target_os = "macos")]
    {
        let macos_resource_path = current_dir.join("../Resources").join(binary_name);
        if macos_resource_path.exists() {
            info!("Found pyrax-node in macOS bundle: {:?}", macos_resource_path);
            ensure_executable(&macos_resource_path);
            return Some(macos_resource_path);
        }
    }
    
    // Check in _up_/resources (dev mode)
    let dev_resource_path = current_dir.join("..").join("resources").join(binary_name);
    if dev_resource_path.exists() {
        info!("Found pyrax-node in dev resources: {:?}", dev_resource_path);
        return Some(dev_resource_path);
    }
    
    // Check src-tauri/resources (dev mode from target directory)
    let src_tauri_path = std::path::PathBuf::from("pyrax-desktop/src-tauri/resources").join(binary_name);
    if src_tauri_path.exists() {
        info!("Found pyrax-node in src-tauri/resources: {:?}", src_tauri_path);
        return Some(src_tauri_path);
    }
    
    // Try to find in PATH using platform-specific command
    // MAC/LINUX FIX: Use 'which' on Unix, 'where' on Windows
    #[cfg(target_os = "windows")]
    let path_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let path_cmd = "which";
    
    if let Ok(output) = std::process::Command::new(path_cmd).arg(binary_name).output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = path_str.lines().next() {
                let path = std::path::PathBuf::from(first_line.trim());
                if path.exists() {
                    info!("Found pyrax-node in PATH: {:?}", path);
                    return Some(path);
                }
            }
        }
    }
    
    // MAC/LINUX FIX: Also check common Unix installation paths
    #[cfg(not(target_os = "windows"))]
    {
        let unix_paths = [
            "/usr/local/bin/pyrax-node",
            "/usr/bin/pyrax-node",
            "/opt/pyrax/bin/pyrax-node",
            "~/.local/bin/pyrax-node",
        ];
        
        for path_str in unix_paths {
            let path = if path_str.starts_with("~") {
                if let Some(home) = std::env::var_os("HOME") {
                    std::path::PathBuf::from(home).join(&path_str[2..])
                } else {
                    continue;
                }
            } else {
                std::path::PathBuf::from(path_str)
            };
            
            if path.exists() {
                info!("Found pyrax-node at Unix path: {:?}", path);
                return Some(path);
            }
        }
    }
    
    warn!("pyrax-node binary not found");
    None
}

#[tauri::command]
pub async fn start_node(
    app: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<NodeStatus, String> {
    // RELIABILITY FIX: Clean up any stale state before checking
    // This handles cases where the app was force-closed while node was running
    let (network, rpc_port, data_dir, log_verbosity, had_stale_process, 
         connection_mode, p2p_port_setting, enable_websocket, auto_port_fallback) = {
        let mut app_state = state.lock();
        
        // Check if we have a stale process handle
        let mut had_stale = false;
        if let Some(ref mut child) = app_state.node_process {
            // Check if process is actually still running
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Process exited - clean up stale state
                    info!("Cleaning up stale node process state");
                    had_stale = true;
                    app_state.node_running = false;
                }
                Ok(None) => {
                    // Process still running
                    return Err("Node is already running".to_string());
                }
                Err(_) => {
                    // Can't check - assume stale
                    had_stale = true;
                    app_state.node_running = false;
                }
            }
        }
        
        // Clean up stale process handle
        if had_stale {
            app_state.node_process = None;
        }
        
        if app_state.node_running && app_state.node_process.is_none() {
            // Flag is set but no process - reset stale state
            info!("Resetting stale node_running flag");
            app_state.node_running = false;
        }
        
        // Get mass adoption network settings
        let conn_mode = app_state.settings.connection_mode;
        let p2p_port = app_state.settings.p2p_port;
        let ws_enabled = app_state.settings.enable_websocket;
        let auto_fallback = app_state.settings.auto_port_fallback;
        
        (
            app_state.network.clone(),
            get_rpc_port(&app_state.network),
            app_state.data_dir.clone(),
            app_state.settings.log_verbosity,
            had_stale,
            conn_mode,
            p2p_port,
            ws_enabled,
            auto_fallback,
        )
    };
    
    // On Windows, kill any orphaned pyrax-node processes before starting
    #[cfg(target_os = "windows")]
    {
        if had_stale_process {
            emit_log(&app, "info", "node", "Cleaning up orphaned processes...");
            let _ = Command::new("taskkill")
                .args(["/IM", "pyrax-node.exe", "/F"])
                .output();
            // Brief delay to ensure cleanup
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    let _ = had_stale_process; // Suppress unused warning
    
    emit_log(&app, "info", "node", &format!("Starting PYRAX full node on {:?} network...", network));
    info!("Starting PYRAX full node on {:?} network...", network);
    
    // Check if a local node is already running on this port
    emit_log(&app, "info", "rpc", &format!("Checking for existing node on port {}...", rpc_port));
    let local_rpc = RpcClient::localhost(rpc_port);
    if local_rpc.is_connected().await {
        emit_log(&app, "info", "node", &format!("Found existing local node on port {}, connecting...", rpc_port));
        info!("Found existing local node on port {}, connecting...", rpc_port);
        {
            let mut app_state = state.lock();
            app_state.node_running = true;
            app_state.node_process = None; // External node
            app_state.rpc_port = rpc_port;
        }
        
        match local_rpc.get_chain_info().await {
            Ok(info) => {
                emit_log(&app, "info", "block", &format!("Connected to existing node: height={}", info.best_block_height));
                info!("Connected to existing node: network={}, height={}", info.network, info.best_block_height);
                return Ok(NodeStatus {
                    running: true,
                    connected: true,
                    syncing: info.syncing,
                    sync_progress: if info.syncing { 50.0 } else { 100.0 },
                    peer_count: 0,
                    block_height: info.best_block_height,
                    block_hash: info.best_block_hash,
                    network: info.network,
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    inbound_peers: None,
                    outbound_peers: None,
                    target_peers: None,
                    max_peers: None,
                    dial_attempts: None,
                    dial_successes: None,
                    dial_failures: None,
                    average_rtt_ms: None,
                    network_state: None,
                    nat_status: None,
                    mesh_peers: None,
                    gossip_peers: None,
                });
            }
            Err(e) => {
                warn!("Connected but failed to get chain info: {}", e);
            }
        }
    }
    
    // Try to spawn local pyrax-node binary (TRUE DECENTRALIZATION)
    if let Some(binary_path) = get_node_binary_path() {
        emit_log(&app, "info", "node", &format!("Spawning local full node from: {:?}", binary_path));
        info!("Spawning local full node from: {:?}", binary_path);
        
        let network_arg = match network {
            crate::state::Network::Mainnet => "mainnet",
            crate::state::Network::Testnet => "testnet",
            crate::state::Network::Devnet => "devnet",
        };
        
        let rpc_addr = format!("0.0.0.0:{}", rpc_port);
        
        // MASS ADOPTION: Use user-configured P2P port from settings
        // This allows users to bypass ISP blocks on port 30303
        let p2p_port = p2p_port_setting;
        let p2p_addr = format!("/ip4/0.0.0.0/tcp/{}", p2p_port); // libp2p multiaddr format
        let staking_port = rpc_port + 2; // Staking RPC (e.g., 28545 -> 28547)
        let staking_addr = format!("0.0.0.0:{}", staking_port);
        // Use standard stratum port - same across all networks
        let stratum_port = 3333;
        let stratum_addr = format!("0.0.0.0:{}", stratum_port);
        
        // Dynamically discover bootstrap peers (fetches peer IDs from bootnodes)
        emit_log(&app, "info", "p2p", "Discovering bootnode peer IDs...");
        let bootstrap_peers = discover_bootstrap_peers(&network).await;
        emit_log(&app, "info", "p2p", &format!("Discovered {} bootnodes", bootstrap_peers.len()));
        
        // Determine connection mode string for pyrax-node
        let conn_mode_str = match connection_mode {
            crate::state::ConnectionMode::FullNode => "full",
            crate::state::ConnectionMode::RelayOnly => "relay",
            crate::state::ConnectionMode::Auto => "auto",
        };
        
        emit_log(&app, "info", "p2p", &format!("Connection mode: {} | P2P port: {} | WebSocket: {} | Auto-fallback: {}", 
            conn_mode_str, p2p_port, enable_websocket, auto_port_fallback));
        
        // Build command with all TriStream services enabled
        let mut cmd = Command::new(&binary_path);
        cmd.arg("--network").arg(network_arg)
           .arg("--rpc")
           .arg("--rpc-addr").arg(&rpc_addr)
           .arg("--p2p")
           .arg("--p2p-addr").arg(&p2p_addr)
           .arg("--stratum")
           .arg("--stratum-addr").arg(&stratum_addr)
           .arg("--staking")
           .arg("--staking-addr").arg(&staking_addr)
           .arg("--datadir").arg(&data_dir)
           .arg("--verbosity").arg(log_verbosity.to_string())
           // MASS ADOPTION: Pass connection mode to node
           .arg("--connection-mode").arg(conn_mode_str);
        
        // MASS ADOPTION: Enable WebSocket transport if configured
        if enable_websocket {
            cmd.arg("--enable-websocket");
        }
        
        // MASS ADOPTION: Enable automatic port fallback
        if auto_port_fallback {
            cmd.arg("--auto-port-fallback");
        }
        
        // Add ALL bootstrap peers for relay redundancy (--peer can be specified multiple times)
        for peer in &bootstrap_peers {
            cmd.arg("--peer").arg(peer);
        }
        
        // Ensure data directory exists
        let data_path = std::path::PathBuf::from(&data_dir);
        if let Err(e) = fs::create_dir_all(&data_path) {
            emit_log(&app, "error", "node", &format!("Failed to create data directory {:?}: {}", data_path, e));
        }
        emit_log(&app, "info", "node", &format!("Data directory: {:?}", data_path));
        
        // Capture stdout/stderr to stream logs to UI
        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());
        
        // On Windows, create the process without a window and detached
        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            const DETACHED_PROCESS: u32 = 0x00000008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        
        // Log the full command for debugging
        emit_log(&app, "debug", "node", &format!("Command: {:?}", cmd));
        info!("Starting node with command: {:?}", cmd);
        
        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                
                // Spawn background thread to stream stdout logs to UI
                if let Some(stdout) = child.stdout.take() {
                    let app_clone = app.clone();
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                // Parse tracing log format: "2026-01-16T05:12:43.406091Z  INFO message"
                                let (level, category, message) = parse_node_log(&line);
                                // Skip filtered logs (empty strings)
                                if !level.is_empty() && !message.is_empty() {
                                    emit_log(&app_clone, &level, &category, &message);
                                }
                            }
                        }
                    });
                }
                
                // Spawn background thread to stream stderr logs to UI
                if let Some(stderr) = child.stderr.take() {
                    let app_clone = app.clone();
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                let (level, category, message) = parse_node_log(&line);
                                // Skip filtered logs (empty strings)
                                if !level.is_empty() && !message.is_empty() {
                                    emit_log(&app_clone, &level, &category, &message);
                                }
                            }
                        }
                    });
                }
                
                // Give the process a moment to start, then verify it's running
                std::thread::sleep(std::time::Duration::from_millis(500));
                
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process exited immediately - this is a problem!
                        emit_log(&app, "error", "node", &format!("Node process exited immediately with status: {}", status));
                        return Err(format!("Node process exited immediately with status: {}", status));
                    }
                    Ok(None) => {
                        // Process is still running - good!
                        emit_log(&app, "info", "node", &format!("Node process {} is running", pid));
                    }
                    Err(e) => {
                        emit_log(&app, "warn", "node", &format!("Could not check process status: {}", e));
                    }
                }
                
                emit_log(&app, "info", "node", &format!("Node started with PID: {}", pid));
                emit_log(&app, "info", "p2p", &format!("P2P listening on {}", p2p_addr));
                emit_log(&app, "info", "rpc", &format!("Stream A RPC binding to {}", rpc_addr));
                emit_log(&app, "info", "mining", &format!("Stream B Stratum binding to {}", stratum_addr));
                emit_log(&app, "info", "staking", &format!("Stream C Staking RPC binding to {}", staking_addr));
                info!("Node started with PID: {} - syncing blockchain from P2P peers...", pid);
                
                // Update state
                {
                    let mut app_state = state.lock();
                    app_state.node_running = true;
                    app_state.node_process = Some(child);
                    app_state.rpc_port = rpc_port;
                }
                
                // Wait for node RPC to be ready
                emit_log(&app, "info", "rpc", "Waiting for RPC server to be ready...");
                let rpc = RpcClient::localhost(rpc_port);
                let mut attempts = 0;
                let max_attempts = 120; // Wait up to 60 seconds
                
                while attempts < max_attempts {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    
                    // Check if process is still running
                    {
                        let app_state = state.lock();
                        if let Some(ref _process) = app_state.node_process {
                            // Process handle exists - check if it's still alive
                        } else {
                            emit_log(&app, "error", "node", "Node process terminated unexpectedly");
                            break;
                        }
                    }
                    
                    if rpc.is_connected().await {
                        emit_log(&app, "info", "rpc", &format!("RPC server ready on port {}", rpc_port));
                        emit_log(&app, "info", "node", "Node fully operational - all streams active");
                        info!("Node RPC is ready!");
                        break;
                    }
                    
                    if attempts % 10 == 0 && attempts > 0 {
                        emit_log(&app, "debug", "rpc", &format!("Still waiting for RPC... ({}s)", attempts / 2));
                    }
                    attempts += 1;
                }
                
                if attempts >= max_attempts {
                    emit_log(&app, "warn", "rpc", "RPC timeout - falling back to remote RPC");
                }
                
                // Start connection watchdog for self-healing
                let _ = start_connection_watchdog_internal(app.clone(), state.inner().clone()).await;
                
                // Get initial status
                return get_node_status(state).await;
            }
            Err(e) => {
                emit_log(&app, "error", "node", &format!("Failed to spawn pyrax-node: {}", e));
                error!("Failed to spawn pyrax-node: {}", e);
                // Fall through to remote RPC fallback
            }
        }
    } else {
        emit_log(&app, "warn", "node", "pyrax-node binary not found, falling back to remote RPC");
        warn!("pyrax-node binary not found, falling back to remote RPC");
    }
    
    // Fallback: Connect to remote RPC (light client mode - NOT fully decentralized)
    let remote_url = get_remote_rpc_url(&network);
    emit_log(&app, "info", "rpc", &format!("Connecting to remote RPC: {}", remote_url));
    info!("Falling back to remote RPC (light client mode): {}", remote_url);
    let remote_rpc = RpcClient::new(remote_url);
    
    if remote_rpc.is_connected().await {
        emit_log(&app, "warn", "node", "Connected in LIGHT CLIENT mode (not fully decentralized)");
        warn!("Connected to remote RPC - running in LIGHT CLIENT mode (not fully decentralized)");
        
        {
            let mut app_state = state.lock();
            app_state.node_running = true;
            app_state.node_process = None;
            app_state.rpc_port = rpc_port;
        }
        
        match remote_rpc.get_chain_info().await {
            Ok(info) => {
                return Ok(NodeStatus {
                    running: true,
                    connected: true,
                    syncing: info.syncing,
                    sync_progress: if info.syncing { 50.0 } else { 100.0 },
                    peer_count: 1,
                    block_height: info.best_block_height,
                    block_hash: info.best_block_hash,
                    network: format!("{} (light)", info.network),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    inbound_peers: None,
                    outbound_peers: None,
                    target_peers: None,
                    max_peers: None,
                    dial_attempts: None,
                    dial_successes: None,
                    dial_failures: None,
                    average_rtt_ms: None,
                    network_state: None,
                    nat_status: None,
                    mesh_peers: None,
                    gossip_peers: None,
                });
            }
            Err(e) => {
                warn!("Remote RPC connected but failed to get chain info: {}", e);
                return Ok(NodeStatus {
                    running: true,
                    connected: true,
                    syncing: false,
                    sync_progress: 100.0,
                    peer_count: 1,
                    block_height: 0,
                    block_hash: String::new(),
                    network: format!("{} (light)", network.to_string()),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    inbound_peers: None,
                    outbound_peers: None,
                    target_peers: None,
                    max_peers: None,
                    dial_attempts: None,
                    dial_successes: None,
                    dial_failures: None,
                    average_rtt_ms: None,
                    network_state: None,
                    nat_status: None,
                    mesh_peers: None,
                    gossip_peers: None,
                });
            }
        }
    }
    
    Err(format!(
        "Cannot start node: pyrax-node binary not found and remote RPC {} is unavailable. \
         Please ensure pyrax-node.exe is in the application resources folder.",
        remote_url
    ))
}

#[tauri::command]
pub async fn stop_node(
    app: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    emit_log(&app, "info", "node", "Stopping node...");
    info!("Stopping node...");
    
    // Extract child process WITHOUT holding the mutex during wait
    let child_opt = {
        let mut app_state = state.lock();
        app_state.node_process.take()
    };
    
    // Kill the node process if we have one (mutex NOT held here)
    if let Some(mut child) = child_opt {
        let pid = child.id();
        emit_log(&app, "info", "node", &format!("Stopping local node process (PID: {})", pid));
        info!("Stopping local node process (PID: {})", pid);
        
        #[cfg(target_os = "windows")]
        {
            // On Windows, use taskkill for forceful shutdown of process tree
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .output();
            
            // Also kill any orphaned pyrax-node processes
            let _ = Command::new("taskkill")
                .args(["/IM", "pyrax-node.exe", "/F"])
                .output();
        }
        
        #[cfg(not(target_os = "windows"))]
        {
            // On Unix, send SIGTERM then SIGKILL
            let _ = child.kill();
        }
        
        // Wait for process to exit (with timeout)
        let wait_result = std::thread::spawn(move || {
            child.wait()
        });
        
        // Wait up to 5 seconds for graceful exit
        match wait_result.join() {
            Ok(_) => {
                emit_log(&app, "info", "node", "Node process stopped successfully");
                info!("Node process stopped");
            }
            Err(_) => {
                emit_log(&app, "warn", "node", "Process wait timed out, force killed");
                warn!("Process wait timed out");
            }
        }
    } else {
        // No process handle - check if node_running flag is stale
        let was_running = {
            let app_state = state.lock();
            app_state.node_running
        };
        
        if was_running {
            emit_log(&app, "info", "rpc", "Disconnecting (no local process to stop)");
            info!("Disconnecting (no local process to stop)");
            
            // On Windows, also try to kill any orphaned pyrax-node processes
            #[cfg(target_os = "windows")]
            {
                let _ = Command::new("taskkill")
                    .args(["/IM", "pyrax-node.exe", "/F"])
                    .output();
            }
        }
    }
    
    // Reset state AFTER process is stopped
    {
        let mut app_state = state.lock();
        app_state.node_running = false;
        app_state.node_process = None;
    }
    
    emit_log(&app, "info", "node", "Node stopped");
    Ok(())
}

#[tauri::command]
pub async fn get_node_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<NodeStatus, String> {
    let (running, network, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.network.clone(), app_state.rpc_port)
    };
    
    if !running {
        return Ok(NodeStatus {
            running: false,
            connected: false,
            syncing: false,
            sync_progress: 0.0,
            peer_count: 0,
            block_height: 0,
            block_hash: String::new(),
            network: network.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            inbound_peers: None,
            outbound_peers: None,
            target_peers: None,
            max_peers: None,
            dial_attempts: None,
            dial_successes: None,
            dial_failures: None,
            average_rtt_ms: None,
            network_state: None,
            nat_status: None,
            mesh_peers: None,
            gossip_peers: None,
        });
    }
    
    // P2P STATS FIX: Always prefer LOCAL node for accurate P2P statistics
    // Remote bootnode's P2P state is irrelevant to the user's local connections
    let remote_url = get_remote_rpc_url(&network);
    info!("get_node_status: Checking RPC connections (local port={}, remote={})", rpc_port, remote_url);
    let remote_rpc = RpcClient::new(remote_url);
    let local_rpc = RpcClient::localhost(rpc_port);
    
    // Check which RPC is connected - LOCAL FIRST for accurate P2P stats
    let local_connected = local_rpc.is_connected().await;
    let remote_connected = remote_rpc.is_connected().await;
    info!("get_node_status: local_connected={}, remote_connected={}", local_connected, remote_connected);
    
    let (rpc, is_remote) = if local_connected {
        info!("get_node_status: Using LOCAL RPC");
        (local_rpc, false)  // Local node preferred - has our actual P2P state
    } else if remote_connected {
        info!("get_node_status: Using REMOTE RPC (bootnode)");
        (remote_rpc, true)  // Remote only as fallback when no local node
    } else {
        return Ok(NodeStatus {
            running: true,
            connected: false,
            syncing: false,
            sync_progress: 0.0,
            peer_count: 0,
            block_height: 0,
            block_hash: String::new(),
            network: network.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            inbound_peers: None,
            outbound_peers: None,
            target_peers: None,
            max_peers: None,
            dial_attempts: None,
            dial_successes: None,
            dial_failures: None,
            average_rtt_ms: None,
            network_state: None,
            nat_status: None,
            mesh_peers: None,
            gossip_peers: None,
        });
    };
    
    // Get chain info from connected node
    match rpc.get_chain_info().await {
        Ok(info) => {
            let syncing = info.syncing;
            let sync_progress = if syncing { 50.0 } else { 100.0 };
            
            // P2P STATS FIX: Fetch P2P stats from both local and remote nodes
            // Remote stats show bootnode's view of the network, useful for users without local node
            let (peer_count, p2p_stats, network_state_override) = match rpc.get_network_info().await {
                Ok(net_info) => {
                    info!("get_node_status: get_network_info returned peer_count={}, mesh={}, gossip={}, in={}, out={}",
                        net_info.peer_count, net_info.mesh_peers, net_info.gossip_peers, 
                        net_info.inbound_peers, net_info.outbound_peers);
                    // PEER COUNT FIX: Use mesh_peers or gossip_peers as fallback if peer_count is 0
                    let effective_count = if net_info.peer_count > 0 {
                        net_info.peer_count as u32
                    } else if net_info.mesh_peers > 0 {
                        net_info.mesh_peers as u32
                    } else if net_info.gossip_peers > 0 {
                        net_info.gossip_peers as u32
                    } else if (net_info.inbound_peers + net_info.outbound_peers) > 0 {
                        (net_info.inbound_peers + net_info.outbound_peers) as u32
                    } else {
                        if is_remote { 1 } else { 0 }
                    };
                    info!("get_node_status: effective_peer_count={}", effective_count);
                    // When remote, indicate it's bootnode stats
                    let state_override = if is_remote { 
                        Some("Connected (via Bootnode)".to_string()) 
                    } else { 
                        None 
                    };
                    (effective_count, Some(net_info), state_override)
                },
                Err(_) => (if is_remote { 1 } else { 0 }, None, None)
            };
            
            Ok(NodeStatus {
                running: true,
                connected: true,
                syncing,
                sync_progress,
                peer_count,
                block_height: info.best_block_height,
                block_hash: info.best_block_hash,
                network: info.network,
                version: env!("CARGO_PKG_VERSION").to_string(),
                // Extended P2P stats from network info
                inbound_peers: p2p_stats.as_ref().map(|s| s.inbound_peers as u32),
                outbound_peers: p2p_stats.as_ref().map(|s| s.outbound_peers as u32),
                target_peers: p2p_stats.as_ref().map(|s| s.target_peers as u32),
                max_peers: p2p_stats.as_ref().map(|s| s.max_peers as u32),
                dial_attempts: p2p_stats.as_ref().map(|s| s.dial_attempts),
                dial_successes: p2p_stats.as_ref().map(|s| s.dial_successes),
                dial_failures: p2p_stats.as_ref().map(|s| s.dial_failures),
                average_rtt_ms: p2p_stats.as_ref().and_then(|s| s.average_rtt_ms),
                network_state: network_state_override.or_else(|| p2p_stats.as_ref().map(|s| s.network_state.clone())),
                nat_status: p2p_stats.as_ref().map(|s| s.nat_status.clone()),
                mesh_peers: p2p_stats.as_ref().map(|s| s.mesh_peers as u32),
                gossip_peers: p2p_stats.as_ref().map(|s| s.gossip_peers as u32),
            })
        }
        Err(e) => {
            warn!("Failed to get chain info: {}", e);
            Ok(NodeStatus {
                running: true,
                connected: true,
                syncing: false,
                sync_progress: 0.0,
                peer_count: 0,
                block_height: 0,
                block_hash: String::new(),
                network: network.to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                inbound_peers: None,
                outbound_peers: None,
                target_peers: None,
                max_peers: None,
                dial_attempts: None,
                dial_successes: None,
                dial_failures: None,
                average_rtt_ms: None,
                network_state: None,
                nat_status: None,
                mesh_peers: None,
                gossip_peers: None,
            })
        }
    }
}

#[tauri::command]
pub async fn get_chain_info(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ChainInfo, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let rpc = RpcClient::localhost(rpc_port);
    
    match rpc.get_chain_info().await {
        Ok(info) => Ok(ChainInfo {
            chain_id: info.chain_id as u64,
            best_block_hash: info.best_block_hash,
            best_block_height: info.best_block_height,
            genesis_hash: info.genesis_hash,
            difficulty: info.difficulty.to_string(),
            total_difficulty: info.difficulty.to_string(),
            peer_count: 0, // Peers not yet exposed via RPC
        }),
        Err(e) => Err(format!("Failed to get chain info: {}", e)),
    }
}

#[tauri::command]
pub async fn get_peers(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<PeerInfo>, String> {
    info!("get_peers called");
    
    let (running, rpc_port) = {
        let app_state = state.lock();
        info!("get_peers: Node running={}, RPC port={}", app_state.node_running, app_state.rpc_port);
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        warn!("get_peers: Node is not running");
        return Err("Node is not running".to_string());
    }
    
    let rpc = RpcClient::localhost(rpc_port);
    
    match rpc.get_peers().await {
        Ok(peers) => {
            info!("get_peers: Got {} peers from RPC", peers.len());
            Ok(peers.into_iter().map(|p| PeerInfo {
                id: p.peer_id,
                address: p.address,
                ip: p.ip,
                port: p.port,
                protocol: p.protocol,
                direction: p.direction,
                connected_secs: p.connected_secs,
                version: p.version,
                block_height: p.block_height,
            }).collect())
        },
        Err(e) => {
            error!("get_peers failed: {}", e);
            Err(format!("Failed to get peers: {}", e))
        }
    }
}

/// Get remote bootnode server status and logs
#[tauri::command]
pub async fn get_remote_server_logs(
    app: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<RemoteServerStatus, String> {
    let network = {
        let app_state = state.lock();
        app_state.network.clone()
    };
    
    let status_url = match network {
        crate::state::Network::Mainnet => "https://rpc.pyrax.org/status",
        crate::state::Network::Testnet => "https://rpc.pyrax-testnet.org/status",
        crate::state::Network::Devnet => "http://209.38.137.105:28545", // Use RPC endpoint
    };
    
    emit_log(&app, "info", "rpc", &format!("Fetching remote server status from {}", status_url));
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    
    match client.get(status_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<RemoteServerStatus>().await {
                    Ok(status) => {
                        // Emit each log entry to the UI
                        for log in &status.logs {
                            emit_log(&app, &log.level, &log.category, &format!("[REMOTE] {}", log.message));
                        }
                        Ok(status)
                    }
                    Err(e) => {
                        // Server responded but not with expected format - try basic connectivity
                        emit_log(&app, "warn", "rpc", &format!("Server responded but status format unknown: {}", e));
                        Ok(RemoteServerStatus {
                            online: true,
                            streams: StreamStatus {
                                stream_a_rpc: true,
                                stream_b_stratum: false,
                                stream_c_staking: false,
                            },
                            peer_count: 0,
                            block_height: 0,
                            logs: vec![],
                        })
                    }
                }
            } else {
                emit_log(&app, "error", "rpc", &format!("Remote server returned status: {}", response.status()));
                Err(format!("Remote server returned status: {}", response.status()))
            }
        }
        Err(e) => {
            emit_log(&app, "error", "rpc", &format!("Failed to connect to remote server: {}", e));
            Err(format!("Failed to connect to remote server: {}", e))
        }
    }
}

/// Start streaming logs from remote bootnode
#[tauri::command]
pub async fn start_remote_log_stream(
    app: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let network = {
        let app_state = state.lock();
        app_state.network.clone()
    };
    
    let rpc_url = get_remote_rpc_url(&network);
    
    emit_log(&app, "info", "node", &format!("Connecting to remote bootnode: {}", rpc_url));
    
    // Try to get chain info from remote RPC to verify connection
    let rpc = RpcClient::new(rpc_url);
    
    if rpc.is_connected().await {
        emit_log(&app, "info", "rpc", "✓ Stream A (RPC) - Connected to remote bootnode");
        
        match rpc.get_chain_info().await {
            Ok(info) => {
                emit_log(&app, "info", "block", &format!("Chain: {} | Height: {} | Hash: {}", 
                    info.network, info.best_block_height, &info.best_block_hash[..16]));
                emit_log(&app, "info", "node", &format!("Genesis: {}", &info.genesis_hash[..16]));
                emit_log(&app, "info", "node", &format!("Difficulty: {}", info.difficulty));
                
                if info.syncing {
                    emit_log(&app, "info", "node", "Node is syncing...");
                } else {
                    emit_log(&app, "info", "node", "Node is fully synced");
                }
            }
            Err(e) => {
                emit_log(&app, "warn", "rpc", &format!("Connected but failed to get chain info: {}", e));
            }
        }
        
        // Check stratum port (Stream B)
        let stratum_port = match network {
            crate::state::Network::Mainnet => 3333,
            crate::state::Network::Testnet => 13333,
            crate::state::Network::Devnet => 3333,
        };
        emit_log(&app, "info", "mining", &format!("Stream B (Stratum) - Port {} configured", stratum_port));
        
        // Check staking port (Stream C)
        let staking_port = match network {
            crate::state::Network::Mainnet => 8547,
            crate::state::Network::Testnet => 18547,
            crate::state::Network::Devnet => 28547,
        };
        emit_log(&app, "info", "staking", &format!("Stream C (Staking) - Port {} configured", staking_port));
        
        emit_log(&app, "info", "node", "Remote bootnode connection established");
        Ok(())
    } else {
        emit_log(&app, "error", "rpc", &format!("Failed to connect to remote bootnode: {}", rpc_url));
        Err(format!("Failed to connect to remote bootnode: {}", rpc_url))
    }
}

/// Internal function to start the connection watchdog (called from start_node)
async fn start_connection_watchdog_internal(
    app: AppHandle,
    state: Arc<Mutex<AppState>>,
) -> Result<(), String> {
    let (network, rpc_port) = {
        let app_state = state.lock();
        (app_state.network.clone(), app_state.rpc_port)
    };
    
    let remote_url = get_remote_rpc_url(&network);
    let state_clone = state.clone();
    let app_clone = app.clone();
    
    emit_log(&app, "info", "node", "Starting connection watchdog for self-healing network recovery");
    
    start_watchdog_task(app_clone, state_clone, remote_url, rpc_port).await;
    
    Ok(())
}

/// Start the watchdog background task
async fn start_watchdog_task(
    app: AppHandle,
    state: Arc<Mutex<AppState>>,
    remote_url: &'static str,
    rpc_port: u16,
) {
    let state_clone = state;
    let app_clone = app;
    
    // Spawn background watchdog task
    tokio::spawn(async move {
        let mut consecutive_failures = 0;
        let max_failures = 3; // Restart after 3 consecutive failures (30 seconds)
        let check_interval = tokio::time::Duration::from_secs(10);
        
        loop {
            tokio::time::sleep(check_interval).await;
            
            // Check if node is supposed to be running
            let (node_running, has_process) = {
                let app_state = state_clone.lock();
                (app_state.node_running, app_state.node_process.is_some())
            };
            
            if !node_running {
                // Node is stopped, exit watchdog
                emit_log(&app_clone, "debug", "node", "Watchdog: Node stopped, exiting watchdog");
                break;
            }
            
            // Check local RPC connectivity
            let local_rpc = RpcClient::localhost(rpc_port);
            let local_connected = local_rpc.is_connected().await;
            
            // Check remote bootnode connectivity
            let remote_rpc = RpcClient::new(remote_url);
            let remote_connected = remote_rpc.is_connected().await;
            
            if !local_connected && has_process {
                consecutive_failures += 1;
                emit_log(&app_clone, "warn", "node", &format!(
                    "Watchdog: Local node RPC not responding ({}/{})", 
                    consecutive_failures, max_failures
                ));
                
                if consecutive_failures >= max_failures {
                    emit_log(&app_clone, "error", "node", "Watchdog: Node unresponsive - initiating auto-restart");
                    
                    // Kill the unresponsive node
                    {
                        let mut app_state = state_clone.lock();
                        if let Some(mut child) = app_state.node_process.take() {
                            emit_log(&app_clone, "info", "node", &format!("Watchdog: Killing unresponsive node (PID: {})", child.id()));
                            
                            #[cfg(target_os = "windows")]
                            {
                                let _ = std::process::Command::new("taskkill")
                                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                                    .output();
                            }
                            
                            #[cfg(not(target_os = "windows"))]
                            {
                                let _ = child.kill();
                            }
                            
                            let _ = child.wait();
                        }
                        app_state.node_running = false;
                    }
                    
                    // Emit disconnect event to UI
                    let _ = app_clone.emit_all("node-disconnected", serde_json::json!({
                        "reason": "Node unresponsive",
                        "will_restart": true
                    }));
                    
                    // Wait for network to stabilize
                    emit_log(&app_clone, "info", "node", "Watchdog: Waiting 5 seconds before restart attempt...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    
                    // Check if bootnode is reachable before restarting
                    if remote_rpc.is_connected().await {
                        emit_log(&app_clone, "info", "node", "Watchdog: Bootnode reachable - triggering auto-restart");
                        
                        // Emit restart event to UI (UI should call start_node)
                        let _ = app_clone.emit_all("node-restart-requested", serde_json::json!({
                            "reason": "Auto-recovery after disconnect"
                        }));
                    } else {
                        emit_log(&app_clone, "warn", "node", "Watchdog: Bootnode not reachable - waiting for network...");
                        
                        // Keep checking bootnode until it's available
                        let mut bootnode_wait_count = 0;
                        while bootnode_wait_count < 12 { // Wait up to 2 minutes
                            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                            bootnode_wait_count += 1;
                            
                            if remote_rpc.is_connected().await {
                                emit_log(&app_clone, "info", "node", "Watchdog: Bootnode now reachable - triggering auto-restart");
                                let _ = app_clone.emit_all("node-restart-requested", serde_json::json!({
                                    "reason": "Auto-recovery after network restoration"
                                }));
                                break;
                            }
                            
                            emit_log(&app_clone, "debug", "node", &format!(
                                "Watchdog: Still waiting for bootnode ({}/12)...", 
                                bootnode_wait_count
                            ));
                        }
                        
                        if bootnode_wait_count >= 12 {
                            emit_log(&app_clone, "error", "node", "Watchdog: Bootnode unreachable for 2 minutes - manual intervention may be required");
                            let _ = app_clone.emit_all("node-network-error", serde_json::json!({
                                "reason": "Bootnode unreachable",
                                "duration_seconds": 120
                            }));
                        }
                    }
                    
                    // Exit watchdog - a new one will start when node restarts
                    break;
                }
            } else if local_connected {
                // Reset failure counter on successful connection
                if consecutive_failures > 0 {
                    emit_log(&app_clone, "info", "node", "Watchdog: Node connection restored");
                    consecutive_failures = 0;
                }
            }
            
            // Also check bootnode connectivity periodically
            if !remote_connected && consecutive_failures == 0 {
                emit_log(&app_clone, "warn", "p2p", "Watchdog: Bootnode not reachable - monitoring...");
            }
        }
    });
}

/// Connection watchdog - monitors bootnode connectivity and auto-restarts node on disconnect
/// This runs as a background task and emits events to the UI
#[tauri::command]
pub async fn start_connection_watchdog(
    app: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    start_connection_watchdog_internal(app, state.inner().clone()).await
}

/// Stop connection watchdog (called when node is intentionally stopped)
#[tauri::command]
pub async fn stop_connection_watchdog(
    app: AppHandle,
) -> Result<(), String> {
    emit_log(&app, "info", "node", "Connection watchdog stopped");
    // The watchdog will exit on its own when it detects node_running = false
    Ok(())
}

/// Clear local blockchain data for a specific network
/// This removes the chain database to allow fresh sync
#[tauri::command]
pub async fn clear_local_data(
    app: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
    network: String,
) -> Result<String, String> {
    // First, ensure the node is stopped
    {
        let state_guard = state.lock();
        if state_guard.node_running {
            return Err("Please stop the node before clearing data".to_string());
        }
    }
    
    // Get the data directory
    let app_data_dir = app.path_resolver()
        .app_data_dir()
        .ok_or("Failed to get app data directory")?;
    
    let data_path = app_data_dir.join("data").join(&network);
    
    emit_log(&app, "info", "system", &format!("Clearing local data for {} network...", network));
    
    if data_path.exists() {
        // Remove the entire network data directory
        match fs::remove_dir_all(&data_path) {
            Ok(_) => {
                emit_log(&app, "info", "system", &format!("Successfully cleared {} data at {:?}", network, data_path));
                Ok(format!("Cleared local data for {} network. The node will sync fresh on next start.", network))
            }
            Err(e) => {
                let error_msg = format!("Failed to clear data: {}", e);
                emit_log(&app, "error", "system", &error_msg);
                Err(error_msg)
            }
        }
    } else {
        emit_log(&app, "info", "system", &format!("No data found for {} network at {:?}", network, data_path));
        Ok(format!("No local data found for {} network.", network))
    }
}

/// Get the size of local data for a specific network
#[tauri::command]
pub async fn get_local_data_size(
    app: AppHandle,
    network: String,
) -> Result<String, String> {
    let app_data_dir = app.path_resolver()
        .app_data_dir()
        .ok_or("Failed to get app data directory")?;
    
    let data_path = app_data_dir.join("data").join(&network);
    
    if !data_path.exists() {
        return Ok("No data".to_string());
    }
    
    // Calculate directory size
    fn dir_size(path: &std::path::Path) -> u64 {
        let mut size = 0;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    size += dir_size(&path);
                } else if let Ok(metadata) = entry.metadata() {
                    size += metadata.len();
                }
            }
        }
        size
    }
    
    let size = dir_size(&data_path);
    
    // Format size
    let formatted = if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    };
    
    Ok(formatted)
}

/// Network mesh data for visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkMeshData {
    pub mesh_connections: Vec<MeshConnectionInfo>,
    pub local_peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshConnectionInfo {
    pub peer_a: String,
    pub peer_b: String,
    pub topic: String,
    pub connection_type: String,
}

#[tauri::command]
pub async fn get_network_mesh(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<NetworkMeshData, String> {
    let (running, rpc_port) = {
        let app_state = state.lock();
        (app_state.node_running, app_state.rpc_port)
    };
    
    if !running {
        return Err("Node is not running".to_string());
    }
    
    let rpc = RpcClient::localhost(rpc_port);
    
    match rpc.get_network_info().await {
        Ok(info) => Ok(NetworkMeshData {
            mesh_connections: info.mesh_connections.into_iter().map(|c| MeshConnectionInfo {
                peer_a: c.peer_a,
                peer_b: c.peer_b,
                topic: c.topic,
                connection_type: c.connection_type,
            }).collect(),
            local_peer_id: info.local_peer_id,
        }),
        Err(e) => {
            warn!("Failed to get network mesh: {}", e);
            Err(format!("Failed to get network mesh: {}", e))
        }
    }
}

/// Measure real-time latency to bootnodes
/// Returns latency in milliseconds for each bootnode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootnodeLatency {
    pub ip: String,
    pub latency_ms: Option<u64>,
    pub online: bool,
}

#[tauri::command]
pub async fn measure_bootnode_latency(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<BootnodeLatency>, String> {
    let network = {
        let app_state = state.lock();
        app_state.network.clone()
    };
    
    let bootnode_configs = get_bootnode_configs(&network);
    let mut results = Vec::new();
    
    for config in bootnode_configs {
        let start = std::time::Instant::now();
        let rpc_url = format!("http://{}:{}", config.ip, config.rpc_port);
        let rpc = RpcClient::new(&rpc_url);
        
        match rpc.health_check().await {
            Ok(_) => {
                let latency = start.elapsed().as_millis() as u64;
                results.push(BootnodeLatency {
                    ip: config.ip.to_string(),
                    latency_ms: Some(latency),
                    online: true,
                });
            }
            Err(_) => {
                results.push(BootnodeLatency {
                    ip: config.ip.to_string(),
                    latency_ms: None,
                    online: false,
                });
            }
        }
    }
    
    Ok(results)
}
