"""Tests for supervisor retry and fallback behavior."""

from __future__ import annotations

import pathlib
import sys
import unittest
from dataclasses import dataclass
from typing import Any

PROJECT_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from supervisor import AgentSupervisor, OutputEvaluator, RecoveryManager


@dataclass(slots=True)
class SequenceAgent:
    name: str
    outputs: list[str]

    def run(self, task: str, context: dict[str, Any]) -> str:
        if not self.outputs:
            return "default successful output with enough details."
        current = self.outputs.pop(0)
        if current == "RAISE":
            raise RuntimeError(f"{self.name} failed")
        return current


class SupervisorRecoveryTests(unittest.TestCase):
    def test_retry_recovers_after_quality_failure(self) -> None:
        supervisor = AgentSupervisor(
            evaluator=OutputEvaluator(minimum_score=0.6),
            recovery_manager=RecoveryManager(max_retries=1),
        )
        flaky = SequenceAgent(
            name="writer_agent",
            outputs=[
                "",
                "This retry output is detailed enough because it includes context, evidence, and a concrete recommendation.",
            ],
        )

        result = supervisor.execute(
            step_name="writer",
            agent=flaky,
            task="Write",
            context={},
            fallback_agents=[],
            timeout_seconds=1.0,
        )

        self.assertTrue(result.succeeded)
        self.assertEqual(result.attempt, 2)
        self.assertEqual(supervisor.recovery_manager.actions[0].action_type, "retry")

    def test_fallback_executes_when_primary_keeps_failing(self) -> None:
        supervisor = AgentSupervisor(
            evaluator=OutputEvaluator(minimum_score=0.6),
            recovery_manager=RecoveryManager(max_retries=1),
        )
        primary = SequenceAgent(name="research_agent", outputs=["RAISE", "RAISE"])
        fallback = SequenceAgent(
            name="search_agent",
            outputs=["Fallback output with evidence and complete explanation."],
        )

        result = supervisor.execute(
            step_name="research",
            agent=primary,
            task="Research",
            context={},
            fallback_agents=[fallback],
            timeout_seconds=1.0,
        )

        self.assertTrue(result.succeeded)
        self.assertEqual(result.agent_name, "search_agent")
        action_types = [action.action_type for action in supervisor.recovery_manager.actions]
        self.assertIn("fallback", action_types)


if __name__ == "__main__":
    unittest.main()
