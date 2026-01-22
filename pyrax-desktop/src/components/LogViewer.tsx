import { useEffect, useRef, useState, useCallback } from 'react';
import { writeText } from '@tauri-apps/api/clipboard';
import { 
  Terminal, 
  Trash2, 
  Filter, 
  Pause, 
  Play,
  ChevronDown,
  Copy,
  Check,
  ArrowDown
} from 'lucide-react';
import { useLogStore, LogEntry } from '../stores/logStore';

const levelColors: Record<LogEntry['level'], string> = {
  info: 'text-green-400',
  warn: 'text-yellow-500',
  error: 'text-red-500',
  debug: 'text-gray-500',
};

const levelSymbols: Record<LogEntry['level'], string> = {
  info: '●',
  warn: '▲',
  error: '✖',
  debug: '○',
};

const categoryColors: Record<LogEntry['category'], string> = {
  node: 'text-purple-400',
  block: 'text-green-400',
  p2p: 'text-cyan-400',
  rpc: 'text-blue-400',
  mining: 'text-yellow-400',
  staking: 'text-pink-400',
  system: 'text-gray-400',
};

function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
  const time = date.toLocaleTimeString('en-US', { 
    hour12: false, 
    hour: '2-digit', 
    minute: '2-digit', 
    second: '2-digit',
  });
  const ms = String(date.getMilliseconds()).padStart(3, '0');
  return `${time}.${ms}`;
}

