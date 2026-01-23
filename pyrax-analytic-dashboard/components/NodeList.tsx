import { NodeInfo } from "@/lib/types";
import { cn } from "@/lib/utils";
import { Server, Wifi, WifiOff } from "lucide-react";

interface NodeListProps {
    nodes: NodeInfo[];
}

export function NodeList({ nodes }: NodeListProps) {
    if (nodes.length === 0) {
        return (
            <div className="text-center p-8 text-muted-foreground bg-card rounded-xl border border-border">
                No nodes configured
            </div>
        );
    }

    return (
        <div className="bg-card border border-border rounded-xl overlow-hidden">
            <div className="p-4 border-b border-border flex justify-between items-center">
                <h3 className="font-semibold flex items-center gap-2">
                    <Server className="w-4 h-4 text-primary" />
                    Network Nodes
                </h3>
                <span className="text-sm text-muted-foreground">{nodes.length} nodes</span>
            </div>

            <div className="overflow-x-auto">
                <table className="w-full text-sm text-left">
                    <thead className="bg-secondary/50 text-secondary-foreground uppercase text-xs font-medium">
                        <tr>
                            <th className="px-6 py-3">Endpoint</th>
                            <th className="px-6 py-3 text-right">Height</th>
                            <th className="px-6 py-3 text-center">Status</th>
                            <th className="px-6 py-3 text-right">Latency</th>
                        </tr>
                    </thead>
                    <tbody className="divide-y divide-border">
                        {nodes.map((node) => (
                            <tr key={node.endpoint} className="hover:bg-secondary/30 transition-colors">
                                <td className="px-6 py-4 font-mono truncate max-w-[200px]" title={node.endpoint}>
                                    {node.endpoint.replace('http://', '').replace('https://', '')}
                                </td>
                                <td className="px-6 py-4 text-right tabular-nums font-medium">
                                    #{(node.block_height || 0).toLocaleString()}
                                </td>
                                <td className="px-6 py-4 text-center">
                                    <div className={cn(
                                        "inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium border",
                                        node.reachable
                                            ? "bg-success/10 text-success border-success/20"
                                            : "bg-danger/10 text-danger border-danger/20"
                                    )}>
                                        {node.reachable ? <Wifi className="w-3 h-3" /> : <WifiOff className="w-3 h-3" />}
                                        {node.reachable ? "Online" : "Offline"}
                                    </div>
                                </td>
                                <td className={cn(
                                    "px-6 py-4 text-right tabular-nums",
                                    node.latency_ms > 1000 ? "text-warning" : "text-muted-foreground"
                                )}>
                                    {node.latency_ms}ms
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </div>
    );
}
