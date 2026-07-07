"""Supervisor that wraps agent execution with monitoring and self-healing."""

from __future__ import annotations

import logging
import time
from concurrent.futures import ThreadPoolExecutor, TimeoutError
from dataclasses import dataclass
from typing import Any, Protocol, Sequence

from .evaluator import EvaluationResult, OutputEvaluator
from .recovery import RecoveryManager


class AgentLike(Protocol):
    """Protocol that all supervised agents should satisfy."""

    name: str

    def run(self, task: str, context: dict[str, Any]) -> str:
        """Executes one unit of agent work."""


@dataclass(slots=True)
class AgentExecutionResult:
    """Execution metadata produced by the supervisor for each agent call."""

    step_name: str
    agent_name: str
    status: str
    attempt: int
    duration_seconds: float
    output: str
    error: str
    evaluation: EvaluationResult
    recovery_trigger: str

    @property
    def succeeded(self) -> bool:
        return self.status == "success" and self.evaluation.passed


class AgentSupervisor:
    """Monitors workflow steps and applies quality-aware recovery policies."""

    def __init__(
        self,
        evaluator: OutputEvaluator | None = None,
        recovery_manager: RecoveryManager | None = None,
        logger: logging.Logger | None = None,
    ) -> None:
        self.evaluator = evaluator or OutputEvaluator()
        self.recovery_manager = recovery_manager or RecoveryManager()
        self.logger = logger or logging.getLogger("agent_supervisor")
        self.execution_history: list[AgentExecutionResult] = []

    def execute(
        self,
        step_name: str,
        agent: AgentLike,
        task: str,
        context: dict[str, Any] | None = None,
        fallback_agents: Sequence[AgentLike] | None = None,
        timeout_seconds: float = 2.0,
    ) -> AgentExecutionResult:
        """Runs one workflow step with supervision and automated recovery."""
        effective_context = dict(context or {})
        fallback_agents = fallback_agents or []

        self.logger.info("[START] step=%s agent=%s", step_name, agent.name)

        attempt_counter = 0

        def run_attempt(current_agent: AgentLike, recovery_trigger: str) -> AgentExecutionResult:
            nonlocal attempt_counter
            attempt_counter += 1
            result = self._run_once(
                step_name=step_name,
                agent=current_agent,
                task=task,
                context=effective_context,
                timeout_seconds=timeout_seconds,
                attempt=attempt_counter,
                recovery_trigger=recovery_trigger,
            )
            self.execution_history.append(result)
            self.logger.info(
                "[END] step=%s agent=%s attempt=%s status=%s score=%.3f trigger=%s",
                result.step_name,
                result.agent_name,
                result.attempt,
                result.status,
                result.evaluation.score,
                result.recovery_trigger,
            )
            return result

        first_result = run_attempt(agent, "initial")
        if first_result.succeeded:
            return first_result

        recovered = self.recovery_manager.recover(
            run_attempt=run_attempt,
            primary_agent=agent,
            fallback_agents=fallback_agents,
            last_result=first_result,
        )
        return recovered

    def _run_once(
        self,
        step_name: str,
        agent: AgentLike,
        task: str,
        context: dict[str, Any],
        timeout_seconds: float,
        attempt: int,
        recovery_trigger: str,
    ) -> AgentExecutionResult:
        started = time.perf_counter()
        status = "failure"
        output = ""
        error = ""

        with ThreadPoolExecutor(max_workers=1) as executor:
            future = executor.submit(agent.run, task, context)
            try:
                output = future.result(timeout=timeout_seconds)
                status = "success"
            except TimeoutError:
                future.cancel()
                status = "timeout"
                error = f"Execution exceeded {timeout_seconds:.2f}s"
            except Exception as exc:  # noqa: BLE001
                status = "failure"
                error = str(exc)

        duration = time.perf_counter() - started
        evaluation = self.evaluator.evaluate(output if status == "success" else error)

        # A successful call can still fail quality checks and trigger recovery.
        if status == "success" and not evaluation.passed:
            status = "failure"
            error = f"Quality check failed: {evaluation.reason}"

        return AgentExecutionResult(
            step_name=step_name,
            agent_name=agent.name,
            status=status,
            attempt=attempt,
            duration_seconds=round(duration, 4),
            output=output,
            error=error,
            evaluation=evaluation,
            recovery_trigger=recovery_trigger,
        )
