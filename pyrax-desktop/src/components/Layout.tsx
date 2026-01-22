import { Outlet, NavLink } from 'react-router-dom';
import { 
  LayoutDashboard, 
  Wallet, 
  Hammer, 
  Search, 
  Settings,
  Network,
  Circle,
  Loader2,
  Gauge,
  Brain
} from 'lucide-react';
import { useNodeStore } from '../stores/nodeStore';
import { cn } from '../lib/utils';

const navItems = [
  { to: '/', icon: LayoutDashboard, label: 'Dashboard' },
  { to: '/wallet', icon: Wallet, label: 'Wallet' },
  { to: '/mining', icon: Hammer, label: 'Mining' },
  { to: '/mining-dashboard', icon: Gauge, label: 'Mining Pro' },
  { to: '/explorer', icon: Search, label: 'Explorer' },
  { to: '/network', icon: Network, label: 'Network' },
  { to: '/neurax', icon: Brain, label: 'NEURAX AI' },
  { to: '/settings', icon: Settings, label: 'Settings' },
];

export default function Layout() {
  const { status } = useNodeStore();

  return (
    <div className="flex h-screen bg-dark-900 text-stone-100">
      {/* Sidebar */}
      <aside className="w-64 bg-dark-800 border-r border-dark-600 flex flex-col">
        {/* Logo */}
        <div className="p-6 border-b border-dark-600">
          <div className="flex flex-col items-center">
            <img src="/pyrax-logo.png" alt="Inferno Node" className="w-20 h-20 object-contain" />
            <h1 className="mt-3 text-lg font-semibold text-stone-200">Inferno Node</h1>
          </div>
        </div>

        {/* Navigation */}
        <nav className="flex-1 p-4 space-y-1">
          {navItems.map(({ to, icon: Icon, label }) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-3 px-3 py-2 rounded-lg transition-colors',
                  isActive
                    ? 'bg-pyrax-600 text-white'
                    : 'text-stone-400 hover:bg-dark-700 hover:text-white'
                )
              }
            >
              <Icon size={20} />
              <span>{label}</span>
            </NavLink>
          ))}
        </nav>

        {/* Node Status */}
        <div className="p-4 border-t border-dark-600">
          <div className="flex items-center gap-2 text-sm">
            {status?.running ? (
              status?.connected ? (
                <>
                  <Circle className="w-3 h-3 fill-green-500 text-green-500" />
                  <span className="text-green-400">Connected</span>
                </>
              ) : (
                <>
                  <Loader2 className="w-3 h-3 text-pyrax-500 animate-spin" />
                  <span className="text-pyrax-400">Connecting...</span>
                </>
              )
            ) : (
              <>
                <Circle className="w-3 h-3 fill-red-500 text-red-500" />
                <span className="text-red-400">Node Offline</span>
              </>
            )}
          </div>
          {status?.running && status?.connected && (
            <div className="mt-2 text-xs text-stone-500">
              <div>Block: #{status.blockHeight.toLocaleString()}</div>
              <div>Peers: {status.peerCount}</div>
              <div>Network: {status.network}</div>
            </div>
          )}
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-auto bg-dark-900">
        <Outlet />
      </main>
    </div>
  );
}
