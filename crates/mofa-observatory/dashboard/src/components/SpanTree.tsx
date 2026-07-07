import React, { useState } from 'react'
import { ChevronRight, ChevronDown, Activity } from 'lucide-react'
import { clsx } from 'clsx'
import type { Span } from '../lib/api'
import { Badge } from './ui/badge'

interface SpanNodeProps {
  span: Span
  children: Span[]
  allSpans: Span[]
  depth: number
}

function getChildren(span: Span, allSpans: Span[]): Span[] {
  return allSpans.filter((s) => s.parent_span_id === span.span_id)
}

function SpanNode({ span, allSpans, depth }: SpanNodeProps) {
  const [expanded, setExpanded] = useState(depth < 2)
  const [showIO, setShowIO] = useState(false)
  const children = getChildren(span, allSpans)
  const hasChildren = children.length > 0

  const statusVariant =
    span.status === 'ok' ? 'ok' : span.status === 'error' ? 'error' : 'unset'

  return (
    <div className="font-mono text-xs">
      <div
        className={clsx(
          'flex items-center gap-2 rounded px-2 py-1.5 transition-colors hover:bg-gray-700/40 cursor-pointer select-none',
          depth === 0 && 'border-l-2 border-blue-500',
        )}
        style={{ paddingLeft: `${8 + depth * 20}px` }}
        onClick={() => hasChildren && setExpanded((e) => !e)}
      >
        {/* Expand toggle */}
        <span className="w-4 flex-shrink-0 text-gray-500">
          {hasChildren ? (
            expanded ? (
              <ChevronDown className="h-3.5 w-3.5" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" />
            )
          ) : (
            <Activity className="h-3 w-3 text-gray-600" />
          )}
        </span>

        {/* Span name */}
        <span className="flex-1 truncate font-medium text-gray-200">
          {span.name}
        </span>

        {/* Agent */}
        <span className="text-gray-500 text-[10px] shrink-0">{span.agent_id}</span>

        {/* Status */}
        <Badge variant={statusVariant}>{span.status}</Badge>

        {/* Latency */}
        {span.latency_ms !== undefined && (
          <span
            className={clsx(
              'shrink-0 rounded px-1.5 py-0.5 text-[10px]',
              span.latency_ms > 2000
                ? 'bg-red-900/60 text-red-300'
                : 'bg-gray-700/60 text-gray-400',
            )}
          >
            {span.latency_ms}ms
          </span>
        )}

        {/* Tokens */}
        {span.token_count !== undefined && (
          <span className="shrink-0 text-[10px] text-gray-500">
            {span.token_count}tok
          </span>
        )}

        {/* I/O toggle */}
        {(span.input ?? span.output) && (
          <button
            className="shrink-0 rounded border border-gray-600 px-1.5 py-0.5 text-[10px] text-gray-400 hover:text-gray-200 hover:border-gray-400 transition-colors"
            onClick={(e) => {
              e.stopPropagation()
              setShowIO((v) => !v)
            }}
          >
            {showIO ? 'Hide I/O' : 'I/O'}
          </button>
        )}
      </div>

      {/* Input / Output panel */}
      {showIO && (span.input ?? span.output) && (
        <div
          className="mx-2 mb-2 rounded border border-gray-700 bg-gray-900/80 p-3 text-xs"
          style={{ marginLeft: `${16 + depth * 20}px` }}
        >
          {span.input && (
            <div className="mb-3">
              <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-blue-400">
                Input
              </div>
              <pre className="whitespace-pre-wrap break-words text-gray-300 leading-relaxed">
                {span.input}
              </pre>
            </div>
          )}
          {span.output && (
            <div>
              <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-emerald-400">
                Output
              </div>
              <pre className="whitespace-pre-wrap break-words text-gray-300 leading-relaxed">
                {span.output}
              </pre>
            </div>
          )}
          {span.cost_usd !== undefined && (
            <div className="mt-2 text-[10px] text-gray-500">
              Cost: ${span.cost_usd.toFixed(6)}
            </div>
          )}
        </div>
      )}

      {/* Children */}
      {expanded && hasChildren && (
        <div>
          {children.map((child) => (
            <SpanNode
              key={child.span_id}
              span={child}
              children={getChildren(child, allSpans)}
              allSpans={allSpans}
              depth={depth + 1}
            />
          ))}
        </div>
      )}
    </div>
  )
}

interface SpanTreeProps {
  spans: Span[]
}

export function SpanTree({ spans }: SpanTreeProps) {
  // Find root spans (no parent, or parent not in set)
  const spanIds = new Set(spans.map((s) => s.span_id))
  const roots = spans.filter(
    (s) => !s.parent_span_id || !spanIds.has(s.parent_span_id),
  )

  if (roots.length === 0 && spans.length > 0) {
    // Fallback: treat all spans as roots
    return (
      <div className="space-y-0.5">
        {spans.map((span) => (
          <SpanNode
            key={span.span_id}
            span={span}
            children={[]}
            allSpans={spans}
            depth={0}
          />
        ))}
      </div>
    )
  }

  return (
    <div className="space-y-0.5">
      {roots.map((span) => (
        <SpanNode
          key={span.span_id}
          span={span}
          children={getChildren(span, spans)}
          allSpans={spans}
          depth={0}
        />
      ))}
    </div>
  )
}
