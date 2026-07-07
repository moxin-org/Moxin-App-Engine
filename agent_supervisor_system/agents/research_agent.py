"""Research agent implementation."""

from __future__ import annotations

from typing import Any

from .base_agent import BaseAgent


class ResearchAgent(BaseAgent):
    """Builds a compact research brief for a topic."""

    def __init__(self) -> None:
        super().__init__(name="research_agent")

    def run(self, task: str, context: dict[str, Any]) -> str:
        simulated = self._simulate_failure_mode(context)
        if simulated is not None:
            return simulated

        topic = context.get("topic", task)
        return (
            f"Research brief for {topic}: key trends, historical context, "
            "and major constraints collected from trusted references."
        )
