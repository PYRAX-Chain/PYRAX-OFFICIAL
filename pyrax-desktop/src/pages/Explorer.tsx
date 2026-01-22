import { useState, useEffect, useCallback } from 'react';
import { Search, Blocks, ArrowRight, Clock, Hash, Activity, Users, Globe, RefreshCw, Wallet } from 'lucide-react';
import { invoke } from '@tauri-apps/api/tauri';
import { useNodeStore } from '../stores/nodeStore';
import { truncateHash, formatTimeAgo } from '../lib/utils';
import NetworkNodes from '../components/NetworkNodes';

interface BlockInfo {
  hash: string;
  height: number;
  parent_hash: string;
  timestamp: number;
  difficulty: string;
  beneficiary: string;
  transaction_count: number;
  size: number;
}

interface NetworkStats {
  block_height: number;
  difficulty: string;
  peer_count: number;
  pending_tx_count: number;
  chain_id: number;
  syncing: boolean;
}

interface SearchResult {
  result_type: string;
  block?: BlockInfo;
  address?: { address: string; balance_formatted: string; transaction_count: number; };
}

interface Transaction {
  hash: string;
  from: string;
  to?: string;
  value: string;
  blockNumber?: string;
}

type TabType = 'dashboard' | 'blocks' | 'network';

export default function Explorer() {
  const { status } = useNodeStore();
  const [activeTab, setActiveTab] = useState<TabType>('dashboard');
  const [searchQuery, setSearchQuery] = useState('');
  const [recentBlocks, setRecentBlocks] = useState<BlockInfo[]>([]);
  const [selectedBlock, setSelectedBlock] = useState<BlockInfo | null>(null);
  const [networkStats, setNetworkStats] = useState<NetworkStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (status?.connected) {
      fetchRecentBlocks();
    }
  }, [status?.connected]);

  const fetchRecentBlocks = async () => {
    try {
      const blocks = await invoke<BlockInfo[]>('get_recent_blocks', { count: 10 });
      setRecentBlocks(blocks);
    } catch (e) {
      console.error('Failed to fetch blocks:', e);
    }
  };

  const handleSearch = async () => {
    if (!searchQuery.trim()) return;
    setLoading(true);
    try {
      if (searchQuery.startsWith('0x') && searchQuery.length === 66) {
        // Block or transaction hash
        const block = await invoke<BlockInfo | null>('get_block', { identifier: searchQuery });
        if (block) {
          setSelectedBlock(block);
        } else {
          const tx = await invoke<Transaction | null>('get_transaction', { hash: searchQuery });
          if (tx) {
            // Show transaction details
          }
        }
      } else if (/^\d+$/.test(searchQuery)) {
        // Block number
        const block = await invoke<BlockInfo | null>('get_block', { identifier: searchQuery });
        if (block) {
          setSelectedBlock(block);
        }
      }
    } catch (e) {
      console.error('Search failed:', e);
    } finally {
      setLoading(false);
    }
  };

  if (!status?.connected) {
    return (
      <div className="p-6">
        <div className="text-center py-12 text-stone-400">
          <Blocks size={64} className="mx-auto mb-4 opacity-50" />
          <p>Connect to a node to explore the blockchain</p>
        </div>
      </div>
    );
  }

  return (
    <div className="p-6 space-y-6">
      <h1 className="text-2xl font-bold">Block Explorer</h1>

      {/* Search Bar */}
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-stone-400" size={20} />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
            placeholder="Search by block number, hash, or transaction..."
            className="w-full pl-10 pr-4 py-3 bg-dark-800 border border-dark-600 rounded-lg focus:border-pyrax-500 outline-none"
          />
        </div>
        <button
          onClick={handleSearch}
          disabled={loading}
          className="px-6 py-3 bg-pyrax-600 hover:bg-pyrax-700 rounded-lg transition-colors disabled:opacity-50"
        >
          Search
        </button>
      </div>

      {/* Network Nodes */}
      <NetworkNodes />

      {/* Selected Block Details */}
      {selectedBlock && (
        <div className="bg-dark-800 rounded-xl p-6">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold">Block #{selectedBlock.height.toLocaleString()}</h2>
            <button onClick={() => setSelectedBlock(null)} className="text-stone-400 hover:text-white">
              ✕
            </button>
          </div>
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div>
              <div className="text-stone-500">Hash</div>
              <div className="font-mono">{truncateHash(selectedBlock.hash, 16)}</div>
            </div>
            <div>
              <div className="text-stone-500">Parent Hash</div>
              <div className="font-mono">{truncateHash(selectedBlock.parent_hash, 16)}</div>
            </div>
            <div>
              <div className="text-stone-500">Timestamp</div>
              <div>{new Date(selectedBlock.timestamp * 1000).toLocaleString()}</div>
            </div>
            <div>
              <div className="text-stone-500">Miner</div>
              <div className="font-mono">{truncateHash(selectedBlock.beneficiary, 10)}</div>
            </div>
            <div>
              <div className="text-stone-500">Transactions</div>
              <div>{selectedBlock.transaction_count}</div>
            </div>
            <div>
              <div className="text-stone-500">Size</div>
              <div>{selectedBlock.size.toLocaleString()} bytes</div>
            </div>
          </div>
        </div>
      )}

      {/* Recent Blocks */}
      <div className="bg-dark-800 rounded-xl p-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold flex items-center gap-2">
            <Blocks size={20} className="text-pyrax-400" />
            Recent Blocks
          </h2>
          <button
            onClick={fetchRecentBlocks}
            className="text-sm text-pyrax-400 hover:text-pyrax-300"
          >
            Refresh
          </button>
        </div>
        
        {recentBlocks.length === 0 ? (
          <p className="text-center py-8 text-stone-400">No blocks found</p>
        ) : (
          <div className="space-y-2">
            {recentBlocks.map((block) => (
              <div
                key={block.hash}
                onClick={() => setSelectedBlock(block)}
                className="flex items-center justify-between p-4 bg-dark-700 rounded-lg cursor-pointer hover:bg-dark-600 transition-colors"
              >
                <div className="flex items-center gap-4">
                  <div className="w-12 h-12 bg-pyrax-600/20 rounded-lg flex items-center justify-center">
                    <Blocks className="text-pyrax-400" size={24} />
                  </div>
                  <div>
                    <div className="font-semibold">Block #{block.height.toLocaleString()}</div>
                    <div className="text-sm text-stone-400 flex items-center gap-2">
                      <Clock size={14} />
                      {formatTimeAgo(block.timestamp)}
                    </div>
                  </div>
                </div>
                <div className="text-right">
                  <div className="text-sm">{block.transaction_count} txs</div>
                  <div className="text-xs text-stone-500 font-mono">{truncateHash(block.hash)}</div>
                </div>
                <ArrowRight className="text-stone-500" size={20} />
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
