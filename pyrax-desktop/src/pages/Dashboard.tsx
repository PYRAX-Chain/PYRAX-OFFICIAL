import { useEffect, useState, useRef, useCallback } from 'react';
import { Link } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/tauri';
import { listen } from '@tauri-apps/api/event';
import { checkUpdate, installUpdate } from '@tauri-apps/api/updater';
import { relaunch } from '@tauri-apps/api/process';
import { 
  Activity, Blocks, Users, Zap, Play, Square, RefreshCw, Globe,
  Network, Wifi, Clock, TrendingUp, ArrowUpRight, ArrowDownLeft,
  Shield, Signal, Radio, Gauge, ChevronRight, Server, Download
} from 'lucide-react';
import { useNodeStore, MeshConnection } from '../stores/nodeStore';
import { useWalletStore } from '../stores/walletStore';
import { useMinerStore } from '../stores/minerStore';
import { useLogStore } from '../stores/logStore';
import { formatBalance, formatHashrate } from '../lib/utils';
import LogViewer from '../components/LogViewer';
import NeuraxInsightsPanel from '../components/NeuraxInsightsPanel';

interface TelemetryDataPoint {
  timestamp: number;
  peerCount: number;
  meshPeers: number;
  latency: number;
}

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

const MAX_DATA_POINTS = 30;

// Bootnode IPs for devnet
const BOOTNODE_IPS = ['209.38.137.105', '137.184.118.228'];

// Helper to check if an IP is a bootnode
const isBootnodeIP = (ip: string): boolean => BOOTNODE_IPS.includes(ip);

// Helper to extract IP from multiaddr format (e.g., /ip4/1.2.3.4/tcp/30303)
const extractIPFromAddress = (address: string): string => {
  const match = address.match(/\/ip4\/([^/]+)/);
  if (match) return match[1];
  const colonMatch = address.match(/^([^:]+):/);
  if (colonMatch) return colonMatch[1];
  return address;
};

