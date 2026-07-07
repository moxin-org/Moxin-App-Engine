import React, { useState } from 'react'
import { CheckCircle, XCircle, BarChart3 } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '../components/ui/card'
import { Badge } from '../components/ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '../components/ui/table'
import { RubricRadar, type RadarCriterion } from '../components/RubricRadar'

// ─── Types ─────────────────────────────────────────────────────────────────────

interface EvalResult {
  id: string
  evaluator: string
  model: string
  run_id: string
  timestamp: string
  score: number
  passed: boolean
  reason: string
  criteria: RadarCriterion[]
}

// ─── Mock evaluation data ──────────────────────────────────────────────────────

const MOCK_EVALS: EvalResult[] = [
  {
    id: 'eval-001',
    evaluator: 'GPT-4o Judge',
    model: 'gpt-4o',
    run_id: 'run-2024-03-01-a',
    timestamp: new Date(Date.now() - 3600_000).toISOString(),
    score: 88,
    passed: true,
    reason: 'Response was accurate and comprehensive, with minor gaps in safety considerations.',
    criteria: [
      { criterion: 'Accuracy', score: 92, fullMark: 100 },
      { criterion: 'Helpfulness', score: 90, fullMark: 100 },
      { criterion: 'Safety', score: 78, fullMark: 100 },
      { criterion: 'Coherence', score: 95, fullMark: 100 },
      { criterion: 'Conciseness', score: 85, fullMark: 100 },
    ],
  },
  {
    id: 'eval-002',
    evaluator: 'Claude Judge',
    model: 'claude-3-5-sonnet',
    run_id: 'run-2024-03-01-b',
    timestamp: new Date(Date.now() - 7200_000).toISOString(),
    score: 76,
    passed: true,
    reason: 'Generally helpful but verbose. Safety guardrails appropriately triggered.',
    criteria: [
      { criterion: 'Accuracy', score: 80, fullMark: 100 },
      { criterion: 'Helpfulness', score: 75, fullMark: 100 },
      { criterion: 'Safety', score: 95, fullMark: 100 },
      { criterion: 'Coherence', score: 70, fullMark: 100 },
      { criterion: 'Conciseness', score: 60, fullMark: 100 },
    ],
  },
  {
    id: 'eval-003',
    evaluator: 'Rubric Eval v2',
    model: 'gpt-4o-mini',
    run_id: 'run-2024-03-02-a',
    timestamp: new Date(Date.now() - 86400_000).toISOString(),
    score: 45,
    passed: false,
    reason: 'Response contained factual errors and failed to address the core question.',
    criteria: [
      { criterion: 'Accuracy', score: 35, fullMark: 100 },
      { criterion: 'Helpfulness', score: 50, fullMark: 100 },
      { criterion: 'Safety', score: 88, fullMark: 100 },
      { criterion: 'Coherence', score: 45, fullMark: 100 },
      { criterion: 'Conciseness', score: 55, fullMark: 100 },
    ],
  },
  {
    id: 'eval-004',
    evaluator: 'Human Eval',
    model: 'gpt-4o',
    run_id: 'run-2024-03-02-b',
    timestamp: new Date(Date.now() - 172800_000).toISOString(),
    score: 95,
    passed: true,
    reason: 'Excellent response across all criteria. Clear, accurate, and appropriately scoped.',
    criteria: [
      { criterion: 'Accuracy', score: 98, fullMark: 100 },
      { criterion: 'Helpfulness', score: 96, fullMark: 100 },
      { criterion: 'Safety', score: 99, fullMark: 100 },
      { criterion: 'Coherence', score: 94, fullMark: 100 },
      { criterion: 'Conciseness', score: 88, fullMark: 100 },
    ],
  },
]

function formatTimestamp(iso: string): string {
  try {
    return new Date(iso).toLocaleString()
  } catch {
    return iso
  }
}

// ─── Aggregate radar (average across all evals) ────────────────────────────────

function buildAggregateRadar(evals: EvalResult[]): RadarCriterion[] {
  const sums: Record<string, { total: number; count: number }> = {}
  for (const e of evals) {
    for (const c of e.criteria) {
      if (!sums[c.criterion]) sums[c.criterion] = { total: 0, count: 0 }
      sums[c.criterion].total += c.score
      sums[c.criterion].count += 1
    }
  }
  return Object.entries(sums).map(([criterion, { total, count }]) => ({
    criterion,
    score: Math.round(total / count),
    fullMark: 100,
  }))
}

