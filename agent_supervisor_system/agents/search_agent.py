"""Search agent implementation."""

from __future__ import annotations

from typing import Any

from .base_agent import BaseAgent


class SearchAgent(BaseAgent):
    """Produces searchable evidence snippets for downstream composition."""

    def __init__(self) -> None:
        super().__init__(name="search_agent")

    def run(self, task: str, context: dict[str, Any]) -> str:
        simulated = self._simulate_failure_mode(context)
        if simulated is not None:
            return simulated

        return (
            "Search findings: source A confirms market direction; source B shows risks; "
            "source C provides measurable KPIs for validation."
        )
