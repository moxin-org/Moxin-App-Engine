"""Base behavior for demo agents."""

from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Any


@dataclass(slots=True)
class BaseAgent:
    """Minimal base class that supports deterministic failure simulations."""

    name: str

    def _get_mode(self, context: dict[str, Any]) -> str:
        failure_modes = context.setdefault("failure_modes", {})
        modes_for_agent = failure_modes.get(self.name, [])
        if modes_for_agent:
            return modes_for_agent.pop(0)
        return "ok"

    def _simulate_failure_mode(self, context: dict[str, Any]) -> str | None:
        mode = self._get_mode(context)
        if mode == "error":
            raise RuntimeError(f"{self.name} simulated failure")
        if mode == "empty":
            return ""
        if mode == "low":
            return "ok"
        if mode == "timeout":
            time.sleep(float(context.get("timeout_sleep_seconds", 3.0)))
            return "Timed out response"
        return None