export default function LogViewer() {
  const { logs, filters, clearLogs, setFilters } = useLogStore();
  const [paused, setPaused] = useState(false);
  const [showFilters, setShowFilters] = useState(false);
  const [copied, setCopied] = useState(false);
  const logContainerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [userScrolledAway, setUserScrolledAway] = useState(false);
  const isUserScrollingRef = useRef(false);
  const scrollTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // PERFORMANCE FIX: Cleanup scroll timeout on unmount to prevent memory leaks
  useEffect(() => {
    return () => {
      if (scrollTimeoutRef.current) {
        clearTimeout(scrollTimeoutRef.current);
      }
    };
  }, []);

  // Handle scroll events to detect user scrolling away from bottom
  // PERFORMANCE FIX: Throttled scroll handler to reduce overhead
  const handleScroll = useCallback(() => {
    if (!logContainerRef.current) return;
    
    // Throttle: Skip if we recently handled a scroll
    if (scrollTimeoutRef.current) return;
    
    const container = logContainerRef.current;
    const { scrollTop, scrollHeight, clientHeight } = container;
    
    // Check if user is at or near the bottom (within 50px)
    const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
    
    // If user scrolled away from bottom, pause auto-scroll
    if (!isAtBottom && !isUserScrollingRef.current) {
      isUserScrollingRef.current = true;
      setUserScrolledAway(true);
      setAutoScroll(false);
    }
    
    // If user scrolled back to bottom, resume auto-scroll
    if (isAtBottom && isUserScrollingRef.current) {
      isUserScrollingRef.current = false;
      setUserScrolledAway(false);
      setAutoScroll(true);
    }
    
    // Throttle scroll handling to once per 100ms
    scrollTimeoutRef.current = setTimeout(() => {
      scrollTimeoutRef.current = null;
    }, 100);
  }, []);

  // Resume auto-scroll and scroll to bottom
  const resumeAutoScroll = useCallback(() => {
    setAutoScroll(true);
    setUserScrolledAway(false);
    isUserScrollingRef.current = false;
    
    if (logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, []);

  // Auto-scroll to bottom when new logs arrive (only if auto-scroll is enabled)
  useEffect(() => {
    if (autoScroll && !paused && !userScrolledAway && logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, [logs, autoScroll, paused, userScrolledAway]);

  // PERFORMANCE FIX: Limit displayed logs to prevent rendering overhead
  const MAX_DISPLAY_LOGS = 50;
  const filteredLogs = logs.filter((log) => 
    filters.level.includes(log.level) && 
    filters.category.includes(log.category)
  );

  // Only render the most recent logs to prevent UI freeze
  const displayLogs = paused ? [] : filteredLogs.slice(0, MAX_DISPLAY_LOGS);

  const handleCopyLogs = async () => {
    const text = filteredLogs
      .map((log) => `[${formatTime(log.timestamp)}] [${log.level.toUpperCase()}] [${log.category}] ${log.message}`)
      .join('\n');
    
    try {
      // CLIPBOARD FIX: Use Tauri clipboard API instead of navigator.clipboard
      // navigator.clipboard doesn't work reliably in Tauri apps
      await writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      console.error('Failed to copy logs:', e);
      // Fallback to navigator.clipboard if Tauri API fails
      try {
        await navigator.clipboard.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      } catch (e2) {
        console.error('Fallback copy also failed:', e2);
      }
    }
  };

  const toggleLevel = (level: string) => {
    const newLevels = filters.level.includes(level)
      ? filters.level.filter((l) => l !== level)
      : [...filters.level, level];
    setFilters({ level: newLevels });
  };

  const toggleCategory = (category: string) => {
    const newCategories = filters.category.includes(category)
      ? filters.category.filter((c) => c !== category)
      : [...filters.category, category];
    setFilters({ category: newCategories });
  };

  return (
    <div className="bg-dark-900/80 backdrop-blur rounded-2xl border border-dark-600/50 overflow-hidden flex flex-col h-[350px] shadow-2xl">
      {/* Terminal Header - Modern glass style */}
      <div className="flex items-center justify-between px-4 py-2.5 bg-dark-800/80 border-b border-dark-600/50">
        <div className="flex items-center gap-3">
          {/* Traffic light buttons */}
          <div className="flex items-center gap-1.5">
            <div className="w-3 h-3 rounded-full bg-red-500 hover:bg-red-400 cursor-pointer" />
            <div className="w-3 h-3 rounded-full bg-yellow-500 hover:bg-yellow-400 cursor-pointer" />
            <div className="w-3 h-3 rounded-full bg-green-500 hover:bg-green-400 cursor-pointer" />
          </div>
          <div className="flex items-center gap-2 ml-2">
            <Terminal size={14} className="text-green-500" />
            <span className="font-mono text-sm text-gray-300">pyrax-node</span>
            <span className="text-xs text-gray-600 font-mono">— {filteredLogs.length} lines</span>
          </div>
        </div>
        
        <div className="flex items-center gap-1">
          <button
            onClick={() => setPaused(!paused)}
            className={`p-1.5 rounded hover:bg-gray-700 transition-colors ${paused ? 'text-yellow-400' : 'text-gray-400'}`}
            title={paused ? 'Resume' : 'Pause'}
          >
            {paused ? <Play size={14} /> : <Pause size={14} />}
          </button>
          
          <button
            onClick={() => setShowFilters(!showFilters)}
            className={`p-1.5 rounded hover:bg-gray-700 transition-colors ${showFilters ? 'text-purple-400' : 'text-gray-400'}`}
            title="Filters"
          >
            <Filter size={14} />
          </button>
          
          <button
            onClick={handleCopyLogs}
            className="p-1.5 rounded hover:bg-gray-700 transition-colors text-gray-400"
            title="Copy logs"
          >
            {copied ? <Check size={14} className="text-green-400" /> : <Copy size={14} />}
          </button>
          
          <button
            onClick={clearLogs}
            className="p-1.5 rounded hover:bg-gray-700 transition-colors text-gray-400 hover:text-red-400"
            title="Clear logs"
          >
            <Trash2 size={14} />
          </button>
          
          {/* Resume auto-scroll button - shown when user has scrolled away */}
          {userScrolledAway && !paused && (
            <button
              onClick={resumeAutoScroll}
              className="ml-2 px-2 py-1 rounded bg-cyan-600 hover:bg-cyan-500 transition-colors text-white text-xs flex items-center gap-1 animate-pulse"
              title="Resume auto-scroll"
            >
              <ArrowDown size={12} />
              <span>Resume</span>
            </button>
          )}
        </div>
      </div>

      {/* Filters Panel */}
      {showFilters && (
        <div className="px-4 py-2 bg-dark-800/50 border-b border-dark-600/50 flex flex-wrap gap-4 text-xs font-mono">
          <div className="flex items-center gap-2">
            <span className="text-gray-600">level:</span>
            {(['info', 'warn', 'error', 'debug'] as const).map((level) => (
              <button
                key={level}
                onClick={() => toggleLevel(level)}
                className={`px-2 py-0.5 rounded border transition-colors ${
                  filters.level.includes(level) 
                    ? levelColors[level] + ' border-current bg-current/10' 
                    : 'text-gray-600 border-gray-700 hover:border-gray-600'
                }`}
              >
                {levelSymbols[level]} {level}
              </button>
            ))}
          </div>
          
          <div className="flex items-center gap-2">
            <span className="text-gray-600">category:</span>
            {(['node', 'block', 'p2p', 'rpc', 'mining', 'staking'] as const).map((cat) => (
              <button
                key={cat}
                onClick={() => toggleCategory(cat)}
                className={`px-2 py-0.5 rounded border transition-colors ${
                  filters.category.includes(cat) 
                    ? categoryColors[cat] + ' border-current bg-current/10'
                    : 'text-gray-600 border-gray-700 hover:border-gray-600'
                }`}
              >
                {cat}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Log Content - Clean terminal style */}
      <div 
        ref={logContainerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto font-mono text-[11px] leading-relaxed bg-dark-900/50"
      >
        {paused ? (
          <div className="flex items-center justify-center h-full text-yellow-500 font-mono">
            <Pause size={16} className="mr-2" />
            <span className="animate-pulse">█</span> Logging paused
          </div>
        ) : displayLogs.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-gray-600 font-mono">
            <Terminal size={24} className="mb-2 opacity-50" />
            <span className="text-green-500/50">$</span> Waiting for output...
            <span className="animate-pulse text-green-500 mt-1">▌</span>
          </div>
        ) : (
          <div className="p-2">
            {displayLogs.map((log, index) => (
              <div 
                key={log.id} 
                className="flex items-start gap-1 py-[2px] hover:bg-pyrax-500/5 group"
              >
                {/* Line number */}
                <span className="text-gray-700 w-8 text-right flex-shrink-0 select-none group-hover:text-gray-600">
                  {String(index + 1).padStart(3, ' ')}
                </span>
                
                {/* Separator */}
                <span className="text-gray-800 flex-shrink-0">│</span>
                
                {/* Timestamp */}
                <span className="text-gray-600 flex-shrink-0 w-[85px]">
                  {formatTime(log.timestamp)}
                </span>
                
                {/* Level indicator */}
                <span className={`flex-shrink-0 w-4 ${levelColors[log.level]}`}>
                  {levelSymbols[log.level]}
                </span>
                
                {/* Category */}
                <span className={`flex-shrink-0 w-[52px] ${categoryColors[log.category]}`}>
                  [{log.category}]
                </span>
                
                {/* Message */}
                <span className={`break-all ${
                  log.level === 'error' ? 'text-red-400' : 
                  log.level === 'warn' ? 'text-yellow-400' : 
                  'text-gray-300'
                }`}>
                  {log.message}
                </span>
              </div>
            ))}
            
            {/* Cursor line */}
            <div className="flex items-center gap-1 py-[2px] text-green-500">
              <span className="text-gray-700 w-8 text-right flex-shrink-0">
                {String(displayLogs.length + 1).padStart(3, ' ')}
              </span>
              <span className="text-gray-800">│</span>
              <span className="animate-pulse">▌</span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
