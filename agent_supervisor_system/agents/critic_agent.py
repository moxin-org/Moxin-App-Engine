"""Critic agent implementation."""

from __future__ import annotations

from typing import Any

from .base_agent import BaseAgent


class CriticAgent(BaseAgent):
    """Reviews a draft and provides quality feedback."""

    def __init__(self) -> None:
        super().__init__(name="critic_agent")

    def run(self, task: str, context: dict[str, Any]) -> str:
        simulated = self._simulate_failure_mode(context)
        if simulated is not None:
            return simulated

        draft = context.get("draft", "")
        if len(draft) < 60:
            return "Critique: draft is too short; add concrete evidence and risk analysis."

        return (
            "Critique: draft quality is acceptable, evidence coverage is strong, "
            "and recommendations are actionable with minor wording refinements."
        )
