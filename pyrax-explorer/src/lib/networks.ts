// ═══════════════════════════════════════════════════════════════════════════
// PYRAX TriStream DAG Network Architecture
// ═══════════════════════════════════════════════════════════════════════════
//
// LAYER 1: TriStream DAG (GHOSTDAG ordering)
//   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
//   │  STREAM A   │   │  STREAM B   │   │  STREAM C   │
//   │   (ASIC)    │   │   (GPU)     │   │   (PoS)     │
//   │  BLAKE3     │   │  KAWPOW     │   │  ZK-STARK   │
//   │  10s blocks │   │  60s blocks │   │  Finality   │
//   │  50 PYRAX   │   │  100 PYRAX  │   │  10 PYRAX   │
//   └──────┬──────┘   └──────┬──────┘   └──────┬──────┘
//          └────────┬────────┴────────┬────────┘
//                   │    GHOSTDAG     │
//                   └────────┬────────┘
//                     Unified UTXO State
//
// LAYER 2: EVM Sidechain (Account-based, 2s blocks, 1k-5k TPS)
//   - Solidity 0.8.x + Rust/WASM contracts
//   - 2-way bridge to L1
//   - Validated by Stream C stakers
//
// LAYER 3: ZK-Rollups (500k+ TPS)
//   - AI Compute Rollup
//   - DeFi Rollup  
//   - Gaming Rollup
//
// GAS FEE DISTRIBUTION:
//   Stream A Miners: 20%
//   Stream B Miners: 40%
//   Stream C Stakers: 30%
//   Protocol Treasury: 10%
//
// TOKEN: PYRAX | Max Supply: 100,000,000,000 | Decimals: 8
// ═══════════════════════════════════════════════════════════════════════════

export type StreamType = 'A' | 'B' | 'C';

export interface Stream {
  id: StreamType;
  name: string;
  consensus: string;
  algorithm: string;
  blockTime: number; // seconds (0 = event-driven)
  blockReward: number; // PYRAX per block/checkpoint
  gasShare: number; // percentage of gas fees (0-100)
  description: string;
  rpcPort: number;
}

export const STREAMS: Record<StreamType, Stream> = {
  A: {
    id: 'A',
    name: 'Stream A (ASIC)',
    consensus: 'PoW',
    algorithm: 'BLAKE3',
    blockTime: 10,
    blockReward: 50,
    gasShare: 20,
    description: 'High-speed UTXO transactions, ASIC-friendly',
    rpcPort: 8545,
  },
  B: {
    id: 'B',
    name: 'Stream B (GPU)',
    consensus: 'PoW',
    algorithm: 'KAWPOW',
    blockTime: 60,
    blockReward: 100,
    gasShare: 40,
    description: 'GPU mining + AI compute jobs',
    rpcPort: 8546,
  },
  C: {
    id: 'C',
    name: 'Stream C (ZK)',
    consensus: 'PoS + ZK-STARK',
    algorithm: 'ZK-STARK',
    blockTime: 0, // Event-driven finality
    blockReward: 10,
    gasShare: 30,
    description: 'ZK proofs & finality checkpoints, EVM validation',
    rpcPort: 8547,
  },
};

// Protocol Treasury receives 10% of all gas fees
export const TREASURY_GAS_SHARE = 10;

// Token Economics
export const TOKEN = {
  name: 'PYRAX',
  symbol: 'PYRAX',
  decimals: 8,
  maxSupply: '100000000000', // 100 billion (string for precision)
  maxSupplyNum: 100_000_000_000,
} as const;

export interface Network {
  id: string;
  name: string;
  chainId: number;
  symbol: string;
  streams: {
    A: string; // RPC URL for Stream A
    B: string; // RPC URL for Stream B
    C: string; // RPC URL for Stream C
  };
  evmSidechain: string; // EVM Sidechain RPC
  blockExplorer?: string;
}

// Network configurations with correct chain IDs
// Note: Client-side code uses /api/rpc proxy which handles actual RPC routing
// These URLs are for reference/fallback only - the API proxy uses correct endpoints
export const NETWORKS: Network[] = [
  {
    id: 'mainnet',
    name: 'Mainnet',
    chainId: 7227, // PYRAX Mainnet chain ID
    symbol: 'PYRAX',
    streams: {
      A: '/api/rpc', // Uses API proxy for CORS handling
      B: '/api/rpc',
      C: '/api/rpc',
    },
    evmSidechain: '/api/rpc',
    blockExplorer: 'https://explorer.pyrax.org',
  },
  {
    id: 'testnet',
    name: 'Testnet',
    chainId: 7226, // PYRAX Testnet chain ID
    symbol: 'tPYRAX',
    streams: {
      A: '/api/rpc',
      B: '/api/rpc',
      C: '/api/rpc',
    },
    evmSidechain: '/api/rpc',
    blockExplorer: 'https://testnet.explorer.pyrax.org',
  },
  {
    id: 'devnet',
    name: 'Devnet',
    chainId: 7225, // PYRAX Devnet chain ID (CORRECT!)
    symbol: 'dPYRAX',
    streams: {
      A: '/api/rpc',
      B: '/api/rpc',
      C: '/api/rpc',
    },
    evmSidechain: '/api/rpc',
    blockExplorer: 'https://explorer.pyrax-devnet.org',
  },
];

export type ConnectionStatus = 'connected' | 'degraded' | 'disconnected';

export interface StreamStatus {
  stream: StreamType;
  status: ConnectionStatus;
  latency: number | null;
  blockHeight: number | null;
  lastChecked: number | null;
}

export interface NetworkState {
  network: Network;
  streams: Record<StreamType, StreamStatus>;
  evmStatus: ConnectionStatus;
  evmLatency: number | null;
}

export function getNetworkById(id: string): Network | undefined {
  return NETWORKS.find((n) => n.id === id);
}

export function getDefaultNetwork(): Network {
  // Use environment variable to determine network
  const envNetwork = process.env.NEXT_PUBLIC_NETWORK;
  if (envNetwork) {
    const network = NETWORKS.find(n => n.id === envNetwork);
    if (network) return network;
  }
  // Fallback to testnet
  return NETWORKS.find(n => n.id === 'testnet') || NETWORKS[0];
}

export function getOverallStatus(state: NetworkState): ConnectionStatus {
  const statuses = Object.values(state.streams).map(s => s.status);
  if (statuses.every(s => s === 'connected')) return 'connected';
  if (statuses.every(s => s === 'disconnected')) return 'disconnected';
  return 'degraded';
}
