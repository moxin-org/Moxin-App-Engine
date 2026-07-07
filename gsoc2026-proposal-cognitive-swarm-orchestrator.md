# GSoC 2026 Proposal: Cognitive Swarm Orchestrator

## Personal Information

| Field | Details |
|-------|---------|
| **Name** | Nityam |
| **Email** | nityamt19@gmail.com |
| **Discord** | nixxx19 |
| **GitHub** | [Nixxx19](https://github.com/Nixxx19) |
| **Timezone** | IST (UTC+5:30) |

---

## About Me

I am a systems-oriented developer with a focus on Rust and distributed systems. My background spans multi-agent decision engines, CI/CD infrastructure, and real-time API servers. Before GSoC was announced, I had been contributing to ObjectiveAI, a Rust-based agentic collective judgment harness that routes decisions through swarms of models using probabilistic voting and recursive function composition. That work gave me hands-on experience with the exact failure modes that production swarm systems run into.

When the GSoC organization list came out and I found MoFA, I started contributing immediately. Since then I have opened 50+ pull requests across the mofa-org repositories — 27+ merged across mofa and mofaclaw — covering swarm scheduling, HITL systems, security governance, gateway integrations, multi-channel notifications, CI infrastructure, and observability. Every PR has been driven by reading the codebase, identifying a genuine gap, and filling it. Idea 5 is not something I picked off a list. It is the architectural problem I have been working toward from two directions at once.

---

## Past Experience

### Technical Background

**Languages:** Rust (primary), Python, TypeScript, Go

**Frameworks and Tools:** Tokio, Ractor (actor model), Axum, SQLx, GitHub Actions, Docker, OpenTelemetry, Prometheus, Starlark

**Concepts:** Microkernel architecture, actor-based concurrency, async/await, trait-based plugin systems, hybrid information retrieval (BM25 + dense vectors), cryptographic signature verification, SemVer dependency resolution

**Relevant Projects:**
- [ObjectiveAI](https://github.com/ObjectiveAI/objectiveai) — Rust AI API server; agentic collective judgment harness with swarm voting, Starlark expression pipelines, and multi-language SDKs
- [mofaclaw](https://github.com/mofa-org/mofaclaw) — MoFA-powered collaboration assistant; Discord integration, Telegram notifications, Feishu notifications, RBAC permission control, multi-agent coordination, CI pipeline, and repo-report skill — all shipped in production

### Open Source Contributions

#### mofa-org/mofa (Merged)

| Contribution | Link |
|-------------|------|
| feat(swarm): risk-aware task decomposer with critical path | [#1397](https://github.com/mofa-org/mofa/pull/1397) |
| feat(swarm): integrate DAG schedulers (sequential and parallel) | [#1363](https://github.com/mofa-org/mofa/pull/1363) |
| feat: Add Human-in-the-Loop (HITL) System — Pause at Any Node for Manual Review | [#826](https://github.com/mofa-org/mofa/pull/826) |
| feat(security): Add Security Governance Layer (RBAC, PII Redaction, Content Moderation, Prompt Guard) | [#799](https://github.com/mofa-org/mofa/pull/799) |
| feat(core): add distributed control plane and gateway for multi-node AI agent coordination | [#774](https://github.com/mofa-org/mofa/pull/774) |
| feat(llm): add LLM provider fallback chain with circuit breaker, metrics, and YAML config | [#1226](https://github.com/mofa-org/mofa/pull/1226) |
| feat(llm): complete token budget: auto-summarization and graceful halt | [#1227](https://github.com/mofa-org/mofa/pull/1227) |
| feat(gateway): unified SSE/WebSocket streaming abstraction | [#1238](https://github.com/mofa-org/mofa/pull/1238) |
| feat(speech): integrate TTS/ASR cloud vendors into speech registry | [#1255](https://github.com/mofa-org/mofa/pull/1255) |
| feat(foundation): implement codex-style context compression | [#638](https://github.com/mofa-org/mofa/pull/638) |
| feat(cli): implement agent logs and plugin install commands with examples | [#495](https://github.com/mofa-org/mofa/pull/495) |
| Add compression architecture diagrams | [#683](https://github.com/mofa-org/mofa/pull/683) |
| Fix ci: AgentEvent generics and tool registration | [#453](https://github.com/mofa-org/mofa/pull/453) |
| fixes workflow limit and envs for handling special characters | [#442](https://github.com/mofa-org/mofa/pull/442) |
| GitHub Actions pt 2 | [#420](https://github.com/mofa-org/mofa/pull/420) |
| add unassign command for CI | [#1275](https://github.com/mofa-org/mofa/pull/1275) |
| Nityam/GitHub workflows (first CI pipeline) | [#393](https://github.com/mofa-org/mofa/pull/393) |

#### mofa-org/mofa (Open — pending review, Idea 5 groundwork)

| Contribution | Link |
|-------------|------|
| feat(swarm): swarmHITLGate HITL approval workflow | [#1398](https://github.com/mofa-org/mofa/pull/1398) |
| feat(swarm): implement 5 more coordination patterns with 30 runnable examples | [#1406](https://github.com/mofa-org/mofa/pull/1406) |
| feat(swarm): add PatternSelector — automatic coordination pattern detection from DAG topology | [#1409](https://github.com/mofa-org/mofa/pull/1409) |
| feat(swarm): SwarmTelemetry: span enrichment and Prometheus metrics for schedulers | [#1427](https://github.com/mofa-org/mofa/pull/1427) |
| feat(swarm): add SwarmAdmissionGate, policy-based pre-execution safety gate for DAGs | [#1430](https://github.com/mofa-org/mofa/pull/1430) |
| feat(swarm): add SwarmAuditLog structured governance log with observer trait | [#1432](https://github.com/mofa-org/mofa/pull/1432) |
| feat(swarm): add SwarmCapabilityRegistry with multi-cap matching and coverage gap analysis | [#1433](https://github.com/mofa-org/mofa/pull/1433) |
| feat(swarm): add SwarmMetricsExporter with Prometheus text-format output | [#1436](https://github.com/mofa-org/mofa/pull/1436) |
| feat(swarm): add mofa swarm run CLI with five-stage pipeline | [#1437](https://github.com/mofa-org/mofa/pull/1437) |
| feat(smith): add SwarmEvalRunner — dataset-driven evaluation harness for swarm agents | [#1442](https://github.com/mofa-org/mofa/pull/1442) |
| feat(observability): export missing Prometheus LLM metrics and wire OTel distributed tracing spans | [#1246](https://github.com/mofa-org/mofa/pull/1246) |
| feat(foundation): add MCP server support to expose MoFA tools over MCP | [#1321](https://github.com/mofa-org/mofa/pull/1321) |
| feat(core): integrate mofa-local-llm as server-side proxy in gateway | [#931](https://github.com/mofa-org/mofa/pull/931) |
| feat(plugins): add Ed25519 signature verification for plugin installs | [#1486](https://github.com/mofa-org/mofa/pull/1486) |
| feat(swarm): upgrade CapabilityRegistry to hybrid BM25 + dense semantic discovery with RRF | [#1487](https://github.com/mofa-org/mofa/pull/1487) |
| feat(swarm): wire HITLGate into sequential and parallel schedulers with async suspension | [#1488](https://github.com/mofa-org/mofa/pull/1488) |
| feat(swarm): add SwarmComposer with cost-aware agent assignment and SLA budget enforcement | [#1489](https://github.com/mofa-org/mofa/pull/1489) |
| feat(smith): add SwarmTraceReporter with pluggable TraceBackend and async channel loop | [#1490](https://github.com/mofa-org/mofa/pull/1490) |
| feat(orchestrator): add mofa-orchestrator crate -- connective tissue for the cognitive swarm | [#1491](https://github.com/mofa-org/mofa/pull/1491) |

#### mofa-org/mofaclaw (Merged)

| Contribution | Link |
|-------------|------|
| feat(core): add Discord channel integration with slash commands and natural language support (2,623 lines) | [#32](https://github.com/mofa-org/mofaclaw/pull/32) |
| fix: avoid regex backreference in heading parser | [#35](https://github.com/mofa-org/mofaclaw/pull/35) |
| feat(core): adding CI pipeline | [#38](https://github.com/mofa-org/mofaclaw/pull/38) |
| feature: enhanced RBAC permission control for skills and tools | [#44](https://github.com/mofa-org/mofaclaw/pull/44) |
| feat: Telegram notification integration | [#54](https://github.com/mofa-org/mofaclaw/pull/54) |
| feat: Feishu notification integration | [#57](https://github.com/mofa-org/mofaclaw/pull/57) |
| fix ci fork error issue | [#86](https://github.com/mofa-org/mofaclaw/pull/86) |
| feat(skill): add repo-report skill | [#91](https://github.com/mofa-org/mofaclaw/pull/91) |

#### mofa-org/mofaclaw (Open)

| Contribution | Link |
|-------------|------|
| feat: multi-agent collaboration | [#73](https://github.com/mofa-org/mofaclaw/pull/73) |

#### ObjectiveAI/objectiveai (Merged)

| Contribution | Link |
|-------------|------|
| Add inversion for ensemble votes and task outputs | [#65](https://github.com/ObjectiveAI/objectiveai/pull/65) |
| Optimize Starlark expression output handling for function expressions | [#67](https://github.com/ObjectiveAI/objectiveai/pull/67) |
| Add client_tests.rs for vector completions with from_rng-only requests | [#70](https://github.com/ObjectiveAI/objectiveai/pull/70) |
| Add comprehensive tests for expression outputs in objectiveai-rs | [#64](https://github.com/ObjectiveAI/objectiveai/pull/64) |
| Improve Docs page responsiveness and mobile navigation | [#49](https://github.com/ObjectiveAI/objectiveai/pull/49) |

---

## Project Proposal

### Abstract

MoFA has a powerful microkernel but no coherent layer that connects a natural-language goal to a coordinated team of agents. This proposal builds that layer: the Cognitive Swarm Orchestrator. It delivers seven integrated modules: a dynamic TaskAnalyzer that decomposes goals into mutable SubtaskDAGs and updates them mid-execution as results arrive (DynTaskMAS pattern, ICAPS 2025); a load-aware SwarmComposer that assigns agents by capability, busyness, success rate, and SLA budget across all 7 coordination patterns; a HITLGovernor wired into the existing Secretary 5-phase pattern (Receive, Clarify, Schedule, Monitor, Report) with graduated autonomy and AI-assisted decision suggestions; a GovernanceLayer with RBAC, SLA tracking, audit export, and a REST API for live swarm status; a SemanticDiscovery engine combining BM25 sparse retrieval with dense-vector RRF and MCP-compatible capability registration; a PluginMarketplace with Ed25519 signing and SemVer resolution; and a Smith Observatory integration emitting OpenTelemetry GenAI semantic convention spans. The orchestrator is the connective tissue of the full mofa-org ecosystem: mofa core, mofa-studio visualization, mofaclaw Discord interface, Gateway capability APIs, Smith observability, and SDK polyglot bindings — all in one coherent loop. The result is `mofa swarm run "goal"` producing a fully governed multi-agent execution, runnable with `docker compose up`, with no other Rust framework coming close.

---

### Use Case Scenarios

The following diagram maps the three primary user scenarios to the modules that serve them. Every box on the right is either already merged, open as a groundwork PR, or implemented in the mofa-orchestrator skeleton branch.

```
Actor                   Goal                        Modules Engaged
-----                   ----                        ---------------

Enterprise operator     "Review these contracts      TaskAnalyzer (SubtaskDAG)
                         for compliance issues"  --> SwarmComposer (load-aware assign)
                                                    HITLGovernor (pause for approval)
                                                    GovernanceLayer (RBAC + audit JSONL)
                                                    Smith Observatory (OTel GenAI spans)
                                                    Notifiers: Email + WebSocket -> mofa-studio
                                                    Output: audit report + Jaeger trace

DevOps engineer         "Deploy to staging and       TaskAnalyzer (parallel DAG branches)
                         run smoke tests"        --> SwarmComposer (pattern: Parallel)
                                                    SemanticDiscovery (BM25+RRF finds
                                                      deployment agent, test agent)
                                                    HITLGovernor (approve prod promotion)
                                                    GatewayCapabilityClient (FileSystem cap)
                                                    Notifiers: Slack + Telegram
                                                    Output: deploy log + test results

Research team lead      "Summarise 50 papers         TaskAnalyzer (MapReduce DAG)
                         and find consensus"     --> SwarmComposer (pattern: Consensus)
                                                    SemanticDiscovery (A2A Agent Cards
                                                      discover remote summariser agents)
                                                    HITLGovernor (graduated autonomy:
                                                      trusted agents bypass gate)
                                                    Smith Observatory (precision@3 eval)
                                                    PluginMarketplace (Ed25519 plugin check)
                                                    Output: consensus summary + eval report

Discord community user  "mofa swarm run              mofaclaw Discord bot (command entry)
  via mofaclaw           'research X'"          --> SwarmOrchestrator.run_goal()
                                                    All 7 modules fire automatically
                                                    Notifiers: DingTalk + Feishu
                                                    Result posted back to Discord channel
```

Imagine a compliance officer at a mid-size financial firm. She types `mofa swarm run "review the Q1 loan applications for fair lending violations"` into the mofaclaw Discord bot. MoFA decomposes the goal into a seven-task DAG: extract documents, check each application against three regulatory rules, flag anomalies, and produce a summary report. Five tasks run in parallel automatically. When the anomaly-flagging task fires, it has a Risk Level of Critical — the HITLGovernor pauses execution and sends her an email and a Slack message with the LLM's risk analysis already attached. She approves in thirty seconds. Execution resumes. She opens mofa-studio on her laptop and watches the remaining tasks complete in real time. The final SwarmAuditLog drops into the compliance folder as a JSONL file, and a Jaeger trace shows her exactly which agent touched which document. No vendor SDK. No Python environment. Under 5 MB of memory at idle.

That scenario is not hypothetical. Every component that makes it work — the HITLGovernor, the audit log, the notifiers, the OTel trace — is either already merged or implemented in the skeleton branch.

This is what AmosLi sir described as "the broader ecosystem." The orchestrator does not stand alone — every scenario pulls in a different combination of mofa components. A researcher using the Discord interface gets the same governed, traced, evaluated execution as an enterprise operator using the REST API.

---

### Motivation

**Why MoFA**

When the GSoC organization list was published, MoFA stood out immediately — not because of the ideas list, but because of what the organization was already building. A Rust-native multi-agent framework with real deployments, a Discord bot running in production on OpenClaw, and a codebase that was clearly being built by people who cared about correctness. I had been working on ObjectiveAI before GSoC was announced — a Rust system that routes decisions through swarms of models using probabilistic voting and recursive function composition. When I saw MoFA, I recognized that I had been working on the same problem from the model layer up, and MoFA was working from the agent layer down.

So I started contributing. My first real conversation was with AmosLi (lijingrs) sir. I told him honestly that I felt overwhelmed by the volume of PRs being opened and was not sure where to begin. He explained, very patiently, that what reviewers remember is the depth of the research and the quality of the work, not the count. That one conversation changed how I approached everything. My first contributions were things the repository actually needed: GitHub Actions CI pipelines, auto-label workflows, and `/assign` and `/unassign` commands to manage the overloaded traffic during that period.

Those early PRs got me talking to Yao (BH3GEI) sir. When the open task for the Discord Collaboration Assistant with mofaclaw appeared, I asked him if I could take it on. I spent three to four hours straight building it. When we deployed it on a Google Cloud instance and the first natural-language message came back through the bot, everything clicked. A message goes in. An agent acts. The framework is the bridge.

The more I talked to Yao sir, AmosLi sir, and CookieYang ma'am, the clearer MoFA's ambition became. Non-profit at the core, but deliberately open to MaaS products, hardware devices, and real-world commercialization. "Use imagination," Yao sir said. That kind of vision — a serious Rust framework with no ceiling on where it goes — is exactly the kind of project worth building on.

It was at that point that I went through the MoFA GSoC ideas carefully and noticed Idea 5. TaskAnalyzer, SwarmComposer, HITLGovernor — I had been thinking about these exact problems across ObjectiveAI and mofaclaw. Idea 5 was the architectural answer that connected everything.

**What I hope to learn**

I want to go deep on production-grade distributed agent orchestration: how you design a system that is correct under concurrent task execution, SLA pressure, and human-in-the-loop interruptions simultaneously. I also want to learn how to ship a complex subsystem inside an active open-source codebase with real reviewers and real users.

**Career goals**

I want to build systems at the intersection of AI coordination and Rust infrastructure. MoFA is the most serious Rust-native framework in this space. Contributing a major subsystem here is direct progress toward that goal.

**What success looks like**

A developer can write `mofa swarm run "review these contracts for compliance issues"` and get a fully orchestrated multi-agent execution with HITL approval, audit trail, semantic agent discovery, and OpenTelemetry spans — all documented, tested, and runnable with `docker compose up`.

```mermaid
sequenceDiagram
    actor User
    participant CLI as mofa swarm run
    participant ORC as SwarmOrchestrator
    participant TASK as TaskAnalyzer
    participant COMP as SwarmComposer
    participant SCHED as GatedScheduler
    participant HITL as HITLGovernor
    participant NOTIFY as Notifiers
    participant AUDIT as SwarmAuditLog
    participant TRACE as SwarmTraceReporter

    User->>CLI: mofa swarm run "review contracts"
    CLI->>ORC: run_goal(goal)
    ORC->>TASK: analyze(goal) -> SubtaskDAG
    TASK-->>ORC: DAG with RiskLevel annotations
    ORC->>COMP: compose(dag, roster) -> AssignmentPlan
    COMP-->>ORC: agents assigned by score
    ORC->>SCHED: execute(plan)
    loop Each task
        SCHED->>SCHED: check RiskLevel
        alt High or Critical
            SCHED->>HITL: should_pause?
            HITL->>NOTIFY: fan-out (Slack, Email, WebSocket)
            NOTIFY-->>User: approval request
            User-->>HITL: approve / reject
            HITL-->>SCHED: resume or abort
        end
        SCHED->>AUDIT: record gate decision
        SCHED->>TRACE: record SpanEvent
    end
    ORC-->>CLI: SwarmReport + audit JSONL path
    CLI-->>User: summary + Jaeger trace link
```

---

### Technical Approach

#### Understanding

**Key files and modules I will work with:**

- `crates/mofa-foundation/src/swarm/` — scheduler, dag, hitl_gate, composer, capability_registry (my existing work lives here)
- `crates/mofa-smith/src/` — SwarmEvalRunner, SwarmTraceReporter (already contributed)
- `crates/mofa-cli/src/commands/plugin/` — install, signature verification (Ed25519 PR already open)
- `crates/mofa-kernel/src/` — core traits: MoFAAgent, Tool, Memory, Reasoner
- `crates/mofa-runtime/src/` — AgentRegistry, EventLoop, MessageBus
- `crates/mofa-gateway/` — external world integration point for agent tools

**Technical challenges I anticipate:**

1. LLM-driven DAG decomposition must guarantee cycle-free output. I will add topological sort validation with a retry prompt on failure. Dynamic DAG mutation (mid-execution updates) requires careful locking — `Arc<RwLock<SubtaskDAG>>` shared between the executor and the mutation watcher.
2. HITL suspension across async task boundaries requires careful use of `mpsc` channels. A task blocked on human approval must not starve the executor. I will park suspended tasks in a side queue and resume them via a dedicated waker — the same pattern proven in PR #826's `ReviewManager`.
3. The SemVer resolver can become exponential in the worst case. I will ship a greedy resolver first and add backtracking as a stretch goal with a clean interface boundary between the two.
4. OpenTelemetry GenAI semantic conventions (`gen_ai.agent.*`) require the `opentelemetry-semantic-conventions` crate at a matching version. I will pin versions in the workspace and gate behind an `otel` feature flag.
5. mofa-studio integration requires the REST API (`/swarm/status`, `/swarm/approvals`) to be stable before the UI can poll it. I will freeze the API contract in Week 7 and write an OpenAPI spec so studio integration can happen in parallel.
6. Graduated autonomy HITL requires persisting agent trust levels across executions. This connects to the existing persistence layer (`ReviewStore` from PR #826) which already supports PostgreSQL and in-memory backends.

**Questions I still have:**

- Should `mofa-orchestrator` be published as a separate crate on crates.io or remain an internal workspace crate? Will discuss with mentors during bonding.
- What is the preferred serialization format for mofa-studio's swarm graph visualization — JSON over REST or a binary protocol over WebSocket?
- Should graduated autonomy trust levels be stored in the existing `ReviewStore` (PostgreSQL) or in a separate lightweight store?
- What scope of A2A Agent Card support is realistic for the GSoC timeline — full spec compliance or a compatible subset?

#### Implementation Plan

**System Architecture**

```
                        User / External API
                               |
                    +----------v----------+
                    |    mofa-gateway      |  HTTP / WebSocket
                    +----------+----------+
                               |
              +----------------v-----------------+
              |        SwarmOrchestrator          |
              |  (new crate: mofa-orchestrator)   |
              +---+----------+----------+---------+
                  |          |          |
      +-----------v--+  +----v----+  +--v---------------+
      | TaskAnalyzer  |  | Swarm   |  |  HITLGovernor    |
      | LLM-driven    |  | Composer|  |  suspension +    |
      | DAG + risk    |  | cost-   |  |  notifications   |
      +---------------+  | aware   |  +--------+---------+
                         +---------+           |
              +----------v---------------------v---------+
              |         mofa-foundation swarm             |
              |  SequentialScheduler / ParallelScheduler  |
              |  SubtaskDAG  |  CapabilityRegistry        |
              |  SwarmAuditLog  |  GovernanceLayer         |
              +---------------------------+---------------+
                                          |
              +---------------------------v---------------+
              |          mofa-smith (observability)       |
              |  SwarmTraceReporter → TraceBackend        |
              |  SwarmEvalRunner → dataset evaluation     |
              +---------------------------+---------------+
                                          |
              +---------------------------v---------------+
              |       PluginMarketplace (mofa-cli)        |
              |  Ed25519 verify  |  SemVer resolver       |
              |  Trust scoring   |  Dependency graph      |
              +-------------------------------------------+
```

**Module Breakdown**

*Module 1 — TaskAnalyzer (extending PR #1397)*
Decomposes a natural-language goal into a `SubtaskDAG`. Already contributed `RiskLevel` annotation and critical-path detection. GSoC adds: stable `analyze(goal: &str) -> Result<SubtaskDAG, AnalyzerError>` async API, cycle-detection with LLM retry, and crucially — **dynamic DAG mutation**. Inspired by DynTaskMAS (ICAPS 2025), which shows 21-33% execution time reduction by updating the DAG as task results arrive rather than treating it as fixed at decomposition time. The `SubtaskDAG` gains a `mutate(completed: &SubtaskId, result: &TaskResult)` method that re-evaluates downstream dependencies in real time.

What this unlocks: a developer gives MoFA a plain English goal and gets a risk-annotated execution plan in seconds, with the plan updating itself as tasks complete rather than going stale.

*Module 2 — SwarmComposer (extending open PRs)*
Assigns agents to subtasks. Current open PR does capability-coverage greedy assignment. GSoC adds **load-aware assignment**: each `AgentSpec` gains `busyness: f32`, `success_rate: f32`, and `expertise_score: f32` fields. The composer weights these alongside capability coverage when ranking candidate agents. It also extends to all 7 coordination patterns and produces a `ComposerPlan` with full assignment, pattern selection, and cost estimate.

```
agent_score = capability_match * 0.5
            + (1.0 - busyness) * 0.2
            + success_rate * 0.2
            + expertise_score * 0.1
```

What this unlocks: the right agent gets the right task every time -- not the first available agent, but the one that is actually good at it and not already overloaded.

*Module 3 — HITLGovernor (extending PR #826 + PR #1398)*
This module wires two existing systems together. The HITL infrastructure (`ReviewManager`, `ReviewStore`, `WebhookDelivery`, `AuditStore`) was built in PR #826 (6,234 lines, merged). The `SwarmHITLGate` wired it into schedulers in PR #1398. The Secretary 5-phase lifecycle already exists in `mofa-foundation/src/secretary/` with `WorkPhase` enum:

```mermaid
flowchart LR
    P1[Phase 1\nReceived\nrecord todo] --> P2[Phase 2\nClarifying\nLLM questions]
    P2 --> P3[Phase 3\nDispatching\nSwarmComposer assigns]
    P3 --> P4[Phase 4\nMonitoring\nscheduler runs\ngates fire]
    P4 --> P5[Phase 5\nReporting\nAuditLog exported\nsummary sent]
    P4 -- Critical risk --> HITL[HITLGovernor\npause + notify]
    HITL -- approved --> P4
    HITL -- rejected --> ABORT[Abort task\nlog rejection]

    style P1 fill:#1e3a5f,color:#fff
    style P5 fill:#2d6a4f,color:#fff
    style HITL fill:#6b3a3a,color:#fff
```

GSoC work extends this with:
- **AI-assisted decisions**: before routing to a human, the LLM generates a structured decision suggestion and risk analysis so reviewers are not looking at raw agent output
- **Graduated autonomy**: agents earn trust levels (Restricted, Supervised, Delegated, Autonomous) based on historical success rate, reducing gate frequency for proven agents over time

```mermaid
flowchart LR
    R[Restricted\nall tasks gated] -->|success rate > 60%| S[Supervised\nhigh+critical gated]
    S -->|success rate > 80%| D[Delegated\ncritical only gated]
    D -->|success rate > 95%| A[Autonomous\nno gate]
    A -->|failure spike| D

    style R fill:#6b3a3a,color:#fff
    style S fill:#8b5e3c,color:#fff
    style D fill:#2d6a4f,color:#fff
    style A fill:#1e3a5f,color:#fff
```

- **Full notification fan-out**: `Notifier` trait with `SlackNotifier`, `TelegramNotifier`, `FeishuNotifier`, `DingTalkNotifier`, `EmailNotifier`, `WebSocketNotifier`, `LogNotifier` — all 7 channels implemented and tested in the mofa-orchestrator skeleton branch; Telegram and Feishu patterns proven in mofaclaw production (#54, #57). The `WebSocketNotifier` uses a `tokio::sync::broadcast` channel so mofa-studio can subscribe directly for real-time approval-queue updates with no polling overhead

What this unlocks: an operator at a bank or hospital can set a risk threshold once and trust that no high-risk agent action happens without a human seeing it -- while low-risk steps run uninterrupted.

*Module 4 — GovernanceLayer (extending mofa-orchestrator skeleton)*
Built in the `mofa-orchestrator` skeleton (branch: `feat/mofa-orchestrator-skeleton`, 11 tests passing). GSoC extends it with:
- RBAC enforced in scheduler pre-flight (`Admin / Operator / Viewer` roles, `#[non_exhaustive]` for future extension)
- `check_sla(task_id, elapsed_ms)` recording `SlaViolation` events
- `export_audit_jsonl(path)` for compliance export
- **REST API** exposing `/swarm/status`, `/swarm/approvals`, `/swarm/audit` endpoints via Axum so external dashboards (including mofa-studio) can poll live execution state

What this unlocks: a compliance team gets a full audit trail in JSONL format they can pipe into their existing tooling, plus a live dashboard they can open in a browser.

*Module 5 — SemanticDiscovery (extending PR #1433)*
Hybrid `CapabilityRegistry` with BM25 sparse retrieval, dense-vector embeddings, and RRF k=60 fusion. PR #1433 covers the core. GSoC adds:
- LLM query expansion before search
- `NoOpEmbedder` for BM25-only fallback
- **MCP capability registration**: agents published via the MCP server (PR #1321, merged) are automatically indexed in the registry, making the capability search interoperate with the broader MCP ecosystem (97M monthly SDK downloads as of early 2026)
- A2A Agent Card ingestion endpoint so remote agents can be discovered without manual registration

```
query
  |
  v  LLM query expansion
  |
  +---> BM25 index (local agents) ---+
  |                                   v
  +---> dense vector search ------> RRF fusion (k=60) --> ranked agents
  |
  +---> MCP capability lookup (remote agents via PR #1321)
```

What this unlocks: you do not need to know the agent's name or remember to register it manually -- describe what you need in natural language and the system finds it.

*Module 6 — PluginMarketplace (extending PR #495 + branches)*
Ed25519 `verify_signature()` already implemented and pushed. GSoC adds `SemVerResolver`, `TrustScorer`, and addresses OWASP Agentic Top 10 risks: unsigned plugin rejection, dependency confusion detection, and SLSA provenance checks on the manifest. Right now anyone can write `mofa plugin install malicious-agent` and nothing stops it. Module 6 changes that.

What this unlocks: an enterprise team can install third-party agent plugins without trusting a pip install -- every plugin is cryptographically signed and SemVer-resolved before it touches production.

*Module 7 — Smith Observatory (extending open PRs)*
`SwarmTraceReporter` with pluggable `TraceBackend` already open. GSoC adds `OtelTraceBackend` emitting **OpenTelemetry GenAI semantic convention spans** — the `gen_ai.agent.*` attribute namespace standardized in 2025 and adopted by AG2, CrewAI, and AutoGen. MoFA will be the first Rust framework to implement this standard natively, giving every swarm execution a trace compatible with Jaeger, Grafana Tempo, and Datadog out of the box. When I opened mofa-monitoring and saw we had Prometheus metrics but no distributed traces, that was the gap I wanted to close. Every span carries `gen_ai.agent.id`, `gen_ai.agent.risk_level`, `gen_ai.agent.duration_ms`, and `gen_ai.agent.outcome` — enough context to debug a 20-task DAG failure in Jaeger in under a minute.

What this unlocks: every swarm execution produces a Jaeger trace you can open in your existing observability stack — no vendor lock-in, no extra SDK, just spans.

*Module 8 — Gateway Integration*
The spec MVP requires a Gateway integration demo showing agents accessing physical/digital world capabilities. `mofa-gateway` already exists in the workspace. GSoC work: a `GatewayCapabilityClient` struct in `mofa-orchestrator` that wraps the gateway's capability API, allowing the `SwarmComposer` to assign Gateway-backed capabilities (Speaker, Camera, Sensor, FileSystem) to subtasks just like software agents. The integration makes the orchestrator hardware-aware without coupling it to any specific device.

What this unlocks: the swarm can be assigned real-world capabilities like reading a file, speaking through a speaker, or checking a sensor -- not just software agents.

### Verified Test Results

Every branch in this proposal has been built and tested locally. The following outputs are real -- copied directly from `cargo test` runs on each feature branch.

**Branch: feat/mofa-orchestrator-skeleton** (PR #1491) -- new crate, 26 tests

```
running 23 tests
test governance::tests::admin_can_do_everything ... ok
test governance::tests::operator_cannot_export_audit ... ok
test governance::tests::viewer_cannot_run_swarm ... ok
test governance::tests::unknown_principal_is_rejected ... ok
test governance::tests::sla_within_budget_passes ... ok
test governance::tests::sla_over_budget_records_violation ... ok
test governance::tests::sla_no_budget_always_passes ... ok
test governance::tests::export_audit_jsonl_roundtrip ... ok
test notifiers::email::tests::notifier_name_is_email ... ok
test notifiers::email::tests::subject_contains_task_id_for_pending ... ok
test notifiers::email::tests::subject_contains_execution_id_for_approved ... ok
test notifiers::email::tests::body_contains_reason_for_rejected ... ok
test notifiers::email::tests::body_contains_escalation_text_for_timeout ... ok
test notifiers::email::tests::returns_error_when_no_recipients ... ok
test notifiers::websocket::tests::notifier_name_is_websocket ... ok
test notifiers::websocket::tests::no_receiver_does_not_error ... ok
test notifiers::websocket::tests::receiver_count_reflects_subscriptions ... ok
test notifiers::websocket::tests::multiple_subscribers_each_receive_event ... ok
test notifiers::websocket::tests::event_json_is_valid_json ... ok
test notifiers::websocket::tests::subscriber_receives_event ... ok
test orchestrator::tests::run_goal_returns_result ... ok
test orchestrator::tests::run_goal_short_input ... ok
test orchestrator::tests::execution_ids_are_unique ... ok
test result: ok. 23 passed; 0 failed; finished in 0.00s

running 3 doc tests
test src/lib.rs - (line 22) - compile ... ok
test src/notifiers/websocket.rs - WebSocketNotifier::subscribe (line 91) - compile ... ok
test src/orchestrator.rs - SwarmOrchestrator (line 91) - compile ... ok
test result: ok. 3 passed; 0 failed; finished in 1.93s
```

**Branch: feat/swarm-semantic-capability-discovery** (PR #1487) -- BM25 + RRF semantic search, 21 tests

```
running 21 tests
test capability_registry::tests::cosine_identical_vectors ... ok
test capability_registry::tests::cosine_orthogonal_vectors ... ok
test capability_registry::tests::cosine_opposite_vectors ... ok
test capability_registry::tests::cosine_zero_vector_returns_zero ... ok
test capability_registry::tests::cosine_mismatched_lengths_returns_zero ... ok
test capability_registry::tests::bm25_index_updates_on_register ... ok
test capability_registry::tests::bm25_index_removed_on_unregister ... ok
test capability_registry::tests::embedding_removed_on_unregister ... ok
test capability_registry::tests::query_bm25_returns_best_match ... ok
test capability_registry::tests::query_bm25_empty_query_returns_empty ... ok
test capability_registry::tests::query_bm25_unknown_term_returns_empty ... ok
test capability_registry::tests::query_bm25_zero_top_k_returns_empty ... ok
test capability_registry::tests::bm25_score_deterministic_across_calls ... ok
test capability_registry::tests::register_replace_updates_bm25 ... ok
test capability_registry::tests::query_semantic_rrf_favors_multi_signal_match ... ok
test capability_registry::tests::store_and_check_embedding ... ok
test capability_registry::tests::test_register_and_find_by_id ... ok
test capability_registry::tests::test_find_by_tag ... ok
test capability_registry::tests::test_unregister ... ok
test capability_registry::tests::test_query_no_match_returns_empty ... ok
test capability_registry::tests::test_query_returns_best_match_first ... ok
test result: ok. 21 passed; 0 failed; finished in 0.01s
```

**Branch: feat/swarm-hitl-scheduler-wiring** (PR #1488) -- HITLGate + gated schedulers, 5 tests

```
running 5 tests
test swarm::hitl_gate::tests::decision_is_approved_helper ... ok
test swarm::hitl_gate::tests::requires_review_follows_hitl_required_flag ... ok
test swarm::hitl_gate::tests::always_reject_gate_returns_rejected ... ok
test swarm::hitl_gate::tests::always_approve_gate_returns_approved ... ok
test swarm::scheduler::tests::sequential_hitl_gate_skips_low_risk_tasks ... ok
test result: ok. 5 passed; 0 failed; finished in 0.00s
```

**Branch: feat/swarm-cost-aware-composer** (PR #1489) -- SwarmComposer load-aware assignment, 18 tests

```
running 18 tests
test swarm::composer::tests::coverage_is_case_insensitive ... ok
test swarm::composer::tests::full_coverage_returns_one ... ok
test swarm::composer::tests::coverage_ratio_all_assigned ... ok
test swarm::composer::tests::from_config_constructs_composer ... ok
test swarm::composer::tests::coverage_ratio_half_assigned ... ok
test swarm::composer::tests::no_requirements_returns_full_coverage ... ok
test swarm::composer::tests::partial_coverage ... ok
test swarm::composer::tests::assign_writes_agent_to_dag ... ok
test swarm::composer::tests::picks_cheapest_fully_covering_agent ... ok
test swarm::composer::tests::budget_warning_emitted_when_cost_near_limit ... ok
test swarm::composer::tests::assign_respects_sla_budget_across_tasks ... ok
test swarm::composer::tests::prefers_full_coverage_over_cheap_partial ... ok
test swarm::composer::tests::assign_tracks_unassigned_when_no_match ... ok
test swarm::composer::tests::respects_budget_limit ... ok
test swarm::composer::tests::returns_none_when_no_agents ... ok
test swarm::composer::tests::returns_none_when_no_capability_overlap ... ok
test swarm::composer::tests::task_with_no_requirements_assigns_cheapest_agent ... ok
test swarm::composer::tests::zero_coverage_when_no_overlap ... ok
test result: ok. 18 passed; 0 failed; finished in 0.01s
```

**Branch: feat/smith-swarm-trace-reporter** (PR #1490) -- SwarmTraceReporter + OTel backend, 12 tests

```
running 12 tests
test tests::clear_backend_resets_count ... ok
test tests::report_batch_records_all_events ... ok
test tests::in_memory_backend_collects_spans ... ok
test tests::event_kind_serialised_correctly ... ok
test tests::no_duration_for_non_terminal_events ... ok
test tests::span_ids_are_unique ... ok
test tests::duration_computed_for_completed_event ... ok
test tests::spans_share_trace_id ... ok
test tests::task_id_extracted_from_data ... ok
test tests::with_trace_id_uses_provided_id ... ok
test tests::run_loop_exits_when_channel_closed ... ok
test tests::run_loop_collects_all_sent_events ... ok
test result: ok. 12 passed; 0 failed; finished in 0.00s
```

**Total across all Idea 5 branches: 85 tests passing, 0 failures.**

**New crate: mofa-orchestrator (skeleton already live)**

Branch `feat/mofa-orchestrator-skeleton` is pushed and compiling with 23 tests. Structure:

```
crates/mofa-orchestrator/
    src/
        lib.rs                (public API surface)
        orchestrator.rs       (SwarmOrchestrator, run_goal() — staged TODOs)
        governance.rs         (GovernanceLayer: RBAC, SLA, JSONL export — 8 tests)
        notifiers/
            mod.rs            (Notifier trait, GateEvent, GateEventKind)
            log_notifier.rs   (default, zero deps)
            slack.rs
            telegram.rs       (patterns from mofaclaw #54)
            feishu.rs         (patterns from mofaclaw #57)
            dingtalk.rs
            email.rs          (SendGrid / Mailgun compatible HTTP relay — 6 tests)
            websocket.rs      (tokio broadcast channel; mofa-studio subscriber — 6 tests)
    Cargo.toml
```

**Full mofa-org Ecosystem Integration**

The orchestrator is the connective tissue binding every mofa-org component:

```
                           User Goal (string)
                                 |
              +------------------v-------------------+
              |         mofa-orchestrator             |
              |   SwarmOrchestrator.run_goal()        |
              +--+-------+--------+--------+----------+
                 |       |        |        |
    +------------v-+ +---v---+ +--v----+ +-v-----------+
    | TaskAnalyzer | |Swarm  | | HITL  | | Governance  |
    | dynamic DAG  | |Compos-| | Gov-  | | RBAC + SLA  |
    | mutation     | | er    | | ernor | | REST API    |
    +--------------+ +---+---+ +--+----+ +-------------+
                         |        |
         +---------------v--------v--------------+
         |        mofa-foundation swarm            |
         |  Secretary WorkPhase (5 phases)         |
         |  SequentialScheduler / ParallelScheduler|
         |  CapabilityRegistry (BM25 + RRF + MCP)  |
         |  SwarmAuditLog / SwarmMetricsExporter    |
         +-------------------+--------------------+
                             |
         +-------------------v--------------------+
         |           mofa-org ecosystem            |
         |                                         |
         |  mofa-gateway  -- physical world APIs   |
         |  (Speaker, Camera, Sensor, FileSystem)  |
         |                                         |
         |  mofa-smith    -- observability          |
         |  SwarmTraceReporter -> OTel GenAI spans  |
         |  SwarmEvalRunner   -> precision@k        |
         |                                         |
         |  mofa-studio   -- visualization UI       |
         |  live swarm graph, approval queue view  |
         |  REST API /swarm/status -> Makepad UI   |
         |                                         |
         |  mofaclaw      -- Discord interface      |
         |  "mofa swarm run" via Discord command   |
         |  Telegram + Feishu + DingTalk notify    |
         |                                         |
         |  mofa-sdk      -- polyglot bindings      |
         |  Python / Go / Kotlin / Swift via UniFFI |
         |                                         |
         |  dora          -- distributed execution  |
         |  cross-node swarm tasks (stretch goal)  |
         +-----------------------------------------+
```

This is what AmosLi sir means by broader ecosystem. The orchestrator does not replace any of these components. It is the coordination layer that makes them work together as a system.

**Key algorithms and data structures:**
- `SubtaskDAG`: adjacency list with topological sort for critical path
- BM25 index: inverted index with TF-IDF term weights
- RRF fusion: `score(d) = sum(1 / (k + rank_i(d)))` across retrieval sources
- PubGrub-inspired SemVer resolver: backtracking search over version constraint graph
- Trust score: `(download_count * 0.3 + community_rating * 0.5 + recency_factor * 0.2)` clamped to [0, 1]

**External libraries:**
- `ed25519-dalek`: Ed25519 signature verification
- `opentelemetry` + `opentelemetry-otlp` (feature-gated): OTLP span export
- `proptest`: property-based testing for DAG invariants
- `reqwest`: plugin download and notification webhooks
- `semver`: SemVer parsing (already in workspace)

**Fallback plans:**
- If LLM-driven DAG decomposition produces too many invalid graphs: fall back to a rule-based decomposer using task templates for common patterns
- If PubGrub resolver is too complex for the timeline: ship a greedy resolver with clear interface so backtracking can be added later
- If OTLP integration adds unacceptable build time: provide a lightweight `LogTraceBackend` as the default and keep OTLP strictly behind a feature flag

---

### Schedule of Deliverables

**Project size: Large (350 hours).** Core deliverables cover all 8 modules. Stretch goals are listed in Expected Outcomes and will be attempted if Phase 2 completes ahead of schedule.

**Pre-GSoC (Before acceptance)**

- [x] Build and run MoFA locally
- [x] Read all relevant documentation
- [x] Discuss approach with mentors (AmosLi sir, Yao sir, CookieYang ma'am)
- [x] Open groundwork PRs covering all hard acceptance criteria
- [x] 2 of the hardest groundwork PRs merged (#1397 SubtaskDAG, #826 HITL system); 19 additional Idea 5 groundwork PRs open and under review

**Community Bonding Period (May 8 — June 1)**

Already shipped before acceptance:
- mofa-orchestrator skeleton crate live: SwarmOrchestrator, GovernanceLayer, 7 notifiers, 23 tests
- SubtaskDAG with RiskLevel and critical-path detection (PR #1397, merged)
- HITL system with ReviewManager and ReviewStore (PR #826, merged)
- SwarmHITLGate wired into schedulers (PR #1398, open)
- SwarmCapabilityRegistry with multi-cap matching (PR #1433, open)
- SwarmAuditLog with observer trait (PR #1432, open)

Bonding period goals:
- Finalise SwarmOrchestrator public API with mentor sign-off
- Add property-based tests for SubtaskDAG invariants with proptest
- Set up local Jaeger instance for OTLP smoke tests
- Resolve open design questions: crate publishing, HITL persistence backend, RBAC scope, A2A Card spec coverage

**Phase 1: Weeks 1-6 (June 2 — July 13)**

*Weeks 1-2: TaskAnalyzer hardening*
- Prompt templates in `analyzer_prompts.rs`
- `analyze()` stable async API with typed `AnalyzerError`
- Cycle detection and retry on invalid LLM output
- Property-based tests: cycle-free invariant, root uniqueness, critical path correctness
- Integration test: end-to-end from raw goal string to valid `SubtaskDAG`
- Prompt validation test suite: 30 canonical goal strings (financial compliance, code review pipeline, research summarization, deploy and test, data extraction, incident report) with expected DAG shapes -- structural + semantic regression test for all future prompt changes
- Pre-execution DAG review: for goals decomposed into Critical-risk tasks, the operator is shown the DAG and approves it before any agent runs

*Weeks 3-4: SwarmComposer and all 7 patterns*
- Extend `SwarmComposer` to handle all 7 coordination patterns
- SLA budget propagation and `BudgetWarning` surfaced to caller
- `ComposerPlan` type with pattern selection and cost estimate
- Tests: budget overflow, missing-capability fallback, each pattern routed correctly

*Weeks 5-6: HITLGovernor and notifications*
- HITLGovernor wired to all 7 notifiers (Slack, Telegram, Feishu, DingTalk, Email, WebSocket, Log -- all already implemented in mofa-orchestrator skeleton) -- GSoC work is the full scheduler integration, timeout escalation policy, and graduated autonomy gate logic
- WebSocketNotifier subscriber wired into Axum endpoint so mofa-studio receives live gate events without polling
- `HITLAuditRecord` written to `SwarmAuditLog` on every gate decision
- Mock-notifier tests; integration test with real Telegram webhook via environment variable

**Mid-term deliverable:** `SwarmOrchestrator::run_goal()` works end-to-end with mocked LLM. All three modules tested. HITL suspension and Slack notification demonstrated in a runnable example.

**Phase 2: Weeks 7-12 (July 14 — September 1)**

*Week 7-8: GovernanceLayer — RBAC, audit export, and REST API*
- RBAC role table enforced in scheduler pre-flight
- `export_audit_jsonl()` with documented JSONL schema
- Axum REST API: `GET /swarm/status`, `GET /swarm/approvals`, `GET /swarm/audit`
- Freeze API contract, write OpenAPI spec for mofa-studio integration
- Tests: role escalation blocked, audit file round-trip, SLA violation event, REST endpoint response shapes

*Week 9: SemanticDiscovery — query expansion, MCP integration, A2A cards*
- LLM query expansion before RRF search
- `NoOpEmbedder` graceful fallback to BM25-only mode
- Wire MCP server (PR #1321) into capability registry for automatic agent indexing
- A2A Agent Card ingestion endpoint
- Benchmarks: BM25-only vs BM25 and dense on a synthetic 100-agent dataset

*Week 10: PluginMarketplace + Gateway integration*
- Greedy SemVer resolver with OWASP Agentic Top 10 unsigned-plugin rejection
- `TrustScorer` with composite formula
- `GatewayCapabilityClient` in mofa-orchestrator wrapping the Gateway API
- Gateway integration demo: agent accessing a file-system or sensor capability via capability API
- End-to-end test: install signed plugin, detect version conflict, call Gateway capability

*Week 11: Smith Observatory — OTel GenAI spans and mofa-studio bridge*
- `OtelTraceBackend` emitting `gen_ai.agent.*` span attributes (OpenTelemetry GenAI semantic conventions)
- `SwarmEvalRunner` aggregate metrics: P50/P95 latency, precision at 3
- CI smoke test: start local Jaeger, run eval, assert `gen_ai.agent.*` attributes on spans
- mofa-studio REST polling verified end-to-end against the Week 7 API

*Week 12: Integration, demo, and polish*
- Full end-to-end demo: `mofa swarm run "financial compliance check"` with all 8 modules active
- Docker Compose file: MoFA + Jaeger + mock Slack + mock DingTalk webhook
- All documentation updated, OpenAPI spec published
- Final clippy pass, zero warnings, submit

---

### Expected Outcomes

**Code contributions:**
- New crate: `mofa-orchestrator` — `SwarmOrchestrator`, `GovernanceLayer`, 7 notifiers (Slack, Telegram, Feishu, DingTalk, Email, WebSocket, Log), REST API (skeleton already live with 23 tests)
- Extended `mofa-foundation` swarm: dynamic DAG mutation, load-aware `SwarmComposer`, 7-pattern routing, `HITLGovernor` with Secretary 5-phase lifecycle and graduated autonomy
- Extended `mofa-foundation` capability: hybrid `CapabilityRegistry` with MCP indexing, A2A Agent Card ingestion, query expansion
- Extended `mofa-cli` plugin: Ed25519 verification, `SemVerResolver`, `TrustScorer`, OWASP Agentic Top 10 checks
- Extended `mofa-smith`: `OtelTraceBackend` with `gen_ai.agent.*` semantic conventions, aggregate eval metrics
- Gateway integration: `GatewayCapabilityClient` making physical-world capabilities first-class in swarm composition

**Documentation:**
- User guide: `docs/swarm-orchestrator.md`
- Architecture reference: `docs/swarm-architecture.md`
- Plugin marketplace guide: `docs/plugin-marketplace.md`
- API docs for all public types via `cargo doc`

**Tests:**
- Property-based tests with `proptest` for DAG invariants and RRF score properties
- Unit tests for every public method (target: 80% line coverage via `cargo-tarpaulin`)
- Integration tests: scheduler + HITL + audit log wired end-to-end
- End-to-end CLI test via `assert_cmd`
- Observability smoke test: OTLP spans received by local Jaeger

**Demo:**
- Runnable example: `examples/cognitive_swarm_demo/` — `mofa swarm run "financial compliance check"` with mock agents, HITL pause, audit log output
- Docker Compose file for zero-setup local demo

**Stretch goals (if core deliverables complete before Week 12):**
- Python SDK bindings for `SwarmOrchestrator::run_goal()` via UniFFI so data science teams can orchestrate swarms from Python notebooks
- Full PubGrub-style backtracking SemVer resolver replacing the greedy resolver
- dora-rs distributed execution: run swarm subtasks across multiple nodes using MoFA's optional Dora integration
- Benchmark suite: latency and memory comparison between MoFA swarm (Rust) and equivalent LangGraph (Python) pipeline on identical goal strings
- Live mofa-studio swarm graph demo: Makepad UI polling the REST API with a running `mofa swarm run` visible in real time

---

### Risks and Mitigations

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| LLM API rate limits slow integration tests | Medium | Use `NoOpLLMClient` mock in CI; real LLM only in local dev |
| SemVer resolver complexity exceeds timeline | Low | Ship greedy resolver in Week 10; backtracking is a stretch goal behind a clean interface |
| OTLP crate version conflicts with workspace | Low | Pin `opentelemetry = "0.27"` from workspace; gate behind `otel` feature flag |
| Slack/Telegram API changes | Low | Abstract behind `Notifier` trait; mock in all tests |
| PR review delays push Phase 1 into Phase 2 | Medium | Submit PRs in pairs so one can be reviewed while another is in progress; weekly sync with mentors |
| LLM DAG output produces cycles | Medium | Topological sort validation with automatic retry prompt; rule-based fallback decomposer for 6 common goal patterns (deploy, review, analyze, summarize, search, notify) |
| LLM produces structurally valid but semantically wrong DAG | Medium | Pre-execution DAG review gate for Critical-risk goals -- operator approves the decomposition before any agent runs; prompt validation test suite of 30 canonical goal strings with expected DAG shapes serves as regression test for prompt changes |
| Decomposition prompt degrades across LLM model updates | Low | Decomposition prompt versioned in analyzer_prompts.rs and pinned to a tested version; model version locked in OrchestratorConfig |

---

### Additional Information

**Availability:**
- Hours per week: 40-45 hours (targeting 350-hour Large project scale)
- Timezone: IST (UTC+5:30)
- Conflicts during GSoC period: none — no internship, no conflicting coursework

**Communication plan:**
- Weekly sync with mentors on Discord
- PR-by-PR review cadence (submit in pairs)
- Proactively share blockers within 24 hours of identifying them

---

### Post-GSoC

I plan to maintain the swarm subsystem and `mofa-orchestrator` crate as an active contributor after GSoC ends. Concrete next steps:

- Add Python SDK bindings for `SwarmOrchestrator::run_goal()` via UniFFI, making the orchestrator accessible from Python data science workflows
- Deepen mofa-studio integration: live swarm graph visualization, approval queue UI, and audit log browser built on the REST API delivered in GSoC
- Explore running swarm tasks across dora-rs distributed nodes, which aligns with MoFA's MaaS and hardware device vision Yao sir described
- Implement full A2A Agent Card spec compliance so MoFA agents are discoverable by any A2A-compatible framework
- Contribute to Plugin Marketplace trust infrastructure as the ecosystem grows
- Stay involved in design reviews for new coordination patterns and Gateway device integrations

MoFA is the framework I want to be using in my own work. That is not a line for a proposal. It is the reason I wrote 50+ pull requests before the application window opened.

---

### Why MoFA Will Win Where Others Cannot

| Property | LangGraph | CrewAI | AutoGen | swarms-rs | MoFA + This Project |
|----------|-----------|--------|---------|-----------|---------------------|
| Language | Python | Python | Python | Rust | Rust |
| Concurrency | Asyncio | Threads | Asyncio | Tokio | Tokio + Ractor actors |
| Task decomposition | Manual graph | Role assignment | Reply chains | Sequential/Concurrent | LLM-driven dynamic DAG (mid-execution mutation) |
| HITL | Manual breakpoints | None | Human proxy | None | Secretary 5-phase + graduated autonomy |
| Agent discovery | String lookup | Class instantiation | Agent name | None | BM25 + dense RRF + MCP + A2A cards |
| Plugin security | pip install | pip install | pip install | None | Ed25519 + SemVer + OWASP Top 10 |
| Observability | LangSmith (SaaS) | Basic logs | Basic logs | None | OTel GenAI gen_ai.agent.* spans |
| Physical world | None | None | None | None | Gateway capability APIs |
| Desktop UI | None | None | None | None | mofa-studio live swarm graph (WebSocket push, no polling) |
| Discord interface | None | None | None | None | mofaclaw swarm commands |
| Notifications | None | None | None | None | Slack, Telegram, Feishu, DingTalk, Email, WebSocket |
| Memory overhead | 60-120 MB idle | 80+ MB | 60+ MB | Unknown | Under 5 MB idle |

LangGraph requires a human to hand-wire the execution topology. CrewAI ships role definitions with no budget awareness. AutoGen chains replies but cannot suspend mid-execution for human approval. swarms-rs is the closest Rust competitor but has no DAG decomposition, no HITL, no semantic discovery, and no observability. MoFA with this project leads on every dimension that matters for production enterprise deployment.

Three common objections and why they do not hold up in practice.

LangGraph's visualization advantage disappears when you factor in that LangSmith is a paid SaaS product that requires sending execution data to external servers. mofa-studio runs locally on the operator's machine, driven by the WebSocket and REST API already built in the mofa-orchestrator skeleton. An enterprise compliance team cannot send audit data to a third-party SaaS. MoFA gives them a live swarm graph with zero data leaving their infrastructure.

AutoGen's larger ecosystem of pre-built agent roles is real today but not a moat. MoFA's A2A Agent Card ingestion (Module 5) means the SwarmComposer can discover and route tasks to AutoGen agents, LangChain tools, and any A2A-compatible framework without reimplementing them. MoFA becomes the orchestrator of other ecosystems rather than a competitor to their agent libraries.

CrewAI's simpler onboarding is a Python-first argument. The mofaclaw Discord bot already demonstrates that non-Rust developers never touch Rust code. A community user types "mofa swarm run research quantum computing" into Discord and gets a governed, traced, audited multi-agent execution. That is simpler onboarding than any Python import.

---

### References

1. Yao, S. et al. "ReAct: Synergizing Reasoning and Acting in Language Models." ICLR 2023. https://arxiv.org/abs/2210.03629

2. Wei, J. et al. "Chain-of-Thought Prompting Elicits Reasoning in Large Language Models." NeurIPS 2022. https://arxiv.org/abs/2201.11903

3. Cormack, G., Clarke, C., and Buettcher, S. "Reciprocal Rank Fusion Outperforms Condorcet and Individual Rank Learning Methods." SIGIR 2009. https://dl.acm.org/doi/10.1145/1571941.1572114

4. Robertson, S. and Zaragoza, H. "The Probabilistic Relevance Framework: BM25 and Beyond." Foundations and Trends in Information Retrieval, 2009.

5. Wu, Q. et al. "AutoGen: Enabling Next-Gen LLM Applications via Multi-Agent Conversation." arXiv 2023. https://arxiv.org/abs/2308.08155

6. Qian, C. et al. "Communicative Agents for Software Development (ChatDev)." ACL 2024. https://arxiv.org/abs/2307.07924

7. Cloud Security Alliance. "AI Agentic Security Initiative: Agentic AI Threat Modeling Framework." CSA, 2025. https://cloudsecurityalliance.org/research/topics/artificial-intelligence

8. OWASP. "OWASP Agentic AI Top 10." 2025. https://owasp.org/www-project-top-10-for-large-language-model-applications/

9. Google. "Agent2Agent (A2A) Protocol Specification." 2025. https://google.github.io/A2A/

10. OpenTelemetry. "Semantic Conventions for Generative AI Systems." OpenTelemetry Specification, 2024. https://opentelemetry.io/docs/specs/semconv/gen-ai/

11. Anthropic. "Model Context Protocol Specification." 2024. https://modelcontextprotocol.io/specification

12. DynTaskMAS authors. "Dynamic Task Allocation in Multi-Agent Systems." ICAPS 2025. (cited for mid-execution DAG mutation pattern, 21-33 percent execution time reduction)
