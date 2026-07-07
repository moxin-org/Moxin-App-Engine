import React from 'react'
import {
  Radar,
  RadarChart,
  PolarGrid,
  PolarAngleAxis,
  PolarRadiusAxis,
  ResponsiveContainer,
  Tooltip,
} from 'recharts'

export interface RadarCriterion {
  criterion: string
  score: number
  fullMark: number
}

interface RubricRadarProps {
  data: RadarCriterion[]
  title?: string
}

interface TooltipPayload {
  name: string
  value: number
}

interface CustomTooltipProps {
  active?: boolean
  payload?: TooltipPayload[]
  label?: string
}

function CustomTooltip({ active, payload, label }: CustomTooltipProps) {
  if (!active || !payload?.length) return null
  return (
    <div className="rounded border border-gray-600 bg-gray-800 px-3 py-2 text-xs shadow-lg">
      <p className="font-semibold text-gray-200">{label}</p>
      {payload.map((entry, i) => (
        <p key={i} className="text-emerald-400">
          Score: {entry.value}
        </p>
      ))}
    </div>
  )
}

export function RubricRadar({ data, title }: RubricRadarProps) {
  return (
    <div className="flex flex-col items-center">
      {title && (
        <p className="mb-2 text-sm font-medium text-gray-400">{title}</p>
      )}
      <ResponsiveContainer width="100%" height={300}>
        <RadarChart data={data} margin={{ top: 10, right: 30, bottom: 10, left: 30 }}>
          <PolarGrid stroke="#374151" />
          <PolarAngleAxis
            dataKey="criterion"
            tick={{ fill: '#9ca3af', fontSize: 12 }}
          />
          <PolarRadiusAxis
            angle={30}
            domain={[0, 100]}
            tick={{ fill: '#6b7280', fontSize: 10 }}
            tickCount={5}
          />
          <Radar
            name="Score"
            dataKey="score"
            stroke="#10b981"
            fill="#10b981"
            fillOpacity={0.25}
            strokeWidth={2}
          />
          <Tooltip content={<CustomTooltip />} />
        </RadarChart>
      </ResponsiveContainer>
    </div>
  )
}
