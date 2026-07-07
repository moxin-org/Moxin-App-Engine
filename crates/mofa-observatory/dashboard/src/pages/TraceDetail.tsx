import React, { useCallback, useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { ArrowLeft, ChevronLeft, ChevronRight, Clock, RefreshCw } from 'lucide-react'
import { getTraceById, replaySession, type Span, type ReplayStep } from '../lib/api'
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card'
import { Button } from '../components/ui/button'
import { Badge } from '../components/ui/badge'
import { SpanTree } from '../components/SpanTree'

function StatCard({
  label,
  value,
}: {
  label: string
  value: string | number | undefined
}) {
  return (
    <div className="rounded-lg border border-gray-700 bg-gray-800/50 p-4">
      <p className="text-xs text-gray-500 uppercase tracking-wider">{label}</p>
      <p className="mt-1 text-lg font-semibold text-gray-100">
        {value ?? '—'}
      </p>
    </div>
  )
}

export function TraceDetail() {
  const { traceId } = useParams<{ traceId: string }>()
  const navigate = useNavigate()

  const [spans, setSpans] = useState<Span[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Time-travel replay state
  const [replayStep, setReplayStep] = useState(0)
  const [replayData, setReplayData] = useState<ReplayStep | null>(null)
  const [replayLoading, setReplayLoading] = useState(false)
  const [replayError, setReplayError] = useState<string | null>(null)

  const loadSpans = useCallback(async () => {
    if (!traceId) return
    setLoading(true)
    setError(null)
    try {
      const data = await getTraceById(traceId)
      setSpans(data)
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setError(msg)
      // Fallback to mock spans for this trace
      setSpans(MOCK_DETAIL_SPANS)
    } finally {
      setLoading(false)
    }
  }, [traceId])

  useEffect(() => {
    void loadSpans()
  }, [loadSpans])

  const handleReplay = useCallback(
    async (step: number) => {
      if (!traceId) return
      setReplayLoading(true)
      setReplayError(null)
      try {
        const data = await replaySession(traceId, step)
        setReplayData(data)
        setReplayStep(step)
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setReplayError(msg)
        // Mock replay response
        setReplayData({
          step,
          span: spans[step % spans.length] ?? spans[0],
          total_steps: spans.length,
        })
        setReplayStep(step)
      } finally {
        setReplayLoading(false)
      }
    },
    [traceId, spans],
  )

  // Aggregate stats
  const totalTokens = spans.reduce((acc, s) => acc + (s.token_count ?? 0), 0)
  const totalCost = spans.reduce((acc, s) => acc + (s.cost_usd ?? 0), 0)
  const totalLatency = spans.reduce((acc, s) => acc + (s.latency_ms ?? 0), 0)
  const agents = [...new Set(spans.map((s) => s.agent_id))]
  const hasError = spans.some((s) => s.status === 'error')

  const replayTotalSteps = replayData?.total_steps ?? spans.length

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <Button variant="ghost" size="sm" onClick={() => navigate('/traces')}>
          <ArrowLeft className="mr-2 h-4 w-4" />
          Back
        </Button>
        <div className="flex-1">
          <h1 className="text-2xl font-bold text-gray-100">Trace Detail</h1>
          <p className="mt-0.5 font-mono text-sm text-gray-400">{traceId}</p>
        </div>
        <div className="flex items-center gap-2">
          {hasError && <Badge variant="error">Has Errors</Badge>}
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

      {/* Stats */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <StatCard label="Spans" value={spans.length} />
        <StatCard label="Total Tokens" value={totalTokens || undefined} />
        <StatCard
          label="Total Cost"
          value={totalCost ? `$${totalCost.toFixed(5)}` : undefined}
        />
        <StatCard
          label="Total Latency"
          value={totalLatency ? `${totalLatency}ms` : undefined}
        />
      </div>

      {/* Agents involved */}
      {agents.length > 0 && (
        <div className="flex flex-wrap gap-2">
          <span className="text-xs text-gray-500 self-center">Agents:</span>
          {agents.map((a) => (
            <Badge key={a} variant="info">
              {a}
            </Badge>
          ))}
        </div>
      )}

      {/* Span tree */}
      <Card>
        <CardHeader>
          <CardTitle>Span Tree</CardTitle>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="flex items-center justify-center py-12 text-gray-500">
              <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
              Loading spans…
            </div>
          ) : spans.length === 0 ? (
            <div className="py-12 text-center text-gray-500">No spans found for this trace</div>
          ) : (
            <SpanTree spans={spans} />
          )}
        </CardContent>
      </Card>

      {/* Time Travel */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Clock className="h-4 w-4 text-blue-400" />
            Time Travel
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="mb-4 text-sm text-gray-400">
            Step through the trace execution history to inspect each span in order.
          </p>

          {/* Step controls */}
          <div className="mb-4 flex items-center gap-3">
            <Button
              variant="outline"
              size="sm"
              disabled={replayStep <= 0 || replayLoading}
              onClick={() => void handleReplay(replayStep - 1)}
            >
              <ChevronLeft className="mr-1 h-4 w-4" />
              Prev
            </Button>
            <span className="text-sm text-gray-400">
              Step{' '}
              <span className="font-semibold text-gray-200">{replayStep + 1}</span>
              {' '}of{' '}
              <span className="font-semibold text-gray-200">
                {replayTotalSteps || spans.length}
              </span>
            </span>
            <Button
              variant="outline"
              size="sm"
              disabled={
                replayLoading ||
                replayStep >= (replayTotalSteps || spans.length) - 1
              }
              onClick={() => void handleReplay(replayStep + 1)}
            >
              Next
              <ChevronRight className="ml-1 h-4 w-4" />
            </Button>
            {spans.length > 0 && !replayData && (
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void handleReplay(0)}
                disabled={replayLoading}
              >
                Start Replay
              </Button>
            )}
          </div>

          {replayError && (
            <div className="mb-3 rounded border border-yellow-700/50 bg-yellow-900/20 px-3 py-2 text-xs text-yellow-300">
              Replay endpoint unavailable — using local span data. ({replayError})
            </div>
          )}

          {/* Replay span display */}
          {replayData && (
            <div className="rounded-lg border border-gray-700 bg-gray-900/60 p-4 font-mono text-xs">
              <div className="mb-2 flex items-center justify-between">
                <span className="font-semibold text-gray-200">
                  {replayData.span.name}
                </span>
                <Badge
                  variant={
                    replayData.span.status === 'ok'
                      ? 'ok'
                      : replayData.span.status === 'error'
                      ? 'error'
                      : 'unset'
                  }
                >
                  {replayData.span.status}
                </Badge>
              </div>
              <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-[11px] text-gray-400">
                <span>Agent: <span className="text-gray-300">{replayData.span.agent_id}</span></span>
                <span>Span ID: <span className="text-gray-300">{replayData.span.span_id}</span></span>
                {replayData.span.latency_ms !== undefined && (
                  <span>Latency: <span className="text-gray-300">{replayData.span.latency_ms}ms</span></span>
                )}
                {replayData.span.token_count !== undefined && (
                  <span>Tokens: <span className="text-gray-300">{replayData.span.token_count}</span></span>
                )}
              </div>
              {replayData.span.input && (
                <div className="mt-3">
                  <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-blue-400">Input</div>
                  <pre className="whitespace-pre-wrap break-words text-gray-300 leading-relaxed max-h-32 overflow-y-auto scrollbar-thin">
                    {replayData.span.input}
                  </pre>
                </div>
              )}
              {replayData.span.output && (
                <div className="mt-3">
                  <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-emerald-400">Output</div>
                  <pre className="whitespace-pre-wrap break-words text-gray-300 leading-relaxed max-h-32 overflow-y-auto scrollbar-thin">
                    {replayData.span.output}
                  </pre>
                </div>
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

// ─── Mock data ─────────────────────────────────────────────────────────────────

const MOCK_DETAIL_SPANS: Span[] = [
  {
    span_id: 'root-001',
    trace_id: 'mock-trace',
    name: 'agent.run',
    agent_id: 'orchestrator',
    status: 'ok',
    start_time: new Date(Date.now() - 6000).toISOString(),
    end_time: new Date(Date.now() - 100).toISOString(),
    latency_ms: 5900,
    token_count: 4200,
    cost_usd: 0.0063,
    attributes: {},
  },
  {
    span_id: 'child-001',
    trace_id: 'mock-trace',
    parent_span_id: 'root-001',
    name: 'llm.call',
    agent_id: 'orchestrator',
    status: 'ok',
    start_time: new Date(Date.now() - 5800).toISOString(),
    end_time: new Date(Date.now() - 3200).toISOString(),
    latency_ms: 2600,
    token_count: 1800,
    cost_usd: 0.0027,
    input: 'Plan the following task: research and summarize AI trends in 2024.',
    output: '1. Search for recent AI papers\n2. Identify key themes\n3. Write summary',
    attributes: { model: 'gpt-4o', temperature: 0.7 },
  },
  {
    span_id: 'child-002',
    trace_id: 'mock-trace',
    parent_span_id: 'root-001',
    name: 'tool.web_search',
    agent_id: 'researcher',
    status: 'ok',
    start_time: new Date(Date.now() - 3100).toISOString(),
    end_time: new Date(Date.now() - 2500).toISOString(),
    latency_ms: 600,
    token_count: 0,
    attributes: { query: 'AI trends 2024', results_count: 10 },
  },
  {
    span_id: 'child-003',
    trace_id: 'mock-trace',
    parent_span_id: 'root-001',
    name: 'llm.summarize',
    agent_id: 'summarizer',
    status: 'ok',
    start_time: new Date(Date.now() - 2400).toISOString(),
    end_time: new Date(Date.now() - 200).toISOString(),
    latency_ms: 2200,
    token_count: 2400,
    cost_usd: 0.0036,
    input: 'Summarize: [web search results...]',
    output: 'Key AI trends in 2024 include: multimodal models, reasoning improvements, agentic systems...',
    attributes: { model: 'gpt-4o-mini' },
  },
]
