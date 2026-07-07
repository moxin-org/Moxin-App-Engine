import React from 'react'
import { clsx } from 'clsx'
import type { WsStatus } from '../lib/api'

interface LiveIndicatorProps {
  status: WsStatus
}

const statusConfig: Record<WsStatus, { color: string; label: string; pulse: boolean }> = {
  connected: { color: 'bg-emerald-400', label: 'Live', pulse: true },
  connecting: { color: 'bg-yellow-400', label: 'Connecting', pulse: false },
  disconnected: { color: 'bg-gray-500', label: 'Offline', pulse: false },
  error: { color: 'bg-red-400', label: 'Error', pulse: false },
}

export function LiveIndicator({ status }: LiveIndicatorProps) {
  const config = statusConfig[status]

  return (
    <div className="flex items-center gap-2 rounded-full border border-gray-700 bg-gray-800/80 px-3 py-1.5 text-xs font-medium text-gray-300">
      <span className="relative flex h-2.5 w-2.5">
        {config.pulse && (
          <span
            className={clsx(
              'absolute inline-flex h-full w-full rounded-full opacity-75',
              config.color,
              'animate-ping',
            )}
          />
        )}
        <span
          className={clsx(
            'relative inline-flex h-2.5 w-2.5 rounded-full',
            config.color,
            config.pulse && 'pulse-dot',
          )}
        />
      </span>
      {config.label}
    </div>
  )
}
