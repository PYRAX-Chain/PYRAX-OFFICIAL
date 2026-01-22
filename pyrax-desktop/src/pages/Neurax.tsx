import { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { useLogStore } from '../stores/logStore';
import { useNodeStore } from '../stores/nodeStore';
import { 
  Brain, Shield, Cpu, HardDrive, Wifi, Zap, MessageSquare, 
  Settings, ChevronRight, AlertTriangle, CheckCircle, Info,
  Send, Trash2, RefreshCw, Thermometer, Activity, MemoryStick,
  Power, Network, Lock, Unlock, Eye, EyeOff
} from 'lucide-react';

// Types matching Rust backend
interface NeuraxConfig {
  enabled: boolean;
  permissions: NeuraxPermissions;
  model_downloaded: boolean;
  model_path: string | null;
  last_analysis: number | null;
  proactive_mode: boolean;
}

interface NeuraxPermissions {
  basic_analysis: boolean;
  process_management: boolean;
  power_settings: boolean;
  network_tuning: boolean;
  memory_optimization: boolean;
  p2p_intelligence: boolean;
  auto_apply: boolean;
}

interface SystemMetrics {
  timestamp: number;
  cpu_usage: number;
  cpu_temp: number | null;
  memory_used_gb: number;
  memory_total_gb: number;
  memory_percent: number;
  gpu_usage: number | null;
  gpu_temp: number | null;
  gpu_memory_used_mb: number | null;
  gpu_memory_total_mb: number | null;
  disk_read_speed: number;
  disk_write_speed: number;
  network_rx_speed: number;
  network_tx_speed: number;
  process_count: number;
}

interface NeuraxInsight {
  id: string;
  timestamp: number;
  category: string;
  severity: string;
  title: string;
  description: string;
  suggestion: string | null;
  action_available: boolean;
  action_id: string | null;
  dismissed: boolean;
}

interface ChatMessage {
  id: string;
  timestamp: number;
  role: string;
  content: string;
}

interface QuickInsights {
  system_score: number;
  network_score: number;
  mining_score: number;
  top_insights: NeuraxInsight[];
  error_count: number;
  warning_count: number;
}

export default function Neurax() {
  const [config, setConfig] = useState<NeuraxConfig | null>(null);
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);
  const [insights, setInsights] = useState<NeuraxInsight[]>([]);
  const [chatHistory, setChatHistory] = useState<ChatMessage[]>([]);
  const [chatInput, setChatInput] = useState('');
  const [loading, setLoading] = useState(true);
  const [chatLoading, setChatLoading] = useState(false);
  const [activeTab, setActiveTab] = useState<'dashboard' | 'chat' | 'permissions'>('dashboard');
  const [quickInsights, setQuickInsights] = useState<QuickInsights | null>(null);
  const chatEndRef = useRef<HTMLDivElement>(null);
  
  // Get real data from stores
  const logs = useLogStore((state) => state.logs);
  const nodeStatus = useNodeStore((state) => state.status);

  // Load initial data
  useEffect(() => {
    loadConfig();
    loadMetrics();
    loadInsights();
    loadChatHistory();
    loadQuickInsights();
  }, []);

  // Auto-scroll chat
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [chatHistory]);

  // Refresh metrics periodically when enabled
  useEffect(() => {
    if (!config?.enabled) return;
    const interval = setInterval(() => {
      loadMetrics();
      loadQuickInsights();
    }, 5000);
    return () => clearInterval(interval);
  }, [config?.enabled, logs, nodeStatus]);

  const loadConfig = async () => {
    try {
      const cfg = await invoke<NeuraxConfig>('neurax_get_config');
      setConfig(cfg);
    } catch (e) {
      console.error('Failed to load NEURAX config:', e);
    } finally {
      setLoading(false);
    }
  };

  const loadMetrics = async () => {
    try {
      const m = await invoke<SystemMetrics>('neurax_get_system_metrics');
      setMetrics(m);
    } catch (e) {
      console.error('Failed to load metrics:', e);
    }
  };

  const loadInsights = async () => {
    try {
      // Use real logs from log store
      const logMessages = logs.map(l => `[${l.level.toUpperCase()}] [${l.category}] ${l.message}`);
      // Use real node status from node store
      const status = nodeStatus ? {
        peerCount: nodeStatus.peerCount,
        connected: nodeStatus.connected,
        syncing: nodeStatus.syncing,
        blockHeight: nodeStatus.blockHeight,
        meshPeers: nodeStatus.meshPeers,
        natStatus: nodeStatus.natStatus,
      } : null;
      const ins = await invoke<NeuraxInsight[]>('neurax_generate_insights', { logs: logMessages, nodeStatus: status });
      setInsights(ins);
    } catch (e) {
      console.error('Failed to load insights:', e);
    }
  };

  const loadQuickInsights = async () => {
    try {
      const logMessages = logs.map(l => `[${l.level.toUpperCase()}] [${l.category}] ${l.message}`);
      const status = nodeStatus ? {
        peerCount: nodeStatus.peerCount,
        connected: nodeStatus.connected,
        syncing: nodeStatus.syncing,
        blockHeight: nodeStatus.blockHeight,
        meshPeers: nodeStatus.meshPeers,
        natStatus: nodeStatus.natStatus,
      } : null;
      const qi = await invoke<QuickInsights>('neurax_get_quick_insights', { logs: logMessages, nodeStatus: status });
      setQuickInsights(qi);
    } catch (e) {
      console.error('Failed to load quick insights:', e);
    }
  };

  const loadChatHistory = async () => {
    try {
      const history = await invoke<ChatMessage[]>('neurax_get_chat_history');
      setChatHistory(history);
    } catch (e) {
      console.error('Failed to load chat history:', e);
    }
  };

  const toggleEnabled = async () => {
    if (!config) return;
    try {
      await invoke('neurax_set_enabled', { enabled: !config.enabled });
      setConfig({ ...config, enabled: !config.enabled });
    } catch (e) {
      console.error('Failed to toggle NEURAX:', e);
    }
  };

  // Sensitive permissions that require admin elevation
  const sensitivePermissions: (keyof NeuraxPermissions)[] = [
    'process_management',
    'memory_optimization',
    'auto_apply',
    'power_settings'
  ];

  const updatePermissions = async (key: keyof NeuraxPermissions, value: boolean) => {
    if (!config) return;
    const newPermissions = { ...config.permissions, [key]: value };
    
    // Request elevation for sensitive permissions being enabled
    const needsElevation = value && sensitivePermissions.includes(key);
    
    try {
      const result = await invoke<{
        success: boolean;
        elevated: boolean;
        message: string;
        permissions_changed: string[];
      }>('neurax_set_permissions', { 
        permissions: newPermissions,
        requestElevation: needsElevation 
      });
      
      if (result.success) {
        setConfig({ ...config, permissions: newPermissions });
        if (result.elevated) {
          console.log('Admin elevation granted:', result.message);
        }
      } else {
        // User declined elevation - don't update the toggle
        console.log('Permission change cancelled:', result.message);
      }
    } catch (e) {
      console.error('Failed to update permissions:', e);
    }
  };

  const sendMessage = async () => {
    if (!chatInput.trim() || chatLoading) return;
    
    const userMessage = chatInput.trim();
    setChatInput('');
    setChatLoading(true);
    
    // Optimistically add user message
    const tempUserMsg: ChatMessage = {
      id: `temp-${Date.now()}`,
      timestamp: Date.now(),
      role: 'user',
      content: userMessage,
    };
    setChatHistory(prev => [...prev, tempUserMsg]);
    
    try {
      // Use real logs from log store
      const logMessages = logs.map(l => `[${l.level.toUpperCase()}] [${l.category}] ${l.message}`);
      // Use real node status from node store
      const status = nodeStatus ? {
        peerCount: nodeStatus.peerCount,
        connected: nodeStatus.connected,
        syncing: nodeStatus.syncing,
        blockHeight: nodeStatus.blockHeight,
        meshPeers: nodeStatus.meshPeers,
        natStatus: nodeStatus.natStatus,
      } : null;
      const response = await invoke<ChatMessage>('neurax_chat', { 
        message: userMessage, 
        logs: logMessages, 
        nodeStatus: status 
      });
      
      // Replace temp message and add response
      setChatHistory(prev => [
        ...prev.filter(m => m.id !== tempUserMsg.id),
        { ...tempUserMsg, id: `user-${Date.now()}` },
        response
      ]);
    } catch (e) {
      console.error('Failed to send message:', e);
    } finally {
      setChatLoading(false);
    }
  };

  const clearChat = async () => {
    try {
      await invoke('neurax_clear_chat');
      setChatHistory([]);
    } catch (e) {
      console.error('Failed to clear chat:', e);
    }
  };

  const dismissInsight = async (id: string) => {
    try {
      await invoke('neurax_dismiss_insight', { insightId: id });
      setInsights(prev => prev.map(i => i.id === id ? { ...i, dismissed: true } : i));
    } catch (e) {
      console.error('Failed to dismiss insight:', e);
    }
  };

  const executeAction = async (actionId: string) => {
    try {
      const result = await invoke<{ success: boolean; message: string }>('neurax_execute_action', { actionId });
      if (result.success) {
        loadInsights();
      }
    } catch (e) {
      console.error('Failed to execute action:', e);
    }
  };

  const getSeverityColor = (severity: string) => {
    switch (severity) {
      case 'critical': return 'text-red-400 bg-red-500/10 border-red-500/30';
      case 'warning': return 'text-yellow-400 bg-yellow-500/10 border-yellow-500/30';
      default: return 'text-blue-400 bg-blue-500/10 border-blue-500/30';
    }
  };

  const getScoreColor = (score: number) => {
    if (score >= 80) return 'text-green-400';
    if (score >= 60) return 'text-yellow-400';
    return 'text-red-400';
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <RefreshCw className="w-8 h-8 animate-spin text-pyrax-500" />
      </div>
    );
  }

  return (
    <div className="p-4 lg:p-6 space-y-4 max-w-[1600px] mx-auto">
      {/* Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-gradient-to-br from-purple-600 to-blue-600 rounded-xl">
            <Brain className="w-6 h-6 text-white" />
          </div>
          <div>
            <h1 className="text-2xl lg:text-3xl font-bold bg-gradient-to-r from-purple-400 to-blue-400 bg-clip-text text-transparent">
              NEURAX
            </h1>
            <p className="text-sm text-stone-500">AI-Powered System Optimizer</p>
          </div>
        </div>
        
        {/* Enable Toggle */}
        <button
          onClick={toggleEnabled}
          className={`flex items-center gap-2 px-4 py-2 rounded-xl transition-all ${
            config?.enabled
              ? 'bg-gradient-to-r from-purple-600 to-blue-600 text-white shadow-lg shadow-purple-500/20'
              : 'bg-dark-800 text-stone-400 hover:bg-dark-700'
          }`}
        >
          {config?.enabled ? <Unlock size={16} /> : <Lock size={16} />}
          <span className="font-medium">{config?.enabled ? 'Enabled' : 'Disabled'}</span>
        </button>
      </div>

      {/* Not Enabled State */}
      {!config?.enabled && (
        <div className="bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-8 text-center">
          <Brain className="w-16 h-16 mx-auto text-purple-500 opacity-50 mb-4" />
          <h2 className="text-xl font-semibold mb-2">NEURAX is Disabled</h2>
          <p className="text-stone-400 mb-6 max-w-md mx-auto">
            Enable NEURAX to get AI-powered insights, system optimization suggestions, 
            and real-time performance monitoring.
          </p>
          <button
            onClick={toggleEnabled}
            className="px-6 py-3 bg-gradient-to-r from-purple-600 to-blue-600 rounded-xl font-medium hover:from-purple-500 hover:to-blue-500 transition-all"
          >
            Enable NEURAX
          </button>
        </div>
      )}

      {/* Main Content - Only show when enabled */}
      {config?.enabled && (
        <>
          {/* Tab Navigation */}
          <div className="flex gap-2 p-1 bg-dark-800/50 rounded-xl w-fit">
            {(['dashboard', 'chat', 'permissions'] as const).map((tab) => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                className={`px-4 py-2 rounded-lg text-sm font-medium transition-all ${
                  activeTab === tab
                    ? 'bg-gradient-to-r from-purple-600 to-blue-600 text-white'
                    : 'text-stone-400 hover:text-white hover:bg-dark-700'
                }`}
              >
                {tab === 'dashboard' && <Activity className="w-4 h-4 inline mr-2" />}
                {tab === 'chat' && <MessageSquare className="w-4 h-4 inline mr-2" />}
                {tab === 'permissions' && <Shield className="w-4 h-4 inline mr-2" />}
                {tab.charAt(0).toUpperCase() + tab.slice(1)}
              </button>
            ))}
          </div>

          {/* Dashboard Tab */}
          {activeTab === 'dashboard' && (
            <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
              {/* System Metrics */}
              <div className="lg:col-span-2 bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-4">
                <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
                  <Cpu className="w-5 h-5 text-purple-400" />
                  System Metrics
                </h3>
                
                <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                  {/* CPU */}
                  <div className="bg-dark-900/50 rounded-xl p-3">
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-xs text-stone-500">CPU</span>
                      <Cpu className="w-4 h-4 text-blue-400" />
                    </div>
                    <div className="text-2xl font-bold">{metrics?.cpu_usage?.toFixed(1) ?? '0'}%</div>
                    {metrics?.cpu_temp !== null && metrics?.cpu_temp !== undefined && (
                      <div className="text-xs text-stone-500 flex items-center gap-1 mt-1">
                        <Thermometer className="w-3 h-3" />
                        {metrics?.cpu_temp?.toFixed(0)}°C
                      </div>
                    )}
                  </div>
                  
                  {/* Memory */}
                  <div className="bg-dark-900/50 rounded-xl p-3">
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-xs text-stone-500">Memory</span>
                      <MemoryStick className="w-4 h-4 text-green-400" />
                    </div>
                    <div className="text-2xl font-bold">{metrics?.memory_percent?.toFixed(1) ?? '0'}%</div>
                    <div className="text-xs text-stone-500 mt-1">
                      {metrics?.memory_used_gb?.toFixed(1) ?? '0'} / {metrics?.memory_total_gb?.toFixed(1) ?? '0'} GB
                    </div>
                  </div>
                  
                  {/* GPU */}
                  <div className="bg-dark-900/50 rounded-xl p-3">
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-xs text-stone-500">GPU</span>
                      <Zap className="w-4 h-4 text-yellow-400" />
                    </div>
                    <div className="text-2xl font-bold">
                      {metrics?.gpu_usage !== null && metrics?.gpu_usage !== undefined ? `${metrics.gpu_usage.toFixed(0)}%` : 'N/A'}
                    </div>
                    {metrics?.gpu_temp !== null && metrics?.gpu_temp !== undefined && (
                      <div className="text-xs text-stone-500 flex items-center gap-1 mt-1">
                        <Thermometer className="w-3 h-3" />
                        {metrics?.gpu_temp?.toFixed(0)}°C
                      </div>
                    )}
                  </div>
                  
                  {/* Processes */}
                  <div className="bg-dark-900/50 rounded-xl p-3">
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-xs text-stone-500">Processes</span>
                      <Activity className="w-4 h-4 text-purple-400" />
                    </div>
                    <div className="text-2xl font-bold">{metrics?.process_count}</div>
                    <div className="text-xs text-stone-500 mt-1">Running</div>
                  </div>
                </div>
              </div>

              {/* Scores */}
              <div className="bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-4">
                <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
                  <Shield className="w-5 h-5 text-green-400" />
                  Health Scores
                </h3>
                
                <div className="space-y-4">
                  {[
                    { name: 'System', score: quickInsights?.system_score ?? 0, icon: Cpu },
                    { name: 'Network', score: quickInsights?.network_score ?? 0, icon: Network },
                    { name: 'Mining', score: quickInsights?.mining_score ?? 0, icon: Zap },
                  ].map(({ name, score, icon: Icon }) => (
                    <div key={name} className="flex items-center gap-3">
                      <Icon className={`w-5 h-5 ${getScoreColor(score)}`} />
                      <div className="flex-1">
                        <div className="flex items-center justify-between mb-1">
                          <span className="text-sm">{name}</span>
                          <span className={`text-sm font-bold ${getScoreColor(score)}`}>{score}</span>
                        </div>
                        <div className="h-2 bg-dark-900 rounded-full overflow-hidden">
                          <div 
                            className={`h-full rounded-full transition-all ${
                              score >= 80 ? 'bg-green-500' : score >= 60 ? 'bg-yellow-500' : 'bg-red-500'
                            }`}
                            style={{ width: `${score}%` }}
                          />
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              {/* Insights */}
              <div className="lg:col-span-3 bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-4">
                <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
                  <AlertTriangle className="w-5 h-5 text-yellow-400" />
                  AI Insights
                </h3>
                
                <div className="space-y-3">
                  {insights.filter(i => !i.dismissed).length === 0 ? (
                    <div className="text-center py-8 text-stone-500">
                      <CheckCircle className="w-12 h-12 mx-auto mb-2 text-green-500 opacity-50" />
                      <p>All systems running optimally</p>
                    </div>
                  ) : (
                    insights.filter(i => !i.dismissed).map((insight) => (
                      <div 
                        key={insight.id}
                        className={`p-4 rounded-xl border ${getSeverityColor(insight.severity)}`}
                      >
                        <div className="flex items-start justify-between gap-4">
                          <div className="flex-1">
                            <h4 className="font-medium">{insight.title}</h4>
                            <p className="text-sm text-stone-400 mt-1">{insight.description}</p>
                            {insight.suggestion && (
                              <p className="text-sm text-stone-500 mt-2 flex items-center gap-1">
                                <Info className="w-3 h-3" />
                                {insight.suggestion}
                              </p>
                            )}
                          </div>
                          <div className="flex items-center gap-2">
                            {insight.action_available && insight.action_id && (
                              <button
                                onClick={() => executeAction(insight.action_id!)}
                                className="px-3 py-1 bg-purple-600 hover:bg-purple-500 rounded-lg text-xs font-medium transition-all"
                              >
                                Fix
                              </button>
                            )}
                            <button
                              onClick={() => dismissInsight(insight.id)}
                              className="p-1 hover:bg-dark-700 rounded transition-all"
                            >
                              <EyeOff className="w-4 h-4 text-stone-500" />
                            </button>
                          </div>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </div>
          )}

          {/* Chat Tab */}
          {activeTab === 'chat' && (
            <div className="bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 flex flex-col h-[600px]">
              {/* Chat Header */}
              <div className="p-4 border-b border-dark-600 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Brain className="w-5 h-5 text-purple-400" />
                  <span className="font-medium">Ask NEURAX</span>
                </div>
                <button
                  onClick={clearChat}
                  className="p-2 hover:bg-dark-700 rounded-lg transition-all"
                  title="Clear chat"
                >
                  <Trash2 className="w-4 h-4 text-stone-500" />
                </button>
              </div>
              
              {/* Chat Messages */}
              <div className="flex-1 overflow-y-auto p-4 space-y-4">
                {chatHistory.length === 0 ? (
                  <div className="text-center py-12 text-stone-500">
                    <MessageSquare className="w-12 h-12 mx-auto mb-2 opacity-50" />
                    <p>Ask NEURAX about your system, mining, or network issues</p>
                    <div className="mt-4 flex flex-wrap gap-2 justify-center">
                      {[
                        'Why is my hashrate low?',
                        'Are there any errors?',
                        'How is my network?',
                      ].map((suggestion) => (
                        <button
                          key={suggestion}
                          onClick={() => setChatInput(suggestion)}
                          className="px-3 py-1.5 bg-dark-700 hover:bg-dark-600 rounded-lg text-sm transition-all"
                        >
                          {suggestion}
                        </button>
                      ))}
                    </div>
                  </div>
                ) : (
                  chatHistory.map((msg) => (
                    <div
                      key={msg.id}
                      className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}
                    >
                      <div
                        className={`max-w-[80%] p-3 rounded-2xl ${
                          msg.role === 'user'
                            ? 'bg-purple-600 text-white'
                            : 'bg-dark-700 text-stone-200'
                        }`}
                      >
                        <div className="text-sm whitespace-pre-wrap">{msg.content}</div>
                      </div>
                    </div>
                  ))
                )}
                {chatLoading && (
                  <div className="flex justify-start">
                    <div className="bg-dark-700 p-3 rounded-2xl">
                      <RefreshCw className="w-4 h-4 animate-spin text-purple-400" />
                    </div>
                  </div>
                )}
                <div ref={chatEndRef} />
              </div>
              
              {/* Chat Input */}
              <div className="p-4 border-t border-dark-600">
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={chatInput}
                    onChange={(e) => setChatInput(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && sendMessage()}
                    placeholder="Ask NEURAX anything..."
                    className="flex-1 bg-dark-900 border border-dark-600 rounded-xl px-4 py-2 focus:outline-none focus:border-purple-500 transition-all"
                  />
                  <button
                    onClick={sendMessage}
                    disabled={!chatInput.trim() || chatLoading}
                    className="px-4 py-2 bg-gradient-to-r from-purple-600 to-blue-600 rounded-xl hover:from-purple-500 hover:to-blue-500 transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <Send className="w-4 h-4" />
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Permissions Tab */}
          {activeTab === 'permissions' && (
            <div className="bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-6">
              <h3 className="text-lg font-semibold mb-2 flex items-center gap-2">
                <Shield className="w-5 h-5 text-purple-400" />
                NEURAX Permissions
              </h3>
              <p className="text-sm text-stone-500 mb-6">
                Control what NEURAX can access and modify on your system. 
                Some features require administrator privileges.
              </p>
              
              <div className="space-y-4">
                {[
                  {
                    key: 'basic_analysis' as keyof NeuraxPermissions,
                    name: 'Basic Analysis',
                    description: 'Read-only monitoring of CPU, memory, GPU, and system metrics',
                    icon: Eye,
                    required: true,
                  },
                  {
                    key: 'process_management' as keyof NeuraxPermissions,
                    name: 'Process Management',
                    description: 'Adjust process priorities to optimize node and miner performance',
                    icon: Cpu,
                    admin: true,
                  },
                  {
                    key: 'power_settings' as keyof NeuraxPermissions,
                    name: 'Power Settings',
                    description: 'Switch power plans for better performance or efficiency',
                    icon: Power,
                    admin: true,
                  },
                  {
                    key: 'network_tuning' as keyof NeuraxPermissions,
                    name: 'Network Tuning',
                    description: 'Optimize firewall rules and network settings',
                    icon: Network,
                    admin: true,
                  },
                  {
                    key: 'memory_optimization' as keyof NeuraxPermissions,
                    name: 'Memory Optimization',
                    description: 'Clear system caches and optimize memory allocation',
                    icon: MemoryStick,
                    admin: true,
                  },
                  {
                    key: 'p2p_intelligence' as keyof NeuraxPermissions,
                    name: 'P2P Intelligence',
                    description: 'Share anonymized optimization insights with the network',
                    icon: Wifi,
                  },
                  {
                    key: 'auto_apply' as keyof NeuraxPermissions,
                    name: 'Auto-Apply Recommendations',
                    description: 'Automatically apply safe optimizations without asking',
                    icon: Zap,
                  },
                ].map(({ key, name, description, icon: Icon, required, admin }) => (
                  <div 
                    key={key}
                    className="flex items-center justify-between p-4 bg-dark-900/50 rounded-xl"
                  >
                    <div className="flex items-center gap-4">
                      <div className="p-2 bg-dark-800 rounded-lg">
                        <Icon className="w-5 h-5 text-purple-400" />
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="font-medium">{name}</span>
                          {required && (
                            <span className="px-2 py-0.5 bg-blue-500/20 text-blue-400 rounded text-xs">
                              Required
                            </span>
                          )}
                          {admin && (
                            <span className="px-2 py-0.5 bg-yellow-500/20 text-yellow-400 rounded text-xs">
                              Admin
                            </span>
                          )}
                        </div>
                        <p className="text-sm text-stone-500">{description}</p>
                      </div>
                    </div>
                    <label className="relative inline-flex items-center cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config?.permissions[key] ?? false}
                        onChange={(e) => !required && updatePermissions(key, e.target.checked)}
                        disabled={required}
                        className="sr-only peer"
                      />
                      <div className="w-11 h-6 bg-dark-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-purple-600 peer-disabled:opacity-50"></div>
                    </label>
                  </div>
                ))}
              </div>
              
              <div className="mt-6 p-4 bg-yellow-500/10 border border-yellow-500/30 rounded-xl">
                <div className="flex items-start gap-3">
                  <AlertTriangle className="w-5 h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
                  <div>
                    <h4 className="font-medium text-yellow-400">Administrator Privileges</h4>
                    <p className="text-sm text-stone-400 mt-1">
                      Some features require administrator privileges to function. 
                      You may be prompted by Windows UAC when enabling these features.
                    </p>
                  </div>
                </div>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
