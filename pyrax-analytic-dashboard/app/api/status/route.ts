import { NextResponse } from 'next/server';

export const dynamic = 'force-dynamic'; // Prevent caching

export async function GET() {
    // Point to your PYRAX Metrics service
    const VPS_URL = process.env.VPS_API_URL || 'http://localhost:8080';

    try {
        const res = await fetch(`${VPS_URL}/status`, {
            cache: 'no-store',
            headers: {
                'Cache-Control': 'no-cache'
            }
        });

        if (!res.ok) {
            throw new Error(`Upstream error: ${res.status}`);
        }

        const data = await res.json();
        return NextResponse.json(data);
    } catch (error) {
        console.error('Proxy Error:', error);
        return NextResponse.json(
            { error: 'Failed to fetch status from PYRAX Metrics API' },
            { status: 502 }
        );
    }
}

