import React, { useCallback, useEffect, useState } from 'react'
import { Search, User, Bot, Wrench, RefreshCw } from 'lucide-react'
import { getEpisodes, searchMemory, type Episode } from '../lib/api'
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card'
import { Badge } from '../components/ui/badge'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'

// ─── Mock sessions ─────────────────────────────────────────────────────────────

const MOCK_SESSIONS = ['session-alpha', 'session-beta', 'session-gamma']

const MOCK_EPISODES: Episode[] = [
  {
    id: 'ep-001',
    session_id: 'session-alpha',
    timestamp: new Date(Date.now() - 60_000).toISOString(),
    role: 'user',
    content: 'Hello! Can you help me understand MOFA agents?',
    metadata: {},
  },
  {
    id: 'ep-002',
    session_id: 'session-alpha',
    timestamp: new Date(Date.now() - 55_000).toISOString(),
    role: 'assistant',
    content:
      'Of course! MOFA (Multi-agent Orchestration Framework for AI) is a framework for building and coordinating multiple AI agents. Each agent can have specialized capabilities and communicate through structured message passing.',
    metadata: { model: 'gpt-4o', tokens: 68 },
  },
  {
    id: 'ep-003',
    session_id: 'session-alpha',
    timestamp: new Date(Date.now() - 50_000).toISOString(),
    role: 'tool',
    content: 'web_search("MOFA agent framework documentation")\n→ Found 8 results',
    metadata: { tool: 'web_search', latency_ms: 340 },
  },
  {
    id: 'ep-004',
    session_id: 'session-alpha',
    timestamp: new Date(Date.now() - 45_000).toISOString(),
    role: 'user',
    content: 'What are the main components of an agent in MOFA?',
    metadata: {},
  },
  {
    id: 'ep-005',
    session_id: 'session-alpha',
    timestamp: new Date(Date.now() - 40_000).toISOString(),
    role: 'assistant',
    content:
      'A MOFA agent typically consists of: (1) a perception module for processing inputs, (2) a memory system for storing context and knowledge, (3) a planning component for deciding next actions, and (4) an execution layer for carrying out tasks.',
    metadata: { model: 'gpt-4o', tokens: 72 },
  },
  {
    id: 'ep-006',
    session_id: 'session-beta',
    timestamp: new Date(Date.now() - 3600_000).toISOString(),
    role: 'user',
    content: 'Run the data analysis pipeline on dataset-v3.',
    metadata: {},
  },
  {
    id: 'ep-007',
    session_id: 'session-beta',
    timestamp: new Date(Date.now() - 3590_000).toISOString(),
    role: 'tool',
    content: 'data_loader("dataset-v3")\n→ Loaded 50,000 rows × 24 columns',
    metadata: { tool: 'data_loader', rows: 50000 },
  },
  {
    id: 'ep-008',
    session_id: 'session-beta',
    timestamp: new Date(Date.now() - 3580_000).toISOString(),
    role: 'assistant',
    content:
      'Dataset loaded successfully. Running statistical analysis... I found 3 anomalies in columns [price, volume, timestamp]. Shall I proceed with cleaning?',
    metadata: { model: 'claude-3-5-sonnet', tokens: 54 },
  },
]

// ─── Role config ───────────────────────────────────────────────────────────────

const roleConfig = {
  user: {
    icon: User,
    variant: 'user' as const,
    label: 'User',
    bgClass: 'border-blue-700/40 bg-blue-900/10',
  },
  assistant: {
    icon: Bot,
    variant: 'assistant' as const,
    label: 'Assistant',
    bgClass: 'border-emerald-700/40 bg-emerald-900/10',
  },
  tool: {
    icon: Wrench,
    variant: 'tool' as const,
    label: 'Tool',
    bgClass: 'border-orange-700/40 bg-orange-900/10',
  },
}

function formatTimestamp(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString()
  } catch {
    return iso
  }
}

