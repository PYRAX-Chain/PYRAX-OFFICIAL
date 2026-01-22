# Changelog

All notable changes to the PYRAX project will be documented in this file.

## [0.3.5] - Unreleased (Next-Level Node Features)

### Added - Next-Level P2P Features

#### Zero-Config Connectivity
- **Smart Connectivity Module** (`smart_connectivity.rs`)
  - ISP detection and protocol recommendation
  - Captive portal detection for hotel/airport WiFi
  - Automatic protocol selection (TCP/QUIC/WebSocket/Relay)
  - Connection quality scoring per peer

#### Self-Healing Network
- **Self-Healing Module** (`self_healing.rs`)
  - Auto-reconnect with exponential backoff
  - Relay cascade (fallback through multiple relays)
  - Peer resurrection for known-good peers
  - Network partition detection and recovery

#### Adaptive Performance
- **Adaptive Performance Module** (`adaptive_performance.rs`)
  - Hardware detection (CPU cores, RAM, disk)
  - Performance tiers (High/Medium/Low/Minimal)
  - Battery saver mode for laptops
  - Bandwidth management for metered connections

#### Privacy Features
- **Privacy Module** (`privacy.rs`)
  - Dandelion++ transaction propagation
  - Privacy levels (Standard/Enhanced/Maximum)
  - IP obfuscation via relay

#### Intelligent Peer Selection
- **Intelligent Peers Module** (`intelligent_peers.rs`)
  - ML-inspired multi-factor peer scoring
  - Predictive connection success probability
  - Historical peer behavior learning

#### Network Observability
- **Observability Module** (`observability.rs`)
  - Prometheus metrics export
  - Health check endpoints (/health, /ready, /live)
  - Structured logging support

#### Incentivized Relay
- **Incentivized Relay Module** (`incentivized_relay.rs`)
  - Bandwidth accounting per peer
  - Relay proof generation
  - Fair sharing enforcement

#### Edge Computing Support
- **Edge Computing Module** (`edge_computing.rs`)
  - ARM64/Raspberry Pi optimization
  - Light mode (headers-only sync)
  - Embedded mode for IoT devices
  - WASM support preparation

#### UX Diagnostics
- **Diagnostics Module** (`diagnostics.rs`)
  - One-click "Why can't I connect?" diagnostics
  - Sync progress tracking with ETA
  - Network health metrics dashboard
  - Auto-fix suggestions for common issues

### Changed
- P2P module now exports all next-level features
- Added `num_cpus` dependency for hardware detection

### Documentation
- Added `docs/NEXT_LEVEL_FEATURES.md` with full feature documentation

---

## [0.3.4] - 2024-XX-XX

### Fixed
- Added missing `pyrax_getPeers` RPC method for desktop app compatibility
- Added missing `pyrax_getMiningInfo` RPC method for desktop mining panel
- Stabilized P2P configuration parameters to reduce peer churn
- Fixed selective chain wipe to preserve node identity across deploys
- Fixed Docker container conflicts in deployment workflow
- Corrected explorer chain IDs (7225 for devnet)

### Changed
- Increased dial timeout to 15s (was 10s)
- Increased ping interval to 45s (was 15s)
- Increased peer refresh interval to 120s (was 30s)
- Increased peer reevaluation interval to 300s (was 60s)

---

## [0.3.3] - Previous Release

See git history for earlier changes.
