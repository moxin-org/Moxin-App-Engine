# mofa-orchestrator

The connective tissue of the MoFA cognitive swarm. Takes a plain English goal, decomposes it into a risk-annotated SubtaskDAG, assigns agents via load-aware scoring, enforces governance, gates high-risk steps through a human-in-the-loop workflow, notifies stakeholders across 7 channels, and produces a full audit trail.

## Quick start

```rust
use mofa_orchestrator::{SwarmOrchestrator, OrchestratorConfig};

let config = OrchestratorConfig::default();
let orchestrator = SwarmOrchestrator::new(config);
let report = orchestrator.run_goal("review Q1 loan applications for fair lending violations").await?;
println!("{}", report.summary);
```

## Notification channels

| Channel | Struct | Transport |
|---------|--------|-----------|
| Slack | `SlackNotifier` | Incoming Webhook |
| Telegram | `TelegramNotifier` | Bot API |
| DingTalk | `DingTalkNotifier` | Custom Robot Webhook |
| Feishu | `FeishuNotifier` | Lark Bot API |
| Email | `EmailNotifier` | SendGrid v3 |
| WebSocket | `WebSocketNotifier` | Axum broadcast (mofa-studio) |
| Log | `LogNotifier` | tracing::info! |

## Governance

```rust
use mofa_orchestrator::{GovernanceLayer, Role};

let gov = GovernanceLayer::new();
gov.check_permission(user_id, Role::Operator)?;
gov.export_audit_jsonl("audit.jsonl").await?;
```

## Architecture

See [docs/architecture.md](../../docs/architecture.md) for the full ecosystem diagram.

This crate is the GSoC 2026 Idea 5 skeleton implementation. See the [proposal](../../gsoc2026-proposal-cognitive-swarm-orchestrator.md) for the full design.