export default function Dashboard() {
  const { status, chainInfo, peers, fetchPeers, startNode, stopNode, fetchChainInfo, loading: nodeLoading } = useNodeStore();
  const { addresses } = useWalletStore();
  const { status: minerStatus } = useMinerStore();
  const { addLog } = useLogStore();
  const [selectedNetwork, setSelectedNetwork] = useState<'testnet' | 'devnet'>('devnet');
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<{ version: string; body: string } | null>(null);
  
  // Telemetry history for charts
  const [telemetryHistory, setTelemetryHistory] = useState<TelemetryDataPoint[]>([]);
  const lastUpdateRef = useRef<number>(0);
  
  // Mesh visualization state
  const [meshData, setMeshData] = useState<NetworkInfo | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Listen for log events from backend
  useEffect(() => {
    const unlisten = listen<{ level: string; category: string; message: string }>('node-log', (event) => {
      const { level, category, message } = event.payload;
      addLog(
        level as 'info' | 'warn' | 'error' | 'debug',
        category as 'node' | 'block' | 'p2p' | 'rpc' | 'mining' | 'staking' | 'system',
        message
      );
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [addLog]);

  // Listen for watchdog events (auto-reconnect)
  useEffect(() => {
    const unlistenDisconnected = listen<{ reason: string; will_restart: boolean }>('node-disconnected', (event) => {
      addLog('warn', 'node', `Node disconnected: ${event.payload.reason}`);
      if (event.payload.will_restart) {
        addLog('info', 'node', 'Auto-restart pending...');
      }
    });

    const unlistenRestart = listen<{ reason: string }>('node-restart-requested', async (event) => {
      addLog('info', 'node', `Auto-restart triggered: ${event.payload.reason}`);
      try {
        await startNode();
        addLog('info', 'node', 'Node restarted successfully');
      } catch (e) {
        addLog('error', 'node', `Failed to restart node: ${e}`);
      }
    });

    const unlistenNetworkError = listen<{ reason: string; duration_seconds: number }>('node-network-error', (event) => {
      addLog('error', 'node', `Network error: ${event.payload.reason} (${event.payload.duration_seconds}s)`);
    });

    return () => {
      unlistenDisconnected.then((fn) => fn());
      unlistenRestart.then((fn) => fn());
      unlistenNetworkError.then((fn) => fn());
    };
  }, [startNode, addLog]);

  useEffect(() => {
    // Load current network setting
    invoke<{ network: string }>('get_settings').then((settings) => {
      if (settings.network === 'devnet' || settings.network === 'testnet') {
        setSelectedNetwork(settings.network);
      }
    }).catch(console.error);
  }, []);

  useEffect(() => {
    if (status?.running && status?.connected) {
      fetchChainInfo();
      fetchPeers();
    }
  }, [status?.running, status?.connected, fetchChainInfo, fetchPeers]);

  // Collect telemetry data - add first point immediately, then every 10 seconds
  useEffect(() => {
    if (!status?.connected) return;
    
    const addDataPoint = () => {
      const newDataPoint: TelemetryDataPoint = {
        timestamp: Date.now(),
        peerCount: status.peerCount || 0,
        meshPeers: status.meshPeers ?? 0,
        latency: status.averageRttMs ?? 0,
      };
      setTelemetryHistory(prev => {
        const newHistory = [...prev, newDataPoint];
        return newHistory.length > MAX_DATA_POINTS ? newHistory.slice(-MAX_DATA_POINTS) : newHistory;
      });
    };
    
    // Add first data point immediately
    addDataPoint();
    
    // Then collect every 10 seconds
    const interval = setInterval(addDataPoint, 10000);
    return () => clearInterval(interval);
  }, [status?.connected, status?.peerCount, status?.meshPeers, status?.averageRttMs]);

  // Fetch mesh data for visualization
  const fetchMeshData = useCallback(async () => {
    if (!status?.connected) return;
    try {
      const data = await invoke<NetworkInfo>('get_network_mesh');
      setMeshData(data);
    } catch (e) { console.error('Failed to fetch mesh data:', e); }
  }, [status?.connected]);

  useEffect(() => {
    if (status?.connected) {
      fetchMeshData();
      fetchPeers();
      const interval = setInterval(() => { fetchMeshData(); fetchPeers(); }, 15000);
      return () => clearInterval(interval);
    }
  }, [status?.connected, fetchMeshData, fetchPeers]);

  // Draw mesh visualization on canvas
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !meshData || peers.length === 0) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);
    const width = rect.width;
    const height = rect.height;
    const centerX = width / 2;
    const centerY = height / 2;
    const radius = Math.min(width, height) * 0.35;
    ctx.clearRect(0, 0, width, height);

    // Calculate node positions
    const nodes: NodePosition[] = [];
    nodes.push({ id: meshData.localPeerId, x: centerX, y: centerY, label: 'You', isLocal: true, isBootnode: false });
    peers.forEach((peer, index) => {
      const angle = (2 * Math.PI * index) / peers.length - Math.PI / 2;
      nodes.push({
        id: peer.id,
        x: centerX + radius * Math.cos(angle),
        y: centerY + radius * Math.sin(angle),
        label: peer.id.slice(-6),
        isLocal: false,
        isBootnode: isBootnodeIP(peer.ip) || isBootnodeIP(extractIPFromAddress(peer.address || '')),
      });
    });

    // Draw mesh connections
    meshData.meshConnections.forEach((conn) => {
      const nodeA = nodes.find(n => n.id.includes(conn.peerA.slice(0, 12)) || conn.peerA.includes(n.id.slice(0, 12)));
      const nodeB = nodes.find(n => n.id.includes(conn.peerB.slice(0, 12)) || conn.peerB.includes(n.id.slice(0, 12)));
      if (nodeA && nodeB) {
        ctx.beginPath();
        ctx.moveTo(nodeA.x, nodeA.y);
        ctx.lineTo(nodeB.x, nodeB.y);
        ctx.strokeStyle = conn.connectionType === 'mesh' ? 'rgba(34, 197, 94, 0.4)' : 'rgba(59, 130, 246, 0.4)';
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    });

    // Draw connections from local to peers
    const localNode = nodes[0];
    nodes.slice(1).forEach((peerNode) => {
      const gradient = ctx.createLinearGradient(localNode.x, localNode.y, peerNode.x, peerNode.y);
      gradient.addColorStop(0, 'rgba(139, 92, 246, 0.8)');
      gradient.addColorStop(1, 'rgba(139, 92, 246, 0.2)');
      ctx.beginPath();
      ctx.moveTo(localNode.x, localNode.y);
      ctx.lineTo(peerNode.x, peerNode.y);
      ctx.strokeStyle = gradient;
      ctx.lineWidth = 2;
      ctx.stroke();
    });

    // Draw nodes with glow
    nodes.forEach((node) => {
      const nodeRadius = node.isLocal ? 18 : node.isBootnode ? 12 : 8;
      ctx.beginPath();
      ctx.arc(node.x, node.y, nodeRadius + 4, 0, 2 * Math.PI);
      ctx.fillStyle = node.isLocal ? 'rgba(139, 92, 246, 0.3)' : node.isBootnode ? 'rgba(245, 158, 11, 0.3)' : 'rgba(34, 197, 94, 0.3)';
      ctx.fill();
      ctx.beginPath();
      ctx.arc(node.x, node.y, nodeRadius, 0, 2 * Math.PI);
      ctx.fillStyle = node.isLocal ? '#8b5cf6' : node.isBootnode ? '#f59e0b' : '#22c55e';
      ctx.fill();
      ctx.strokeStyle = '#1c1917';
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.fillStyle = '#a8a29e';
      ctx.font = '10px Inter, sans-serif';
      ctx.textAlign = 'center';
      ctx.fillText(node.label, node.x, node.y + nodeRadius + 14);
    });
  }, [meshData, peers]);

  const handleNetworkChange = async (network: 'testnet' | 'devnet') => {
    setSelectedNetwork(network);
    try {
      const currentSettings = await invoke<any>('get_settings');
      await invoke('save_settings', { 
        settings: { ...currentSettings, network } 
      });
    } catch (e) {
      console.error('Failed to save network:', e);
    }
  };

  const totalBalance = addresses.reduce((sum, addr) => {
    return sum + parseFloat(addr.balance || '0');
  }, 0);

  const handleStartNode = async () => {
    try {
      await startNode();
      // Also try to connect to remote bootnode for logs
      invoke('start_remote_log_stream').catch(console.error);
    } catch (e) {
      console.error('Failed to start node:', e);
    }
  };

  const handleStopNode = async () => {
    try {
      await stopNode();
    } catch (e) {
      console.error('Failed to stop node:', e);
    }
  };

  const handleCheckUpdate = async () => {
    setUpdateChecking(true);
    try {
      const { shouldUpdate, manifest } = await checkUpdate();
      if (shouldUpdate && manifest) {
        setUpdateAvailable(true);
        setUpdateInfo({ version: manifest.version, body: manifest.body || '' });
        addLog('info', 'system', `Update available: v${manifest.version}`);
        
        // Ask user and install if they confirm
        if (window.confirm(`Update to v${manifest.version}?\n\n${manifest.body || 'New version available.'}`)) {
          addLog('info', 'system', 'Installing update...');
          await installUpdate();
          await relaunch();
        }
      } else {
        addLog('info', 'system', 'No updates available - you have the latest version!');
      }
    } catch (e) {
      console.error('Failed to check for updates:', e);
      addLog('error', 'system', `Update check failed: ${e}`);
    } finally {
      setUpdateChecking(false);
    }
  };

  // Health calculations - handle 0 as valid value with !== undefined checks
  const meshHealth = status?.meshPeers !== undefined && status.meshPeers !== null 
    ? Math.min(100, (status.meshPeers / 4) * 100) : 0;
  const totalDials = (status?.dialSuccesses ?? 0) + (status?.dialFailures ?? 0);
  const dialSuccess = totalDials > 0 ? ((status?.dialSuccesses ?? 0) / totalDials) * 100 : 50;
  const latencyScore = status?.averageRttMs !== undefined && status.averageRttMs !== null && status.averageRttMs > 0
    ? Math.max(0, Math.min(100, 100 - (status.averageRttMs / 5))) : 50;
  const overallHealth = Math.round((meshHealth + dialSuccess + latencyScore) / 3);

  const getHealthColor = (value: number) => value > 70 ? 'text-green-400' : value > 40 ? 'text-yellow-400' : 'text-red-400';

  return (
    <div className="p-4 lg:p-6 space-y-4 max-w-[1600px] mx-auto">
      {/* Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl lg:text-3xl font-bold bg-gradient-to-r from-white to-stone-400 bg-clip-text text-transparent">
            Network Dashboard
          </h1>
          <p className="text-sm text-stone-500 mt-1">Real-time node telemetry & P2P mesh status</p>
        </div>
        <div className="flex items-center gap-3">
          {/* Network Selector */}
          <div className="flex items-center gap-1 bg-dark-800/80 backdrop-blur rounded-xl p-1 border border-dark-600">
            <Globe size={14} className="ml-2 text-stone-500" />
            {(['testnet', 'devnet'] as const).map((net) => (
              <button
                key={net}
                onClick={() => handleNetworkChange(net)}
                disabled={status?.running}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
                  selectedNetwork === net
                    ? 'bg-gradient-to-r from-pyrax-600 to-pyrax-500 text-white shadow-lg shadow-pyrax-500/20'
                    : 'text-stone-400 hover:text-white hover:bg-dark-700'
                } ${status?.running ? 'opacity-50 cursor-not-allowed' : ''}`}
              >
                {net.charAt(0).toUpperCase() + net.slice(1)}
              </button>
            ))}
          </div>
          {/* Node Control */}
          {status?.running ? (
            <button onClick={handleStopNode} disabled={nodeLoading}
              className="flex items-center gap-2 px-4 py-2 bg-gradient-to-r from-red-600 to-red-500 hover:from-red-500 hover:to-red-400 rounded-xl transition-all shadow-lg shadow-red-500/20 disabled:opacity-50">
              <Square size={14} /><span className="text-sm font-medium">Stop</span>
            </button>
          ) : (
            <button onClick={handleStartNode} disabled={nodeLoading}
              className="flex items-center gap-2 px-4 py-2 bg-gradient-to-r from-green-600 to-emerald-500 hover:from-green-500 hover:to-emerald-400 rounded-xl transition-all shadow-lg shadow-green-500/20 disabled:opacity-50">
              {nodeLoading ? <RefreshCw size={14} className="animate-spin" /> : <Play size={14} />}
              <span className="text-sm font-medium">Start Node</span>
            </button>
          )}
          {/* Check for Updates */}
          <button 
            onClick={handleCheckUpdate} 
            disabled={updateChecking}
            className={`flex items-center gap-2 px-3 py-2 rounded-xl transition-all border ${
              updateAvailable 
                ? 'bg-gradient-to-r from-blue-600 to-blue-500 hover:from-blue-500 hover:to-blue-400 border-blue-500/50 shadow-lg shadow-blue-500/20' 
                : 'bg-dark-800/80 hover:bg-dark-700 border-dark-600'
            } disabled:opacity-50`}
            title={updateAvailable ? `Update to ${updateInfo?.version}` : 'Check for updates'}
          >
            {updateChecking ? (
              <RefreshCw size={14} className="animate-spin text-stone-400" />
            ) : updateAvailable ? (
              <Download size={14} className="text-white" />
            ) : (
              <Download size={14} className="text-stone-400" />
            )}
            <span className={`text-xs font-medium ${updateAvailable ? 'text-white' : 'text-stone-400'}`}>
              {updateAvailable ? 'Update' : 'Updates'}
            </span>
          </button>
        </div>
      </div>

      {/* Connection Status Banner */}
      {status?.running && (
        <div className={`rounded-xl p-3 border flex items-center justify-between ${
          status.connected ? 'bg-green-500/5 border-green-500/20' : 'bg-yellow-500/5 border-yellow-500/20'
        }`}>
          <div className="flex items-center gap-3">
            <div className={`w-2 h-2 rounded-full ${status.connected ? 'bg-green-400 animate-pulse' : 'bg-yellow-400 animate-pulse'}`} />
            <span className={`text-sm font-medium ${status.connected ? 'text-green-400' : 'text-yellow-400'}`}>
              {status.connected ? 'Connected to Network' : 'Connecting...'}
            </span>
            {status.connected && (
              <span className="text-xs text-stone-500">
                {status.network} • Block #{status.blockHeight?.toLocaleString()}
              </span>
            )}
          </div>
          {status.connected && (
            <div className="flex items-center gap-4 text-xs">
              <span className="text-stone-400"><Users size={12} className="inline mr-1" />{peers.length} peers</span>
              <span className="text-stone-400"><Radio size={12} className="inline mr-1" />{status.meshPeers || 0} mesh</span>
            </div>
          )}
        </div>
      )}

      {/* Main Dashboard Grid */}
      {status?.connected ? (
        <>
          {/* Stats Row */}
          <div className="grid grid-cols-2 lg:grid-cols-6 gap-3">
            <MiniStatCard icon={<Blocks size={16} />} label="Block Height" value={status.blockHeight?.toLocaleString() || '0'} color="purple" />
            <MiniStatCard icon={<Users size={16} />} label="Peers" value={`${peers.length}`} color="blue" />
            <MiniStatCard icon={<Radio size={16} />} label="Mesh" value={`${status.meshPeers || 0}`} color="green" />
            <MiniStatCard icon={<Signal size={16} />} label="Gossip" value={`${status.gossipPeers || 0}`} color="cyan" />
            <MiniStatCard icon={<Gauge size={16} />} label="Latency" value={status.averageRttMs ? `${status.averageRttMs}ms` : 'N/A'} color="yellow" />
            <MiniStatCard icon={<Zap size={16} />} label="Hashrate" value={minerStatus?.running ? formatHashrate(minerStatus.hashrate) : '0 H/s'} color="amber" />
          </div>

          {/* Main Content: Health + Mesh Viz + Telemetry */}
          <div className="grid grid-cols-1 lg:grid-cols-12 gap-4">
            {/* Left: Network Health */}
            <div className="lg:col-span-3 bg-dark-800/60 backdrop-blur rounded-2xl p-5 border border-dark-600/50">
              <h3 className="text-sm font-semibold text-stone-300 mb-4 flex items-center gap-2">
                <TrendingUp size={16} className="text-pyrax-400" />
                Network Health
              </h3>
              {/* Health Ring */}
              <div className="flex items-center justify-center mb-4">
                <div className="relative w-28 h-28">
                  <svg className="w-full h-full transform -rotate-90">
                    <circle cx="56" cy="56" r="48" stroke="rgba(68, 64, 60, 0.3)" strokeWidth="8" fill="none" />
                    <circle cx="56" cy="56" r="48" 
                      stroke={overallHealth > 70 ? '#22c55e' : overallHealth > 40 ? '#eab308' : '#ef4444'}
                      strokeWidth="8" fill="none" strokeLinecap="round"
                      strokeDasharray={`${(overallHealth / 100) * 301} 301`}
                      className="transition-all duration-1000" />
                  </svg>
                  <div className="absolute inset-0 flex items-center justify-center flex-col">
                    <span className={`text-2xl font-bold ${getHealthColor(overallHealth)}`}>{overallHealth}</span>
                    <span className="text-xs text-stone-500">Health</span>
                  </div>
                </div>
              </div>
              {/* Health Bars */}
              <div className="space-y-3">
                <HealthBar label="Mesh Health" value={meshHealth} />
                <HealthBar label="Dial Success" value={dialSuccess} />
                <HealthBar label="Latency Score" value={latencyScore} />
              </div>
              {/* P2P Stats */}
              <div className="mt-4 pt-4 border-t border-dark-600/50 grid grid-cols-2 gap-2 text-xs">
                <div className="flex items-center gap-1.5 text-stone-400">
                  <ArrowDownLeft size={12} className="text-blue-400" />
                  <span>In: {status.inboundPeers || 0}</span>
                </div>
                <div className="flex items-center gap-1.5 text-stone-400">
                  <ArrowUpRight size={12} className="text-purple-400" />
                  <span>Out: {status.outboundPeers || 0}</span>
                </div>
                <div className="flex items-center gap-1.5 text-stone-400">
                  <Activity size={12} className="text-green-400" />
                  <span>Dials: {status.dialSuccesses || 0}/{totalDials}</span>
                </div>
                <div className="flex items-center gap-1.5 text-stone-400">
                  <Clock size={12} className="text-yellow-400" />
                  <span>RTT: {status.averageRttMs || 0}ms</span>
                </div>
              </div>
            </div>

            {/* Center: P2P Mesh Visualization */}
            <div className="lg:col-span-5 bg-dark-800/60 backdrop-blur rounded-2xl p-5 border border-dark-600/50">
              <h3 className="text-sm font-semibold text-stone-300 mb-3 flex items-center gap-2">
                <Network size={16} className="text-pyrax-400" />
                P2P Mesh Topology
                <span className="ml-auto text-xs text-stone-500">{peers.length} connected</span>
              </h3>
              <div className="relative h-64">
                <canvas ref={canvasRef} className="w-full h-full" />
                {peers.length === 0 && (
                  <div className="absolute inset-0 flex items-center justify-center text-stone-500 text-sm">
                    <Network size={24} className="mr-2 opacity-50" />
                    Discovering peers...
                  </div>
                )}
              </div>
              {/* Legend */}
              <div className="flex items-center justify-center gap-4 mt-3 text-xs text-stone-500">
                <span className="flex items-center gap-1"><div className="w-2 h-2 rounded-full bg-pyrax-500" /> You</span>
                <span className="flex items-center gap-1"><div className="w-2 h-2 rounded-full bg-amber-500" /> Bootnode</span>
                <span className="flex items-center gap-1"><div className="w-2 h-2 rounded-full bg-green-500" /> Peer</span>
              </div>
            </div>

            {/* Right: Telemetry Charts */}
            <div className="lg:col-span-4 space-y-4">
              {/* Peer History Chart */}
              <div className="bg-dark-800/60 backdrop-blur rounded-2xl p-4 border border-dark-600/50">
                <h3 className="text-xs font-semibold text-stone-400 mb-2 flex items-center gap-2">
                  <Users size={14} className="text-blue-400" />
                  Peer Connections
                </h3>
                <div className="h-24">
                  {telemetryHistory.length > 1 ? (
                    <MiniLineChart data={telemetryHistory.map(d => d.peerCount)} color="#8b5cf6" />
                  ) : (
                    <div className="h-full flex items-center justify-center text-stone-600 text-xs">Collecting data...</div>
                  )}
                </div>
              </div>
              {/* Mesh Peers Chart */}
              <div className="bg-dark-800/60 backdrop-blur rounded-2xl p-4 border border-dark-600/50">
                <h3 className="text-xs font-semibold text-stone-400 mb-2 flex items-center gap-2">
                  <Radio size={14} className="text-green-400" />
                  Mesh Peers
                </h3>
                <div className="h-24">
                  {telemetryHistory.length > 1 ? (
                    <MiniLineChart data={telemetryHistory.map(d => d.meshPeers)} color="#22c55e" />
                  ) : (
                    <div className="h-full flex items-center justify-center text-stone-600 text-xs">Collecting data...</div>
                  )}
                </div>
              </div>
              {/* Latency Chart */}
              <div className="bg-dark-800/60 backdrop-blur rounded-2xl p-4 border border-dark-600/50">
                <h3 className="text-xs font-semibold text-stone-400 mb-2 flex items-center gap-2">
                  <Gauge size={14} className="text-yellow-400" />
                  Latency (ms)
                </h3>
                <div className="h-24">
                  {telemetryHistory.length > 1 ? (
                    <MiniLineChart data={telemetryHistory.map(d => d.latency)} color="#eab308" />
                  ) : (
                    <div className="h-full flex items-center justify-center text-stone-600 text-xs">Collecting data...</div>
                  )}
                </div>
              </div>
            </div>
          </div>

          {/* Connected Peers List */}
          <div className="bg-dark-800/60 backdrop-blur rounded-2xl p-5 border border-dark-600/50">
            <h3 className="text-sm font-semibold text-stone-300 mb-4 flex items-center gap-2">
              <Server size={16} className="text-pyrax-400" />
              Connected Peers
              <span className="ml-auto text-xs font-normal text-stone-500">{peers.length} active connections</span>
            </h3>
            {peers.length > 0 ? (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-2 max-h-48 overflow-y-auto">
                {peers.slice(0, 12).map((peer) => (
                  <div key={peer.id} className="flex items-center gap-3 bg-dark-700/50 rounded-lg px-3 py-2">
                    <div className={`w-2 h-2 rounded-full ${isBootnodeIP(peer.ip) ? 'bg-amber-500' : 'bg-green-500'}`} />
                    <div className="min-w-0 flex-1">
                      <div className="text-xs font-mono text-stone-300 truncate">{peer.id.slice(0, 16)}...</div>
                      <div className="text-[10px] text-stone-500">{peer.ip}:{peer.port} • {peer.direction}</div>
                    </div>
                    <div className="text-[10px] text-stone-500">#{peer.blockHeight}</div>
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-center py-6 text-stone-500 text-sm">No peers connected yet</div>
            )}
          </div>
        </>
      ) : (
        /* Disconnected State */
        <div className="bg-dark-800/60 backdrop-blur rounded-2xl p-12 border border-dark-600/50 text-center">
          <Network size={48} className="mx-auto text-stone-600 mb-4" />
          <h2 className="text-xl font-semibold text-stone-300 mb-2">Node Not Running</h2>
          <p className="text-stone-500 mb-6 max-w-md mx-auto">
            Start your PYRAX node to connect to the {selectedNetwork} network and see real-time telemetry.
          </p>
          <button onClick={handleStartNode} disabled={nodeLoading}
            className="inline-flex items-center gap-2 px-6 py-3 bg-gradient-to-r from-pyrax-600 to-pyrax-500 hover:from-pyrax-500 hover:to-pyrax-400 rounded-xl transition-all shadow-lg shadow-pyrax-500/20 disabled:opacity-50">
            {nodeLoading ? <RefreshCw size={18} className="animate-spin" /> : <Play size={18} />}
            <span className="font-medium">Start Node</span>
          </button>
        </div>
      )}

      {/* NEURAX AI Insights */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <div className="lg:col-span-2">
          <LogViewer />
        </div>
        <div>
          <NeuraxInsightsPanel />
        </div>
      </div>

      {/* Quick Actions */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <QuickAction title="View Wallet" description="Check balances and send transactions" to="/wallet" icon={<Activity size={18} />} />
        <QuickAction title="Start Mining" description="Earn PYRAX by mining blocks" to="/mining" icon={<Zap size={18} />} />
        <QuickAction title="Explore Blocks" description="Browse the blockchain" to="/explorer" icon={<Blocks size={18} />} />
      </div>
    </div>
  );
}

// Mini stat card component
function MiniStatCard({ icon, label, value, color }: {
  icon: React.ReactNode;
  label: string;
  value: string;
  color: 'purple' | 'blue' | 'green' | 'cyan' | 'yellow' | 'amber';
}) {
  const colorClasses = {
    purple: 'text-pyrax-400 bg-pyrax-500/10 border-pyrax-500/20',
    blue: 'text-blue-400 bg-blue-500/10 border-blue-500/20',
    green: 'text-green-400 bg-green-500/10 border-green-500/20',
    cyan: 'text-cyan-400 bg-cyan-500/10 border-cyan-500/20',
    yellow: 'text-yellow-400 bg-yellow-500/10 border-yellow-500/20',
    amber: 'text-amber-400 bg-amber-500/10 border-amber-500/20',
  };
  return (
    <div className={`rounded-xl p-3 border ${colorClasses[color]}`}>
      <div className="flex items-center gap-2 mb-1">
        <span className={colorClasses[color].split(' ')[0]}>{icon}</span>
        <span className="text-xs text-stone-500">{label}</span>
      </div>
      <div className="text-lg font-bold text-white">{value}</div>
    </div>
  );
}

// Health bar component
function HealthBar({ label, value }: { label: string; value: number }) {
  const getColor = (v: number) => v > 70 ? 'bg-green-500' : v > 40 ? 'bg-yellow-500' : 'bg-red-500';
  return (
    <div>
      <div className="flex justify-between text-xs mb-1">
        <span className="text-stone-400">{label}</span>
        <span className="text-stone-300">{Math.round(value)}%</span>
      </div>
      <div className="h-1.5 bg-dark-600 rounded-full overflow-hidden">
        <div className={`h-full ${getColor(value)} transition-all duration-500`} style={{ width: `${value}%` }} />
      </div>
    </div>
  );
}

// Mini line chart using canvas
function MiniLineChart({ data, color }: { data: number[]; color: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || data.length < 2) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    const width = rect.width;
    const height = rect.height;
    const padding = 4;
    const maxVal = Math.max(...data, 1);
    const minVal = Math.min(...data, 0);
    const range = maxVal - minVal || 1;

    ctx.clearRect(0, 0, width, height);

    // Draw gradient fill
    const gradient = ctx.createLinearGradient(0, 0, 0, height);
    gradient.addColorStop(0, color + '40');
    gradient.addColorStop(1, color + '00');

    ctx.beginPath();
    ctx.moveTo(padding, height - padding);
    data.forEach((val, i) => {
      const x = padding + (i / (data.length - 1)) * (width - padding * 2);
      const y = height - padding - ((val - minVal) / range) * (height - padding * 2);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.lineTo(width - padding, height - padding);
    ctx.lineTo(padding, height - padding);
    ctx.fillStyle = gradient;
    ctx.fill();

    // Draw line
    ctx.beginPath();
    data.forEach((val, i) => {
      const x = padding + (i / (data.length - 1)) * (width - padding * 2);
      const y = height - padding - ((val - minVal) / range) * (height - padding * 2);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.stroke();

    // Draw end point
    const lastX = width - padding;
    const lastY = height - padding - ((data[data.length - 1] - minVal) / range) * (height - padding * 2);
    ctx.beginPath();
    ctx.arc(lastX, lastY, 3, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.fill();
  }, [data, color]);

  return <canvas ref={canvasRef} className="w-full h-full" />;
}

// Quick action card
function QuickAction({ title, description, to, icon }: {
  title: string;
  description: string;
  to: string;
  icon: React.ReactNode;
}) {
  return (
    <Link
      to={to}
      className="flex items-center gap-4 bg-dark-800/60 hover:bg-dark-700/60 rounded-xl p-4 transition-all border border-dark-600/50 hover:border-pyrax-500/50 group"
    >
      <div className="p-2 rounded-lg bg-pyrax-500/10 text-pyrax-400 group-hover:bg-pyrax-500/20 transition-colors">
        {icon}
      </div>
      <div>
        <h3 className="font-semibold text-stone-200 group-hover:text-white transition-colors">{title}</h3>
        <p className="text-xs text-stone-500">{description}</p>
      </div>
      <ChevronRight size={16} className="ml-auto text-stone-600 group-hover:text-pyrax-400 transition-colors" />
    </Link>
  );
}
