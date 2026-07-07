"""Recovery manager that applies retry and fallback strategies."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Protocol, Sequence


class SupportsName(Protocol):
    """Protocol used by recovery logic to identify agents."""

    name: str


@dataclass(slots=True)
class RecoveryAction:
    """Represents one recovery action taken by the manager."""

    action_type: str
    from_agent: str
    to_agent: str
    detail: str


@dataclass(slots=True)
class RecoveryManager:
    """Coordinates retries and fallback routing for failed executions."""

    max_retries: int = 1
    actions: list[RecoveryAction] = field(default_factory=list)

    def recover(
        self,
        run_attempt: Callable[[SupportsName, str], Any],
        primary_agent: SupportsName,
        fallback_agents: Sequence[SupportsName],
        last_result: Any,
    ) -> Any:
        result = last_result

        for retry_index in range(1, self.max_retries + 1):
            detail = f"retry #{retry_index} for {primary_agent.name}"
            self.actions.append(
                RecoveryAction(
                    action_type="retry",
                    from_agent=primary_agent.name,
                    to_agent=primary_agent.name,
                    detail=detail,
                )
            )
            result = run_attempt(primary_agent, detail)
            if getattr(result, "succeeded", False):
                return result

        for fallback in fallback_agents:
            detail = f"fallback route from {primary_agent.name} to {fallback.name}"
            self.actions.append(
                RecoveryAction(
                    action_type="fallback",
                    from_agent=primary_agent.name,
                    to_agent=fallback.name,
                    detail=detail,
                )
            )
            result = run_attempt(fallback, detail)
            if getattr(result, "succeeded", False):
                return result

        self.actions.append(
            RecoveryAction(
                action_type="exhausted",
                from_agent=primary_agent.name,
                to_agent=primary_agent.name,
                detail="Recovery exhausted all retries and fallbacks",
            )
        )
        return result
