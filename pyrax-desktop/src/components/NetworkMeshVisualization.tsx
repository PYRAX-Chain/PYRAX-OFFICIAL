import { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { Network, RefreshCw } from 'lucide-react';
import { useNodeStore, MeshConnection, PeerInfo } from '../stores/nodeStore';

interface NetworkInfo {
  meshConnections: MeshConnection[];
  localPeerId: string;
}

interface NodePosition {
  id: string;
  x: number;
  y: number;
  label: string;
  isLocal: boolean;
  isBootnode: boolean;
}

// Bootnode IPs for devnet
const BOOTNODE_IPS = ['209.38.137.105', '137.184.118.228'];

// Helper to check if an IP is a bootnode
const isBootnodeIP = (ip: string): boolean => BOOTNODE_IPS.includes(ip);

// Helper to extract IP from multiaddr format
const extractIPFromAddress = (address: string): string => {
  const match = address.match(/\/ip4\/([^/]+)/);
  if (match) return match[1];
  const colonMatch = address.match(/^([^:]+):/);
  if (colonMatch) return colonMatch[1];
  return address;
};

export default function NetworkMeshVisualization() {
  const { status, peers } = useNodeStore();
  const [meshData, setMeshData] = useState<NetworkInfo | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const fetchMeshData = async () => {
    if (!status?.connected) return;
    setIsLoading(true);
    try {
      const data = await invoke<NetworkInfo>('get_network_mesh');
      setMeshData(data);
    } catch (e) {
      console.error('Failed to fetch mesh data:', e);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (status?.connected) {
      fetchMeshData();
      const interval = setInterval(fetchMeshData, 15000);
      return () => clearInterval(interval);
    }
  }, [status?.connected]);

  // Draw the mesh visualization
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !meshData || peers.length === 0) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    const centerX = width / 2;
    const centerY = height / 2;
    const radius = Math.min(width, height) * 0.35;

    // Clear canvas
    ctx.clearRect(0, 0, width, height);

    // Calculate node positions in a circle
    const nodes: NodePosition[] = [];
    
    // Add local node at center
    nodes.push({
      id: meshData.localPeerId,
      x: centerX,
      y: centerY,
      label: 'You',
      isLocal: true,
      isBootnode: false,
    });

    // Add peer nodes around the circle
    peers.forEach((peer, index) => {
      const angle = (2 * Math.PI * index) / peers.length - Math.PI / 2;
      nodes.push({
        id: peer.id,
        x: centerX + radius * Math.cos(angle),
        y: centerY + radius * Math.sin(angle),
        label: peer.id.slice(-6),
        isLocal: false,
        isBootnode: isBootnodeIP(peer.ip) || isBootnodeIP(extractIPFromAddress((peer as any).address || '')),
      });
    });

    // Draw connections (mesh lines)
    ctx.strokeStyle = 'rgba(139, 92, 246, 0.3)';
    ctx.lineWidth = 1;
    
    meshData.meshConnections.forEach((conn) => {
      const nodeA = nodes.find(n => n.id.includes(conn.peerA.slice(0, 12)) || conn.peerA.includes(n.id.slice(0, 12)));
      const nodeB = nodes.find(n => n.id.includes(conn.peerB.slice(0, 12)) || conn.peerB.includes(n.id.slice(0, 12)));
      
      if (nodeA && nodeB) {
        ctx.beginPath();
        ctx.moveTo(nodeA.x, nodeA.y);
        ctx.lineTo(nodeB.x, nodeB.y);
        
        // Color based on connection type
        if (conn.connectionType === 'mesh') {
          ctx.strokeStyle = 'rgba(34, 197, 94, 0.5)'; // Green for mesh
        } else if (conn.connectionType === 'gossip') {
          ctx.strokeStyle = 'rgba(59, 130, 246, 0.5)'; // Blue for gossip
        } else {
          ctx.strokeStyle = 'rgba(139, 92, 246, 0.3)'; // Purple default
        }
        ctx.stroke();
      }
    });

    // Draw direct connections from local to peers
    ctx.strokeStyle = 'rgba(139, 92, 246, 0.6)';
    ctx.lineWidth = 2;
    
    const localNode = nodes[0];
    nodes.slice(1).forEach((peerNode) => {
      ctx.beginPath();
      ctx.moveTo(localNode.x, localNode.y);
      ctx.lineTo(peerNode.x, peerNode.y);
      ctx.stroke();
    });

    // Draw nodes
    nodes.forEach((node) => {
      ctx.beginPath();
      const nodeRadius = node.isLocal ? 20 : node.isBootnode ? 14 : 10;
      ctx.arc(node.x, node.y, nodeRadius, 0, 2 * Math.PI);
      
      if (node.isLocal) {
        ctx.fillStyle = '#8b5cf6'; // Purple for local
      } else if (node.isBootnode) {
        ctx.fillStyle = '#f59e0b'; // Amber for bootnodes
      } else {
        ctx.fillStyle = '#22c55e'; // Green for peers
      }
      ctx.fill();
      
      // Draw border
      ctx.strokeStyle = '#1c1917';
      ctx.lineWidth = 2;
      ctx.stroke();

      // Draw label
      ctx.fillStyle = '#a8a29e';
      ctx.font = '10px sans-serif';
      ctx.textAlign = 'center';
      ctx.fillText(node.label, node.x, node.y + nodeRadius + 14);
    });

  }, [meshData, peers]);

  if (!status?.connected) {
    return (
      <div className="bg-dark-800 rounded-xl p-6">
        <div className="text-center py-8 text-stone-400">
          <Network size={48} className="mx-auto mb-4 opacity-50" />
          <p>Connect to a node to view mesh topology</p>
        </div>
      </div>
    );
  }

  return (
    <div className="bg-dark-800 rounded-xl p-6">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-semibold flex items-center gap-2">
          <Network size={20} className="text-pyrax-400" />
          P2P Mesh Topology
        </h2>
        <button
          onClick={fetchMeshData}
          disabled={isLoading}
          className="text-sm text-pyrax-400 hover:text-pyrax-300 flex items-center gap-1"
        >
          <RefreshCw size={14} className={isLoading ? 'animate-spin' : ''} />
          Refresh
        </button>
      </div>

      <div className="relative">
        <canvas
          ref={canvasRef}
          width={400}
          height={300}
          className="w-full max-w-md mx-auto bg-dark-900 rounded-lg"
        />
        
        {/* Legend */}
        <div className="flex justify-center gap-6 mt-4 text-xs text-stone-400">
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 rounded-full bg-purple-500" />
            <span>You</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 rounded-full bg-amber-500" />
            <span>Bootnode</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 rounded-full bg-green-500" />
            <span>Peer</span>
          </div>
        </div>

        {/* Stats */}
        <div className="grid grid-cols-3 gap-4 mt-4 text-center">
          <div>
            <div className="text-2xl font-bold text-pyrax-400">{peers.length}</div>
            <div className="text-xs text-stone-500">Connected Peers</div>
          </div>
          <div>
            <div className="text-2xl font-bold text-green-400">
              {meshData?.meshConnections?.length || 0}
            </div>
            <div className="text-xs text-stone-500">Mesh Links</div>
          </div>
          <div>
            <div className="text-2xl font-bold text-blue-400">
              {status.meshPeers || 0}
            </div>
            <div className="text-xs text-stone-500">Mesh Peers</div>
          </div>
        </div>
      </div>
    </div>
  );
}
