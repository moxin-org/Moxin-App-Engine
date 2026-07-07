import React, { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { RefreshCw, ChevronLeft, ChevronRight } from 'lucide-react'
import { getTraces, useWebSocket, type Span } from '../lib/api'
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card'
import { Badge } from '../components/ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '../components/ui/table'
import { Button } from '../components/ui/button'
import { AnomalyBadge } from '../components/AnomalyBadge'
import { LiveIndicator } from '../components/LiveIndicator'

const PAGE_SIZE = 100

function formatTimestamp(iso: string): string {
  try {
    return new Date(iso).toLocaleString()
  } catch {
    return iso
  }
}

function formatCost(cost?: number): string {
  if (cost === undefined || cost === null) return '—'
  return `$${cost.toFixed(4)}`
}

export function TraceList() {
  const navigate = useNavigate()
  const [spans, setSpans] = useState<Span[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(0)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadSpans = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const data = await getTraces(PAGE_SIZE, page * PAGE_SIZE)
      setSpans(data.spans ?? [])
      setTotal(data.total ?? 0)
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setError(msg)
      // Use mock data so the UI is still useful when backend is down
      setSpans(MOCK_SPANS)
      setTotal(MOCK_SPANS.length)
    } finally {
      setLoading(false)
    }
  }, [page])

  useEffect(() => {
    void loadSpans()
  }, [loadSpans])

  const handleNewSpan = useCallback((span: Span) => {
    setSpans((prev) => [span, ...prev.slice(0, PAGE_SIZE - 1)])
  }, [])

  const { status: wsStatus } = useWebSocket(handleNewSpan)

  const totalPages = Math.ceil(total / PAGE_SIZE)

  return (
    <div className="space-y-6">
      {/* Header row */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-100">Traces</h1>
          <p className="mt-1 text-sm text-gray-400">
            {total} span{total !== 1 ? 's' : ''} total
          </p>
        </div>
        <div className="flex items-center gap-3">
          <LiveIndicator status={wsStatus} />
          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadSpans()}
            disabled={loading}
          >
            <RefreshCw className={`mr-2 h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      {/* Error banner */}
      {error && (
        <div className="rounded-lg border border-yellow-700/50 bg-yellow-900/20 px-4 py-3 text-sm text-yellow-300">
          Backend unavailable — showing mock data. ({error})
        </div>
      )}

      {/* Table */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle>Span List</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Timestamp</TableHead>
                <TableHead>Agent</TableHead>
                <TableHead>Name</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Latency</TableHead>
                <TableHead>Tokens</TableHead>
                <TableHead>Cost</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {spans.length === 0 && !loading && (
                <TableRow>
                  <TableCell colSpan={7} className="text-center text-gray-500 py-12">
                    No spans found
                  </TableCell>
                </TableRow>
              )}
              {spans.map((span) => (
                <TableRow
                  key={span.span_id}
                  className="cursor-pointer"
                  onClick={() => navigate(`/traces/${span.trace_id}`)}
                >
                  <TableCell className="text-gray-400 text-xs">
                    {formatTimestamp(span.start_time)}
                  </TableCell>
                  <TableCell>
                    <span className="rounded bg-gray-700/60 px-1.5 py-0.5 text-xs font-mono text-gray-300">
                      {span.agent_id}
                    </span>
                  </TableCell>
                  <TableCell className="font-medium text-gray-200 max-w-xs truncate">
                    {span.name}
                  </TableCell>
                  <TableCell>
                    <Badge
                      variant={
                        span.status === 'ok'
                          ? 'ok'
                          : span.status === 'error'
                          ? 'error'
                          : 'unset'
                      }
                    >
                      {span.status}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-gray-300">
                    {span.latency_ms !== undefined ? (
                      <span className="flex items-center gap-1">
                        {span.latency_ms}ms
                        <AnomalyBadge latencyMs={span.latency_ms} />
                      </span>
                    ) : (
                      <span className="text-gray-600">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-gray-400">
                    {span.token_count ?? '—'}
                  </TableCell>
                  <TableCell className="text-gray-400">
                    {formatCost(span.cost_usd)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-between text-sm text-gray-400">
          <span>
            Page {page + 1} of {totalPages}
          </span>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={page === 0}
              onClick={() => setPage((p) => p - 1)}
            >
              <ChevronLeft className="h-4 w-4" />
              Prev
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={page >= totalPages - 1}
              onClick={() => setPage((p) => p + 1)}
            >
              Next
              <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

// ─── Mock data for when backend is unavailable ────────────────────────────────

const MOCK_SPANS: Span[] = [
  {
    span_id: 'span-001',
    trace_id: 'trace-abc',
    name: 'agent.plan',
    agent_id: 'planner-v2',
    status: 'ok',
    start_time: new Date(Date.now() - 5000).toISOString(),
    end_time: new Date(Date.now() - 4200).toISOString(),
    latency_ms: 800,
    token_count: 320,
    cost_usd: 0.00048,
    attributes: {},
  },
  {
    span_id: 'span-002',
    trace_id: 'trace-abc',
    parent_span_id: 'span-001',
    name: 'llm.call',
    agent_id: 'planner-v2',
    status: 'ok',
    start_time: new Date(Date.now() - 4200).toISOString(),
    end_time: new Date(Date.now() - 1800).toISOString(),
    latency_ms: 2400,
    token_count: 1500,
    cost_usd: 0.00225,
    input: 'What is the capital of France?',
    output: 'The capital of France is Paris.',
    attributes: { model: 'gpt-4o' },
  },
  {
    span_id: 'span-003',
    trace_id: 'trace-def',
    name: 'tool.search',
    agent_id: 'researcher-v1',
    status: 'error',
    start_time: new Date(Date.now() - 12000).toISOString(),
    latency_ms: 450,
    token_count: 0,
    attributes: { error: 'timeout' },
  },
  {
    span_id: 'span-004',
    trace_id: 'trace-ghi',
    name: 'agent.summarize',
    agent_id: 'summarizer-v3',
    status: 'ok',
    start_time: new Date(Date.now() - 30000).toISOString(),
    end_time: new Date(Date.now() - 27000).toISOString(),
    latency_ms: 3000,
    token_count: 2100,
    cost_usd: 0.00315,
    input: 'Summarize the following text...',
    output: 'The text discusses...',
    attributes: {},
  },
]
