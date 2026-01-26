"use client";

import { useQuery } from "@tanstack/react-query";
import { fetchStatus } from "@/lib/api";
import { StatCard } from "@/components/ui/StatCard";
import { NodeList } from "@/components/NodeList";
import { HealthRing } from "@/components/ui/HealthRing";
import { NodeMap } from "@/components/NodeMap";
import { AlertBanner } from "@/components/AlertBanner";
import { LiveChart } from "@/components/LiveChart";
import { Header } from "@/components/Header";
import { Footer } from "@/components/Footer";
import { cn } from "@/lib/utils";
import { Activity, Blocks, Globe, Zap, AlertTriangle, Layers, Pickaxe } from "lucide-react";

export default function Dashboard() {
    const { data: status, isLoading, isError } = useQuery({
        queryKey: ["status"],
        queryFn: fetchStatus,
        refetchInterval: 5000, // Explicitly set refetch interval
    });

    if (isLoading) {
        return (
            <div className="min-h-screen flex items-center justify-center">
                <div className="animate-pulse flex flex-col items-center gap-4">
                    <div className="w-12 h-12 bg-gradient-to-br from-orange-400 to-orange-500 rounded-full blur-sm" />
                    <div className="h-4 w-32 bg-secondary rounded" />
                </div>
            </div>
        );
    }

    if (isError || !status) {
        return (
            <div className="min-h-screen flex items-center justify-center p-4">
                <div className="text-center space-y-4">
                    <AlertTriangle className="w-16 h-16 text-orange-500 mx-auto animate-pulse" />
                    <h1 className="text-2xl font-bold text-foreground">Connection Failed</h1>
                    <p className="text-muted-foreground max-w-md">Could not connect to PYRAX Metrics API. Please ensure the metrics service is running on port 8080.</p>
                </div>
            </div>
        );
    }

    const { chain, nodes, alerts } = status;
    const activePercentage = nodes.length > 0 ? (chain.online_nodes / nodes.length) * 100 : 0;
    const isProducing = chain.block_rate > 0.1;

    return (
        <div className="min-h-screen flex flex-col bg-background">
            {/* Header */}
            <Header 
                isOperational={!alerts.stalled} 
                nodeCount={nodes.length}
                version="0.1.0"
            />

            {/* Main Content */}
            <main className="flex-1 p-4 md:p-8">
                <div className="max-w-7xl mx-auto space-y-8 animate-slide-up">

                    {/* Critical Alerts */}
                    <AlertBanner
                        stalled={alerts.stalled}
                        forkDetected={alerts.fork_detected}
                        heightDelta={chain.height_delta}
                    />

                    {/* Main Stats Grid */}
                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                        <StatCard
                            label="Block Height"
                            value={chain.head_block.toLocaleString()}
                            icon={Blocks}
                            trend="Real-time"
                            className="md:col-span-1"
                        />

                        <StatCard
                            label="Avg Latency"
                            value={`${chain.avg_latency_ms}ms`}
                            icon={Zap}
                            status={chain.avg_latency_ms < 200 ? "success" : chain.avg_latency_ms > 1000 ? "danger" : "warning"}
                        />

                        <StatCard
                            label="Discovered Nodes"
                            value={chain.discovered_nodes}
                            icon={Globe}
                            status="default"
                        />

                        <StatCard
                            label="Active Nodes"
                            value={chain.online_nodes}
                            icon={Activity}
                            status="success"
                        />
                    </div>

                    {/* Performance Charts */}
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                        <LiveChart
                            title="Block Height Trend"
                            dataKey="height"
                            value={chain.head_block}
                            color="#8b5cf6" // violet-500
                            formatValue={(v) => v.toLocaleString()}
                        />
                        <LiveChart
                            title="Network Latency (ms)"
                            dataKey="latency"
                            value={chain.avg_latency_ms}
                            color={chain.avg_latency_ms > 500 ? "#ef4444" : "#22c55e"}
                            label="ms"
                        />
                    </div>

                    {/* Node Map Section */}
                    <NodeMap nodes={nodes} />

                    <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
                        {/* Main Content - Node List */}
                        <div className="lg:col-span-2 space-y-6">
                            <NodeList nodes={nodes} />
                        </div>

                        {/* Sidebar - Health Indicators */}
                        <div className="space-y-6">
                            <div className="bg-card border border-border rounded-xl p-6 flex flex-col items-center justify-center text-center">
                                <h3 className="font-semibold mb-6 flex items-center gap-2 w-full justify-start">
                                    <Activity className="w-4 h-4 text-primary" />
                                    Network Health
                                </h3>
                                <HealthRing
                                    percentage={activePercentage}
                                    label="Uptime"
                                    subLabel={`${chain.online_nodes} of ${nodes.length} nodes active`}
                                />
                            </div>

                            <div className="bg-card border border-border rounded-xl p-6">
                                <h3 className="font-semibold mb-4 flex items-center gap-2">
                                    <Pickaxe className="w-4 h-4 text-primary" />
                                    Block Production
                                </h3>
                                <div className="space-y-6">
                                    <div>
                                        <div className="flex justify-between text-sm mb-2">
                                            <span className="text-muted-foreground block text-xs uppercase tracking-wide">Status</span>
                                            <span className={cn(
                                                "font-medium px-2 py-0.5 rounded text-xs",
                                                isProducing ? "bg-success/10 text-success" : "bg-danger/10 text-danger"
                                            )}>
                                                {isProducing ? "Producing Blocks" : "Stopped"}
                                            </span>
                                        </div>

                                        <div className="flex justify-between text-sm mb-2 mt-4">
                                            <span className="text-muted-foreground block text-xs uppercase tracking-wide">Block Rate</span>
                                            <span className="font-mono text-xs">{chain.block_rate.toFixed(2)} / min</span>
                                        </div>
                                        <div className="h-2 bg-secondary rounded-full overflow-hidden">
                                            <div className="h-full bg-primary transition-all duration-500" style={{ width: `${Math.min(chain.block_rate * 10, 100)}%` }} />
                                        </div>
                                    </div>

                                    <div className="pt-2 border-t border-border/50">
                                        <div className="flex justify-between items-center mb-2">
                                            <span className="text-muted-foreground text-xs uppercase tracking-wide">Sync Status</span>
                                            <span className={chain.height_delta > 2 ? "text-warning" : "text-success"}>
                                                {chain.height_delta} blocks delta
                                            </span>
                                        </div>
                                        {chain.height_delta > 0 && (
                                            <div className="text-xs text-muted-foreground">
                                                Highest: #{chain.head_block} <br /> Lowest: #{chain.lowest_block}
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>

            </main>

            {/* Footer */}
            <Footer />
        </div>
    );
}
