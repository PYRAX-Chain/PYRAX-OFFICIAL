import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/tauri';
import { 
  Brain, AlertTriangle, CheckCircle, ChevronRight, 
  RefreshCw, Zap, Network, Cpu, Info, EyeOff
} from 'lucide-react';
import { useLogStore } from '../stores/logStore';
import { useNodeStore } from '../stores/nodeStore';

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

interface QuickInsights {
  system_score: number;
  network_score: number;
  mining_score: number;
  top_insights: NeuraxInsight[];
  error_count: number;
  warning_count: number;
}

interface NeuraxConfig {
  enabled: boolean;
}

export default function NeuraxInsightsPanel() {
  const [config, setConfig] = useState<NeuraxConfig | null>(null);
  const [insights, setInsights] = useState<QuickInsights | null>(null);
  const [loading, setLoading] = useState(true);
  const { logs } = useLogStore();
  const { status } = useNodeStore();

  useEffect(() => {
    loadConfig();
  }, []);

  useEffect(() => {
    if (config?.enabled) {
      loadInsights();
      const interval = setInterval(loadInsights, 30000); // Refresh every 30s
      return () => clearInterval(interval);
    }
  }, [config?.enabled, logs]);

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

  const loadInsights = async () => {
    try {
      const logMessages = logs.slice(0, 100).map(l => `[${l.level}] ${l.message}`);
      const nodeStatus = status ? {
        peerCount: status.peerCount,
        connected: status.connected,
        running: status.running,
      } : null;
      
      const data = await invoke<QuickInsights>('neurax_get_quick_insights', { 
        logs: logMessages,
        nodeStatus 
      });
      setInsights(data);
    } catch (e) {
      console.error('Failed to load insights:', e);
    }
  };

  const dismissInsight = async (id: string) => {
    try {
      await invoke('neurax_dismiss_insight', { insightId: id });
      if (insights) {
        setInsights({
          ...insights,
          top_insights: insights.top_insights.filter(i => i.id !== id)
        });
      }
    } catch (e) {
      console.error('Failed to dismiss insight:', e);
    }
  };

  const getScoreColor = (score: number) => {
    if (score >= 80) return 'text-green-400';
    if (score >= 60) return 'text-yellow-400';
    return 'text-red-400';
  };

  const getScoreBg = (score: number) => {
    if (score >= 80) return 'bg-green-500';
    if (score >= 60) return 'bg-yellow-500';
    return 'bg-red-500';
  };

  const getSeverityColor = (severity: string) => {
    switch (severity) {
      case 'critical': return 'border-red-500/30 bg-red-500/5';
      case 'warning': return 'border-yellow-500/30 bg-yellow-500/5';
      default: return 'border-blue-500/30 bg-blue-500/5';
    }
  };

  const getSeverityIcon = (severity: string) => {
    switch (severity) {
      case 'critical': return <AlertTriangle className="w-4 h-4 text-red-400" />;
      case 'warning': return <AlertTriangle className="w-4 h-4 text-yellow-400" />;
      default: return <Info className="w-4 h-4 text-blue-400" />;
    }
  };

  if (loading) {
    return (
      <div className="bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-4">
        <div className="flex items-center justify-center py-8">
          <RefreshCw className="w-5 h-5 animate-spin text-purple-400" />
        </div>
      </div>
    );
  }

  // Not enabled state
  if (!config?.enabled) {
    return (
      <div className="bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-4">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <Brain className="w-5 h-5 text-purple-400" />
            <h3 className="font-semibold">NEURAX AI Insights</h3>
          </div>
        </div>
        <div className="text-center py-6">
          <Brain className="w-10 h-10 mx-auto text-purple-500 opacity-30 mb-2" />
          <p className="text-sm text-stone-500 mb-3">Enable NEURAX for AI-powered insights</p>
          <Link
            to="/neurax"
            className="inline-flex items-center gap-1 px-3 py-1.5 bg-purple-600 hover:bg-purple-500 rounded-lg text-sm font-medium transition-all"
          >
            Enable NEURAX
            <ChevronRight className="w-4 h-4" />
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div className="bg-dark-800/50 backdrop-blur rounded-2xl border border-dark-600 p-4">
      {/* Header */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <div className="p-1.5 bg-gradient-to-br from-purple-600 to-blue-600 rounded-lg">
            <Brain className="w-4 h-4 text-white" />
          </div>
          <h3 className="font-semibold">NEURAX Insights</h3>
        </div>
        <Link
          to="/neurax"
          className="text-xs text-purple-400 hover:text-purple-300 flex items-center gap-1"
        >
          Open NEURAX
          <ChevronRight className="w-3 h-3" />
        </Link>
      </div>

      {/* Health Scores */}
      <div className="grid grid-cols-3 gap-2 mb-4">
        {[
          { name: 'System', score: insights?.system_score ?? 0, icon: Cpu },
          { name: 'Network', score: insights?.network_score ?? 0, icon: Network },
          { name: 'Mining', score: insights?.mining_score ?? 0, icon: Zap },
        ].map(({ name, score, icon: Icon }) => (
          <div key={name} className="bg-dark-900/50 rounded-xl p-2 text-center">
            <Icon className={`w-4 h-4 mx-auto ${getScoreColor(score)}`} />
            <div className={`text-lg font-bold ${getScoreColor(score)}`}>{score}</div>
            <div className="text-[10px] text-stone-500">{name}</div>
            <div className="h-1 bg-dark-700 rounded-full mt-1 overflow-hidden">
              <div 
                className={`h-full rounded-full ${getScoreBg(score)}`}
                style={{ width: `${score}%` }}
              />
            </div>
          </div>
        ))}
      </div>

      {/* Top Insights */}
      <div className="space-y-2">
        {!insights?.top_insights?.length ? (
          <div className="text-center py-4">
            <CheckCircle className="w-8 h-8 mx-auto text-green-500 opacity-50 mb-1" />
            <p className="text-xs text-stone-500">All systems optimal</p>
          </div>
        ) : (
          insights.top_insights.slice(0, 3).map((insight) => (
            <div 
              key={insight.id}
              className={`p-2.5 rounded-xl border ${getSeverityColor(insight.severity)}`}
            >
              <div className="flex items-start gap-2">
                {getSeverityIcon(insight.severity)}
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium truncate">{insight.title}</div>
                  <p className="text-xs text-stone-500 line-clamp-2 mt-0.5">
                    {insight.description}
                  </p>
                </div>
                <button
                  onClick={() => dismissInsight(insight.id)}
                  className="p-1 hover:bg-dark-700 rounded transition-all flex-shrink-0"
                  title="Dismiss"
                >
                  <EyeOff className="w-3 h-3 text-stone-500" />
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      {/* Error/Warning Count */}
      {(insights?.error_count ?? 0) > 0 || (insights?.warning_count ?? 0) > 0 ? (
        <div className="flex items-center justify-between mt-3 pt-3 border-t border-dark-600 text-xs">
          <span className="text-stone-500">Recent logs:</span>
          <div className="flex items-center gap-3">
            {(insights?.error_count ?? 0) > 0 && (
              <span className="text-red-400">{insights?.error_count} errors</span>
            )}
            {(insights?.warning_count ?? 0) > 0 && (
              <span className="text-yellow-400">{insights?.warning_count} warnings</span>
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}
