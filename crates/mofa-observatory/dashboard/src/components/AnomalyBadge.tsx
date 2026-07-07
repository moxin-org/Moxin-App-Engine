import React from 'react'
import { AlertTriangle } from 'lucide-react'
import { Badge } from './ui/badge'

interface AnomalyBadgeProps {
  latencyMs: number
  threshold?: number
}

export function AnomalyBadge({ latencyMs, threshold = 2000 }: AnomalyBadgeProps) {
  if (latencyMs <= threshold) return null

  return (
    <Badge variant="anomaly" className="ml-2 gap-1">
      <AlertTriangle className="h-3 w-3" />
      SLOW {latencyMs}ms
    </Badge>
  )
}