function EpisodeCard({ episode }: { episode: Episode }) {
  const config = roleConfig[episode.role]
  const Icon = config.icon

  return (
    <div
      className={`rounded-lg border p-4 transition-colors ${config.bgClass}`}
    >
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <div className="flex h-7 w-7 items-center justify-center rounded-full bg-gray-700/60">
            <Icon className="h-3.5 w-3.5 text-gray-300" />
          </div>
          <Badge variant={config.variant}>{config.label}</Badge>
        </div>
        <span className="text-xs text-gray-500">{formatTimestamp(episode.timestamp)}</span>
      </div>

      <p className="whitespace-pre-wrap break-words font-mono text-sm leading-relaxed text-gray-200">
        {episode.content}
      </p>

      {Object.keys(episode.metadata).length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2">
          {Object.entries(episode.metadata).map(([k, v]) => (
            <span
              key={k}
              className="rounded bg-gray-700/50 px-1.5 py-0.5 text-[10px] text-gray-400"
            >
              {k}: {String(v)}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

export function MemoryExplorer() {
  const [selectedSession, setSelectedSession] = useState<string>(MOCK_SESSIONS[0])
  const [episodes, setEpisodes] = useState<Episode[]>([])
  const [searchQuery, setSearchQuery] = useState('')
  const [searchResults, setSearchResults] = useState<Episode[] | null>(null)
  const [loading, setLoading] = useState(false)
  const [searchLoading, setSearchLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadEpisodes = useCallback(async (sessionId: string) => {
    setLoading(true)
    setError(null)
    setSearchResults(null)
    try {
      const data = await getEpisodes(sessionId)
      setEpisodes(data)
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setError(msg)
      // Use mock data filtered by session
      setEpisodes(MOCK_EPISODES.filter((e) => e.session_id === sessionId))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void loadEpisodes(selectedSession)
  }, [selectedSession, loadEpisodes])

  const handleSearch = useCallback(async () => {
    if (!searchQuery.trim()) {
      setSearchResults(null)
      return
    }
    setSearchLoading(true)
    try {
      const results = await searchMemory(searchQuery)
      setSearchResults(results)
    } catch {
      // Filter mock data client-side
      const q = searchQuery.toLowerCase()
      setSearchResults(
        MOCK_EPISODES.filter((e) => e.content.toLowerCase().includes(q)),
      )
    } finally {
      setSearchLoading(false)
    }
  }, [searchQuery])

  const handleSearchKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter') {
        void handleSearch()
      }
    },
    [handleSearch],
  )

  const displayEpisodes = searchResults ?? episodes

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold text-gray-100">Memory Explorer</h1>
        <p className="mt-1 text-sm text-gray-400">
          Browse and search episodic memory across agent sessions
        </p>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-4">
        {/* Session selector sidebar */}
        <div className="lg:col-span-1">
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">Sessions</CardTitle>
            </CardHeader>
            <CardContent className="p-2">
              {MOCK_SESSIONS.map((session) => (
                <button
                  key={session}
                  className={`w-full rounded px-3 py-2 text-left text-sm transition-colors ${
                    selectedSession === session
                      ? 'bg-blue-600/30 text-blue-300 border border-blue-600/50'
                      : 'text-gray-400 hover:bg-gray-700/40 hover:text-gray-200'
                  }`}
                  onClick={() => setSelectedSession(session)}
                >
                  <span className="font-mono">{session}</span>
                  <span className="ml-2 text-xs text-gray-600">
                    {MOCK_EPISODES.filter((e) => e.session_id === session).length} eps
                  </span>
                </button>
              ))}
            </CardContent>
          </Card>
        </div>

        {/* Main content */}
        <div className="space-y-4 lg:col-span-3">
          {/* Search bar */}
          <Card>
            <CardContent className="p-4">
              <div className="flex gap-3">
                <div className="relative flex-1">
                  <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-500" />
                  <Input
                    className="pl-9"
                    placeholder="Semantic search across all memories…"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    onKeyDown={handleSearchKeyDown}
                  />
                </div>
                <Button
                  variant="secondary"
                  onClick={() => void handleSearch()}
                  disabled={searchLoading}
                >
                  {searchLoading ? (
                    <RefreshCw className="h-4 w-4 animate-spin" />
                  ) : (
                    'Search'
                  )}
                </Button>
                {searchResults && (
                  <Button
                    variant="ghost"
                    onClick={() => {
                      setSearchResults(null)
                      setSearchQuery('')
                    }}
                  >
                    Clear
                  </Button>
                )}
              </div>
            </CardContent>
          </Card>

          {/* Results header */}
          <div className="flex items-center justify-between">
            <div className="text-sm text-gray-400">
              {searchResults ? (
                <span>
                  <span className="font-semibold text-gray-200">{searchResults.length}</span>{' '}
                  search result{searchResults.length !== 1 ? 's' : ''} for &ldquo;{searchQuery}&rdquo;
                </span>
              ) : (
                <span>
                  <span className="font-semibold text-gray-200">{episodes.length}</span>{' '}
                  episodes in{' '}
                  <span className="font-mono text-gray-300">{selectedSession}</span>
                </span>
              )}
            </div>
            {!searchResults && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => void loadEpisodes(selectedSession)}
                disabled={loading}
              >
                <RefreshCw className={`mr-1.5 h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
                Refresh
              </Button>
            )}
          </div>

          {/* Error banner */}
          {error && (
            <div className="rounded-lg border border-yellow-700/50 bg-yellow-900/20 px-4 py-3 text-sm text-yellow-300">
              Backend unavailable — showing mock data. ({error})
            </div>
          )}

          {/* Episode list */}
          {loading ? (
            <div className="flex items-center justify-center py-12 text-gray-500">
              <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
              Loading episodes…
            </div>
          ) : displayEpisodes.length === 0 ? (
            <div className="rounded-lg border border-gray-700 bg-gray-800/30 py-16 text-center text-gray-500">
              {searchResults ? 'No matching memories found' : 'No episodes in this session'}
            </div>
          ) : (
            <div className="space-y-3">
              {displayEpisodes.map((episode) => (
                <EpisodeCard key={episode.id} episode={episode} />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
