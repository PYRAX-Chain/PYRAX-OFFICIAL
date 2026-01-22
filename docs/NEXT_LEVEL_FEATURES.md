# PYRAX Next-Level Node Features (v0.3.5+)

This document describes the advanced features implemented in the PYRAX node to provide a "just works" user experience with enterprise-grade reliability.

## Overview

The PYRAX node includes 10 next-level feature modules that work together to provide:
- **Zero-config connectivity** - Users can connect without port forwarding or technical setup
- **Self-healing network** - Automatic recovery from network issues
- **Adaptive performance** - Optimizes based on hardware capabilities
- **Privacy-first networking** - Transaction privacy via Dandelion++
- **World-class observability** - Prometheus metrics and health endpoints

---

## 1. Smart Connectivity (`smart_connectivity.rs`)

### Features
- **ISP Detection**: Identifies ISP type and recommends optimal protocols
- **Captive Portal Detection**: Detects hotel/airport WiFi login pages
- **Protocol Selection**: Auto-selects TCP, QUIC, WebSocket, or Relay based on network
- **Connection Quality Scoring**: Tracks peer quality for optimal selection

### How It Works
When a node starts, it:
1. Detects external IP and ISP characteristics
2. Checks for captive portals blocking connectivity
3. Tests which protocols work on the current network
4. Selects the optimal protocol for peer connections

---

## 2. Self-Healing Network (`self_healing.rs`)

### Features
- **Auto-Reconnect**: Exponential backoff reconnection to failed peers
- **Relay Cascade**: Falls back through multiple relays if one fails
- **Peer Resurrection**: Reconnects to known-good peers after disconnection
- **Partition Detection**: Detects network isolation and triggers recovery

### Recovery States
- `Healthy` - Normal operation with adequate peers
- `Degraded` - Reduced connectivity, attempting recovery
- `Partitioned` - Network split detected, aggressive reconnection
- `Isolated` - No peers, emergency bootstrap

---

## 3. Adaptive Performance (`adaptive_performance.rs`)

### Features
- **Hardware Detection**: Detects CPU cores, RAM, disk space
- **Performance Tiers**: High/Medium/Low/Minimal based on resources
- **Power Mode**: Battery saver mode for laptops
- **Bandwidth Management**: Metered connection support with limits

### Performance Tiers
| Tier | CPU | RAM | Max Peers | Cache |
|------|-----|-----|-----------|-------|
| High | 8+ | 16GB+ | 100 | 512MB |
| Medium | 4+ | 8GB+ | 50 | 256MB |
| Low | 2+ | 4GB+ | 25 | 128MB |
| Minimal | Any | <4GB | 10 | 64MB |

---

## 4. Diagnostics (`diagnostics.rs`)

### Features
- **One-Click Diagnostics**: "Why can't I connect?" with fix suggestions
- **Sync Progress**: Real-time sync progress with ETA
- **Health Metrics**: Network health dashboard data
- **Troubleshooting**: Automatic issue detection with fix steps

### Diagnostic Checks
- Internet connectivity
- DNS resolution
- Bootnode reachability
- Port accessibility
- Peer count

---

## 5. Privacy Features (`privacy.rs`)

### Features
- **Dandelion++**: Transaction propagation privacy
- **Privacy Levels**: Standard/Enhanced/Maximum modes
- **IP Obfuscation**: Relay-based IP hiding

### Dandelion++ Phases
1. **Stem Phase**: TX sent to single random peer
2. **Fluff Phase**: TX broadcast to all peers after random hops

---

## 6. Observability (`observability.rs`)

### Features
- **Prometheus Metrics**: Full metrics export at `/metrics`
- **Health Endpoints**: `/health`, `/ready`, `/live`
- **Structured Logging**: JSON-formatted logs

### Key Metrics
- `pyrax_p2p_peers_connected` - Current peer count
- `pyrax_p2p_messages_received_total` - Messages received
- `pyrax_chain_height` - Current chain height
- `pyrax_sync_progress_percent` - Sync percentage

---

## 7. Incentivized Relay (`incentivized_relay.rs`)

### Features
- **Bandwidth Accounting**: Track bytes relayed per peer
- **Relay Proofs**: Cryptographic proof of relay work
- **Fair Sharing**: Prevent freeloaders from abusing relay

### Relay Rewards
Nodes earn reward points for relaying traffic:
- 1 point per MB relayed
- Points tracked for future reward distribution

---

## 8. Intelligent Peer Selection (`intelligent_peers.rs`)

### Features
- **ML-Inspired Scoring**: Multi-factor peer scoring
- **Predictive Selection**: Predict connection success probability
- **History Tracking**: Learn from past peer interactions

### Scoring Factors
- Connection success rate (25%)
- Latency performance (25%)
- Message reliability (25%)
- Responsiveness (20%)
- Bootnode bonus (5%)

---

## 9. Edge Computing (`edge_computing.rs`)

### Features
- **ARM64 Support**: Optimized for Raspberry Pi, etc.
- **Light Mode**: Headers-only sync for constrained devices
- **Embedded Mode**: Minimal resource usage
- **WASM Support**: Browser-based light nodes (future)

### Node Modes
| Mode | Max Peers | Cache | Sync Type |
|------|-----------|-------|-----------|
| Full | 100 | 512MB | Full blocks |
| Light | 25 | 128MB | Headers only |
| Embedded | 10 | 32MB | Headers only |
| WASM | 5 | 8MB | Headers only |

---

## Configuration

These features are enabled by default. Some can be configured via CLI flags:

```bash
# Power mode
pyrax-node --power-mode battery-saver

# Privacy level
pyrax-node --privacy-level enhanced

# Node mode for constrained devices
pyrax-node --node-mode light

# Disable specific features
pyrax-node --disable-dandelion
pyrax-node --disable-relay
```

---

## Integration with Desktop App

The desktop app uses these features via RPC:

```typescript
// Get diagnostics
const report = await rpc.call('pyrax_getDiagnostics');

// Get sync progress
const progress = await rpc.call('pyrax_getSyncProgress');

// Get health metrics
const metrics = await rpc.call('pyrax_getHealthMetrics');
```

---

## Future Enhancements

- **Machine Learning**: Actual ML models for peer selection
- **Tor Integration**: Onion routing support
- **IPFS Integration**: Distributed block storage
- **Cross-chain Bridges**: Multi-chain connectivity

---

*Documentation for PYRAX v0.3.5+ Next-Level Node Features*