export function EvalResults() {
  const [selected, setSelected] = useState<EvalResult | null>(null)
  const aggregateRadar = buildAggregateRadar(MOCK_EVALS)

  const passed = MOCK_EVALS.filter((e) => e.passed).length
  const failed = MOCK_EVALS.filter((e) => !e.passed).length
  const avgScore =
    MOCK_EVALS.reduce((a, e) => a + e.score, 0) / MOCK_EVALS.length

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-100">Evaluation Results</h1>
          <p className="mt-1 text-sm text-gray-400">
            LLM-as-judge and rubric evaluation runs
          </p>
        </div>
      </div>

      {/* Summary stats */}
      <div className="grid grid-cols-3 gap-4">
        <div className="rounded-lg border border-gray-700 bg-gray-800/50 p-4 text-center">
          <p className="text-3xl font-bold text-gray-100">{MOCK_EVALS.length}</p>
          <p className="mt-1 text-xs text-gray-500 uppercase tracking-wider">Total Runs</p>
        </div>
        <div className="rounded-lg border border-emerald-700/50 bg-emerald-900/20 p-4 text-center">
          <p className="text-3xl font-bold text-emerald-400">{passed}</p>
          <p className="mt-1 text-xs text-emerald-600 uppercase tracking-wider">Passed</p>
        </div>
        <div className="rounded-lg border border-red-700/50 bg-red-900/20 p-4 text-center">
          <p className="text-3xl font-bold text-red-400">{failed}</p>
          <p className="mt-1 text-xs text-red-600 uppercase tracking-wider">Failed</p>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        {/* Aggregate Radar */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <BarChart3 className="h-4 w-4 text-emerald-400" />
              Aggregate Scores
            </CardTitle>
            <CardDescription>
              Average per-criterion scores across all {MOCK_EVALS.length} evaluation runs.
              Overall avg: <span className="text-gray-200 font-semibold">{avgScore.toFixed(1)}</span>
            </CardDescription>
          </CardHeader>
          <CardContent>
            <RubricRadar data={aggregateRadar} />
          </CardContent>
        </Card>

        {/* Selected eval radar */}
        <Card>
          <CardHeader>
            <CardTitle>
              {selected ? `${selected.evaluator} — Run ${selected.run_id}` : 'Select an Evaluation'}
            </CardTitle>
            <CardDescription>
              {selected
                ? `Score: ${selected.score}/100 · ${selected.passed ? 'PASSED' : 'FAILED'}`
                : 'Click a row in the table below to view per-criterion breakdown.'}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {selected ? (
              <>
                <RubricRadar data={selected.criteria} />
                <div className="mt-4 rounded-lg border border-gray-700 bg-gray-900/60 p-3 text-sm text-gray-300">
                  <span className="text-xs font-semibold uppercase tracking-wider text-gray-500">
                    Reason
                  </span>
                  <p className="mt-1">{selected.reason}</p>
                </div>
              </>
            ) : (
              <div className="flex h-[300px] items-center justify-center text-gray-600 text-sm">
                No evaluation selected
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Results table */}
      <Card>
        <CardHeader>
          <CardTitle>All Runs</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Evaluator</TableHead>
                <TableHead>Model</TableHead>
                <TableHead>Run ID</TableHead>
                <TableHead>Timestamp</TableHead>
                <TableHead>Score</TableHead>
                <TableHead>Passed</TableHead>
                <TableHead>Reason</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {MOCK_EVALS.map((ev) => (
                <TableRow
                  key={ev.id}
                  className="cursor-pointer"
                  data-state={selected?.id === ev.id ? 'selected' : undefined}
                  onClick={() => setSelected(selected?.id === ev.id ? null : ev)}
                >
                  <TableCell className="font-medium text-gray-200">{ev.evaluator}</TableCell>
                  <TableCell>
                    <span className="rounded bg-gray-700/60 px-1.5 py-0.5 font-mono text-xs text-gray-300">
                      {ev.model}
                    </span>
                  </TableCell>
                  <TableCell className="font-mono text-xs text-gray-400">{ev.run_id}</TableCell>
                  <TableCell className="text-xs text-gray-400">{formatTimestamp(ev.timestamp)}</TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <div className="h-1.5 w-16 rounded-full bg-gray-700">
                        <div
                          className={`h-1.5 rounded-full ${ev.score >= 70 ? 'bg-emerald-500' : ev.score >= 50 ? 'bg-yellow-500' : 'bg-red-500'}`}
                          style={{ width: `${ev.score}%` }}
                        />
                      </div>
                      <span className="text-sm font-semibold text-gray-200">{ev.score}</span>
                    </div>
                  </TableCell>
                  <TableCell>
                    {ev.passed ? (
                      <CheckCircle className="h-5 w-5 text-emerald-400" />
                    ) : (
                      <XCircle className="h-5 w-5 text-red-400" />
                    )}
                  </TableCell>
                  <TableCell className="max-w-sm truncate text-xs text-gray-400">
                    {ev.reason}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  )
}
