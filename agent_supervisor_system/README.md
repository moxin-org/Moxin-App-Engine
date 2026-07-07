# Agent Supervisor Module (Python)

A modular example of an **Agent Supervisor** for monitoring and self-healing in multi-agent workflows.

## Purpose

This project demonstrates how to supervise agent execution and automatically recover from:
- runtime failures
- timeouts
- low-quality outputs

The supervisor wraps each agent call, evaluates quality, then applies retry/fallback strategies when needed.

## Project Structure

```text
agent_supervisor_system/
  supervisor/
    supervisor.py      # Execution wrapper + monitoring
    evaluator.py       # Output quality checks
    recovery.py        # Retry/fallback strategy
  agents/
    research_agent.py
    search_agent.py
    writer_agent.py
    critic_agent.py
  workflow/
    workflow_runner.py # Sequential workflow orchestration
  examples/
    demo_workflow.py   # CLI demo script
  tests/
    test_evaluator.py
    test_supervisor_recovery.py
    test_workflow_runner.py
```

## Architecture

### 1) Supervisor Module
- Tracks each execution attempt with metadata:
  - step name
  - agent name
  - status (`success`, `failure`, `timeout`)
  - quality score
  - trigger (`initial`, `retry`, `fallback`)
- Stores history for full workflow observability.
- Emits logging for start/end of each attempt.

### 2) Output Evaluator
- Detects invalid responses:
  - empty output
  - obvious error text (`error`, `exception`, `traceback`)
  - low-information responses
- Produces a score and pass/fail decision.

### 3) Recovery Manager
- Retries a failed primary agent (`max_retries`).
- If retries fail, routes to fallback agents.
- Logs recovery actions (`retry`, `fallback`, `exhausted`).

### 4) Workflow Runner
Runs a simple pipeline:
1. Research Agent
2. Search Agent
3. Writer Agent
4. Critic Agent

Each step is executed through the supervisor, not called directly.

## Demo CLI

From the repository root:

```bash
python agent_supervisor_system/examples/demo_workflow.py --topic "AI reliability" --scenario retry
```

Scenarios:
- `clean`: no simulated failures
- `retry`: a step fails quality once, then succeeds on retry
- `fallback`: primary agent fails and fallback agent is used
- `timeout`: timeout on first attempt, then recovery succeeds

The script prints:
- workflow outputs
- per-attempt execution history
- recovery actions

## Run Tests

```bash
python -m unittest discover agent_supervisor_system/tests
```

## Integration Notes

To integrate this pattern into a larger framework:
- Keep your framework-specific agent classes; ensure they expose a `name` and `run(task, context)` method.
- Replace or extend `OutputEvaluator` with domain-specific quality metrics.
- Extend `RecoveryManager` to support advanced policies (circuit breakers, dynamic routing, escalation).
