import { useEffect, useRef, useState } from 'react'

const BASE_URL = 'http://localhost:7070'
const WS_URL = 'ws://localhost:7070/ws'

// ─── Types ────────────────────────────────────────────────────────────────────

export interface Span {
  span_id: string
  trace_id: string
  parent_span_id?: string
  name: string
  agent_id: string
  status: 'unset' | 'ok' | 'error'
  start_time: string
  end_time?: string
  latency_ms?: number
  input?: string
  output?: string
  token_count?: number
  cost_usd?: number
  attributes: Record<string, unknown>
}

export interface Episode {
  id: string
  session_id: string
  timestamp: string
  role: 'user' | 'assistant' | 'tool'
  content: string
  metadata: Record<string, unknown>
}

export interface SpanListResponse {
  spans: Span[]
  total: number
}

export interface ReplayStep {
  step: number
  span: Span
  total_steps: number
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

async function fetchJson<T>(url: string, options?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  })
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${res.statusText}`)
  }
  return res.json() as Promise<T>
}

// ─── Traces ───────────────────────────────────────────────────────────────────

export async function getTraces(
  limit = 100,
  offset = 0,
): Promise<SpanListResponse> {
  return fetchJson<SpanListResponse>(
    `${BASE_URL}/v1/traces?limit=${limit}&offset=${offset}`,
  )
}

export async function ingestTraces(spans: Span[]): Promise<void> {
  await fetchJson<void>(`${BASE_URL}/v1/traces`, {
    method: 'POST',
    body: JSON.stringify(spans),
  })
}

export async function getTraceById(traceId: string): Promise<Span[]> {
  // Fetch all spans and filter by trace_id client-side
  const data = await fetchJson<SpanListResponse>(
    `${BASE_URL}/v1/traces?limit=1000&offset=0`,
  )
  return data.spans.filter((s) => s.trace_id === traceId)
}

export async function replaySession(
  sessionId: string,
  step: number,
): Promise<ReplayStep> {
  return fetchJson<ReplayStep>(
    `${BASE_URL}/v1/sessions/${sessionId}/replay?step=${step}`,
  )
}

// ─── Memory / Episodes ────────────────────────────────────────────────────────

export async function getEpisodes(sessionId: string): Promise<Episode[]> {
  return fetchJson<Episode[]>(
    `${BASE_URL}/v1/memory/episodes/${sessionId}`,
  )
}

export async function addEpisode(episode: Omit<Episode, 'id'>): Promise<Episode> {
  return fetchJson<Episode>(`${BASE_URL}/v1/memory/episodes`, {
    method: 'POST',
    body: JSON.stringify(episode),
  })
}

export async function searchMemory(query: string): Promise<Episode[]> {
  return fetchJson<Episode[]>(
    `${BASE_URL}/v1/memory/search?q=${encodeURIComponent(query)}`,
  )
}

// ─── Health ───────────────────────────────────────────────────────────────────

export async function checkHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${BASE_URL}/health`)
    return res.ok
  } catch {
    return false
  }
}

// ─── WebSocket Hook ───────────────────────────────────────────────────────────

export type WsStatus = 'connecting' | 'connected' | 'disconnected' | 'error'

export interface UseWebSocketReturn {
  status: WsStatus
  lastSpan: Span | null
}

export function useWebSocket(onSpan: (span: Span) => void): UseWebSocketReturn {
  const [status, setStatus] = useState<WsStatus>('disconnected')
  const [lastSpan, setLastSpan] = useState<Span | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const retryRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const onSpanRef = useRef(onSpan)

  useEffect(() => {
    onSpanRef.current = onSpan
  }, [onSpan])

  useEffect(() => {
    let isMounted = true

    function connect() {
      if (!isMounted) return
      setStatus('connecting')

      try {
        const ws = new WebSocket(WS_URL)
        wsRef.current = ws

        ws.onopen = () => {
          if (isMounted) setStatus('connected')
        }

        ws.onmessage = (event: MessageEvent) => {
          try {
            const data = JSON.parse(event.data as string) as Span
            if (isMounted) {
              setLastSpan(data)
              onSpanRef.current(data)
            }
          } catch {
            // ignore malformed messages
          }
        }

        ws.onerror = () => {
          if (isMounted) setStatus('error')
        }

        ws.onclose = () => {
          if (isMounted) {
            setStatus('disconnected')
            // Reconnect after 3 seconds
            retryRef.current = setTimeout(connect, 3000)
          }
        }
      } catch {
        if (isMounted) {
          setStatus('error')
          retryRef.current = setTimeout(connect, 5000)
        }
      }
    }

    connect()

    return () => {
      isMounted = false
      if (retryRef.current) clearTimeout(retryRef.current)
      if (wsRef.current) {
        wsRef.current.onclose = null
        wsRef.current.close()
      }
    }
  }, [])

  return { status, lastSpan }
}
