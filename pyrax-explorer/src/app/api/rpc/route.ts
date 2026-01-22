import { NextRequest, NextResponse } from 'next/server';

// RPC endpoints for PYRAX Devnet
// In Docker: Uses host.docker.internal to reach the host pyrax-node (running via systemd)
// Public DNS: rpc.pyrax-devnet.org:28545 (Bootnode 1), rpc2.pyrax-devnet.org:28545 (Bootnode 2)
const DEVNET_RPC = process.env.DEVNET_RPC_URL || 'http://host.docker.internal:28545';
const TESTNET_RPC = process.env.TESTNET_RPC_URL || 'http://host.docker.internal:18545';
const MAINNET_RPC = process.env.MAINNET_RPC_URL || 'http://host.docker.internal:8545';

const RPC_ENDPOINTS: Record<string, string> = {
  mainnet: MAINNET_RPC,
  testnet: TESTNET_RPC,
  devnet: DEVNET_RPC,
};

export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const network = request.headers.get('x-network') || 'devnet';
    // Note: Stream header is accepted but currently all streams use the same node
    // Future: When separate Stream B/C nodes are deployed, re-enable stream-specific routing
    
    // Use the same endpoint for all streams (single node deployment)
    const endpoint = RPC_ENDPOINTS[network] || RPC_ENDPOINTS.devnet;

    const startTime = Date.now();
    
    const response = await fetch(endpoint, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });

    const latency = Date.now() - startTime;

    if (!response.ok) {
      return NextResponse.json(
        { error: `Node returned ${response.status}` },
        { status: response.status }
      );
    }

    const data = await response.json();
    
    // Add latency info to response
    return NextResponse.json({
      ...data,
      _latency: latency,
    });
  } catch (error) {
    console.error('RPC proxy error:', error);
    return NextResponse.json(
      { error: 'Failed to connect to node', details: String(error) },
      { status: 503 }
    );
  }
}
