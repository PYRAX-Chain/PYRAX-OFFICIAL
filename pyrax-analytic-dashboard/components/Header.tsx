import { Activity } from "lucide-react";

interface HeaderProps {
  isOperational: boolean;
  nodeCount: number;
  version: string;
}

export function Header({ isOperational, nodeCount, version }: HeaderProps) {
  return (
    <header className="border-b border-border/50 bg-gradient-to-b from-background to-background/50 backdrop-blur-sm sticky top-0 z-50">
      <div className="max-w-7xl mx-auto px-4 md:px-8 py-4 md:py-6">
        {/* Main Header */}
        <div className="flex flex-col md:flex-row justify-between items-start md:items-center gap-4 mb-4">
          {/* Logo and Title */}
          <div className="flex items-center gap-3 group">
            <div className="relative">
              <img 
                src="/pyrax-logo.svg" 
                alt="PYRAX Logo" 
                className="w-12 h-12 md:w-14 md:h-14 transition-transform group-hover:scale-110"
              />
              <div className="absolute inset-0 bg-orange-500/20 rounded-full blur-lg group-hover:bg-orange-500/40 transition-all"></div>
            </div>
            <div>
              <h1 className="text-2xl md:text-3xl font-bold pyrax-gradient-text">
                PYRAX
              </h1>
              <p className="text-xs md:text-sm text-muted-foreground">
                Network Analytics
              </p>
            </div>
          </div>

          {/* Status Indicator */}
          <div className="flex items-center gap-4">
            <div className={`flex items-center gap-2 px-3 py-2 rounded-lg border ${isOperational ? 'bg-green-500/10 border-green-500/30' : 'bg-red-500/10 border-red-500/30'}`}>
              <span className="relative flex h-2 w-2">
                <span className={`animate-ping absolute inline-flex h-full w-full rounded-full opacity-75 ${isOperational ? 'bg-green-500' : 'bg-red-500'}`}></span>
                <span className={`relative inline-flex rounded-full h-2 w-2 ${isOperational ? 'bg-green-500' : 'bg-red-500'}`}></span>
              </span>
              <span className={`text-sm font-medium ${isOperational ? 'text-green-400' : 'text-red-400'}`}>
                {isOperational ? 'Operational' : 'Stalled'}
              </span>
            </div>

            <div className="hidden md:flex gap-2 text-xs font-mono text-muted-foreground bg-secondary/30 px-3 py-2 rounded-lg border border-border/30">
              <span className="text-orange-400">v{version}</span>
              <span className="text-border/50">•</span>
              <span>{nodeCount} nodes</span>
            </div>
          </div>
        </div>

        {/* Stats Bar */}
        <div className="flex md:hidden gap-2 text-xs font-mono text-muted-foreground bg-secondary/30 px-3 py-2 rounded-lg border border-border/30 w-full justify-center">
          <span className="text-orange-400">v{version}</span>
          <span className="text-border/50">•</span>
          <span>{nodeCount} nodes</span>
        </div>
      </div>
    </header>
  );
}
