import React, { useState } from 'react'
import {
  BrowserRouter,
  Routes,
  Route,
  NavLink,
  Navigate,
} from 'react-router-dom'
import {
  Activity,
  BarChart3,
  Brain,
  GitBranch,
  Menu,
  X,
  Telescope,
} from 'lucide-react'
import { clsx } from 'clsx'
import { TraceList } from './pages/TraceList'
import { TraceDetail } from './pages/TraceDetail'
import { EvalResults } from './pages/EvalResults'
import { MemoryExplorer } from './pages/MemoryExplorer'

// ─── Nav items ─────────────────────────────────────────────────────────────────

interface NavItem {
  to: string
  label: string
  icon: React.ComponentType<{ className?: string }>
  end?: boolean
}

const NAV_ITEMS: NavItem[] = [
  { to: '/traces', label: 'Traces', icon: Activity },
  { to: '/evaluations', label: 'Evaluations', icon: BarChart3 },
  { to: '/memory', label: 'Memory', icon: Brain },
]

// ─── Sidebar ───────────────────────────────────────────────────────────────────

function Sidebar({ mobile, onClose }: { mobile?: boolean; onClose?: () => void }) {
  return (
    <aside
      className={clsx(
        'flex h-full flex-col border-r border-gray-700/60 bg-gray-900',
        mobile ? 'w-64' : 'w-56',
      )}
    >
      {/* Logo */}
      <div className="flex h-16 items-center gap-3 border-b border-gray-700/60 px-5">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-blue-600/20 border border-blue-500/30">
          <Telescope className="h-4 w-4 text-blue-400" />
        </div>
        <div>
          <p className="text-sm font-bold text-gray-100">MOFA</p>
          <p className="text-xs text-gray-500">Observatory</p>
        </div>
        {mobile && (
          <button
            className="ml-auto text-gray-500 hover:text-gray-300"
            onClick={onClose}
          >
            <X className="h-5 w-5" />
          </button>
        )}
      </div>

      {/* Nav */}
      <nav className="flex-1 space-y-1 p-3">
        {NAV_ITEMS.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            onClick={onClose}
            className={({ isActive }) =>
              clsx(
                'flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors',
                isActive
                  ? 'bg-blue-600/20 text-blue-300 border border-blue-600/30'
                  : 'text-gray-400 hover:bg-gray-700/40 hover:text-gray-200',
              )
            }
          >
            <Icon className="h-4 w-4 flex-shrink-0" />
            {label}
          </NavLink>
        ))}
      </nav>

      {/* Footer */}
      <div className="border-t border-gray-700/60 p-4">
        <div className="flex items-center gap-2 text-xs text-gray-600">
          <GitBranch className="h-3.5 w-3.5" />
          <span>feat/mofa-observatory</span>
        </div>
      </div>
    </aside>
  )
}

// ─── Layout ────────────────────────────────────────────────────────────────────

function Layout({ children }: { children: React.ReactNode }) {
  const [mobileOpen, setMobileOpen] = useState(false)

  return (
    <div className="flex h-screen overflow-hidden bg-gray-900">
      {/* Desktop sidebar */}
      <div className="hidden md:flex">
        <Sidebar />
      </div>

      {/* Mobile drawer backdrop */}
      {mobileOpen && (
        <div
          className="fixed inset-0 z-40 bg-black/60 md:hidden"
          onClick={() => setMobileOpen(false)}
        />
      )}

      {/* Mobile drawer */}
      <div
        className={clsx(
          'fixed inset-y-0 left-0 z-50 transform transition-transform duration-200 ease-in-out md:hidden',
          mobileOpen ? 'translate-x-0' : '-translate-x-full',
        )}
      >
        <Sidebar mobile onClose={() => setMobileOpen(false)} />
      </div>

      {/* Main content */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Top bar (mobile) */}
        <header className="flex h-16 items-center gap-4 border-b border-gray-700/60 bg-gray-900 px-4 md:hidden">
          <button
            className="text-gray-400 hover:text-gray-200"
            onClick={() => setMobileOpen(true)}
          >
            <Menu className="h-6 w-6" />
          </button>
          <div className="flex items-center gap-2">
            <Telescope className="h-5 w-5 text-blue-400" />
            <span className="font-bold text-gray-100">MOFA Observatory</span>
          </div>
        </header>

        {/* Page content */}
        <main className="flex-1 overflow-y-auto scrollbar-thin p-6">
          <div className="mx-auto max-w-7xl">{children}</div>
        </main>
      </div>
    </div>
  )
}

// ─── App ───────────────────────────────────────────────────────────────────────

export default function App() {
  return (
    <BrowserRouter>
      <Layout>
        <Routes>
          <Route path="/" element={<Navigate to="/traces" replace />} />
          <Route path="/traces" element={<TraceList />} />
          <Route path="/traces/:traceId" element={<TraceDetail />} />
          <Route path="/evaluations" element={<EvalResults />} />
          <Route path="/memory" element={<MemoryExplorer />} />
          {/* Catch-all */}
          <Route path="*" element={<Navigate to="/traces" replace />} />
        </Routes>
      </Layout>
    </BrowserRouter>
  )
}
