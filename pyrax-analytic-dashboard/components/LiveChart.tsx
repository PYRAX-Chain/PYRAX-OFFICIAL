"use client";

import { useState, useEffect } from "react";
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from "recharts";
import { ChainStatus } from "@/lib/types";

interface LiveChartProps {
    title: string;
    dataKey: string;
    color: string;
    value: number;
    label?: string; // e.g. "ms" or "blocks"
    formatValue?: (val: number) => string;
}

export function LiveChart({ title, dataKey, color, value, label, formatValue }: LiveChartProps) {
    const [data, setData] = useState<{ time: string; value: number }[]>([]);

    useEffect(() => {
        const now = new Date();
        const timeStr = now.toLocaleTimeString([], { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });

        setData(prev => {
            const newData = [...prev, { time: timeStr, value }];
            // Keep last 20 points (approx 2-3 mins if polling every 5s)
            if (newData.length > 20) newData.shift();
            return newData;
        });
    }, [value]);

    return (
        <div className="bg-card border border-border rounded-xl p-6 flex flex-col h-[300px]">
            <h3 className="font-semibold mb-4 text-sm uppercase tracking-wide text-muted-foreground">{title}</h3>
            <div className="flex-1 w-full min-h-0">
                <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={data}>
                        <defs>
                            <linearGradient id={`color${dataKey}`} x1="0" y1="0" x2="0" y2="1">
                                <stop offset="5%" stopColor={color} stopOpacity={0.3} />
                                <stop offset="95%" stopColor={color} stopOpacity={0} />
                            </linearGradient>
                        </defs>
                        <CartesianGrid strokeDasharray="3 3" stroke="#333" vertical={false} />
                        <XAxis
                            dataKey="time"
                            stroke="#666"
                            fontSize={10}
                            tickLine={false}
                            axisLine={false}
                            interval={4}
                        />
                        <YAxis
                            stroke="#666"
                            fontSize={10}
                            tickLine={false}
                            axisLine={false}
                            domain={['auto', 'auto']}
                            tickFormatter={formatValue}
                        />
                        <Tooltip
                            contentStyle={{ backgroundColor: '#18181b', borderColor: '#27272a', color: '#fff' }}
                            itemStyle={{ color: '#fff' }}
                            labelStyle={{ display: 'none' }}
                        />
                        <Area
                            type="monotone"
                            dataKey="value"
                            stroke={color}
                            fillOpacity={1}
                            fill={`url(#color${dataKey})`}
                            isAnimationActive={false}
                        />
                    </AreaChart>
                </ResponsiveContainer>
            </div>
        </div>
    );
}
