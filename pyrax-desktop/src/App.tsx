import { Routes, Route } from 'react-router-dom';
import { useEffect, lazy, Suspense } from 'react';
import Layout from './components/Layout';
import Dashboard from './pages/Dashboard';
import { ErrorBoundary } from './components/ErrorBoundary';
import ToastContainer from './components/ToastContainer';
import UpdateNotification from './components/UpdateNotification';
import { useNodeStore } from './stores/nodeStore';

// Lazy load non-critical pages for faster initial load
const Wallet = lazy(() => import('./pages/Wallet'));
const Mining = lazy(() => import('./pages/Mining'));
const MiningDashboard = lazy(() => import('./pages/MiningDashboard'));
const Explorer = lazy(() => import('./pages/Explorer'));
const Settings = lazy(() => import('./pages/Settings'));
const Network = lazy(() => import('./pages/Network'));
const Neurax = lazy(() => import('./pages/Neurax'));

// Loading fallback for lazy routes
const PageLoader = () => (
  <div className="flex items-center justify-center h-full">
    <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-purple-500"></div>
  </div>
);

export default function App() {
  const { fetchStatus } = useNodeStore();

  useEffect(() => {
    // PERFORMANCE FIX: Increased polling interval from 3s to 5s to reduce CPU/memory overhead
    // This reduces IPC calls and state updates that can cause UI freezing
    const safeF = async () => {
      try {
        await fetchStatus();
      } catch (e) {
        console.error('Status fetch error:', e);
      }
    };
    safeF();
    const interval = setInterval(safeF, 5000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  return (
    <>
      <ErrorBoundary>
        <Routes>
          <Route path="/" element={<Layout />}>
            <Route index element={<Dashboard />} />
            <Route path="wallet" element={<Suspense fallback={<PageLoader />}><Wallet /></Suspense>} />
            <Route path="mining" element={<Suspense fallback={<PageLoader />}><Mining /></Suspense>} />
            <Route path="mining-dashboard" element={<Suspense fallback={<PageLoader />}><MiningDashboard /></Suspense>} />
            <Route path="explorer" element={<Suspense fallback={<PageLoader />}><Explorer /></Suspense>} />
            <Route path="settings" element={<Suspense fallback={<PageLoader />}><Settings /></Suspense>} />
            <Route path="network" element={<Suspense fallback={<PageLoader />}><Network /></Suspense>} />
            <Route path="neurax" element={<Suspense fallback={<PageLoader />}><Neurax /></Suspense>} />
          </Route>
        </Routes>
      </ErrorBoundary>
      <ToastContainer />
      <UpdateNotification />
    </>
  );
}
