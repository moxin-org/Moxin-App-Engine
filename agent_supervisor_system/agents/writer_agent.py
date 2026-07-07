"""Writer agent implementation."""

from __future__ import annotations

from typing import Any

from .base_agent import BaseAgent


class WriterAgent(BaseAgent):
    """Combines research + evidence into a readable draft."""

    def __init__(self) -> None:
        super().__init__(name="writer_agent")

    def run(self, task: str, context: dict[str, Any]) -> str:
        simulated = self._simulate_failure_mode(context)
        if simulated is not None:
            return simulated

        research = context.get("research", "No research")
        search = context.get("search", "No search insights")
        return (
            "Draft article: "
            f"{research} "
            f"Evidence summary: {search}. "
            "Conclusion: recommend phased rollout with weekly quality checks."
        )
