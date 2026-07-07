"""Sequential multi-agent workflow orchestrated by the Agent Supervisor."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from agents import CriticAgent, ResearchAgent, SearchAgent, WriterAgent
from supervisor import AgentExecutionResult, AgentSupervisor


@dataclass(slots=True)
class WorkflowResult:
    """Aggregated outputs and execution details for the full workflow."""

    outputs: dict[str, str]
    steps: list[AgentExecutionResult]


class WorkflowRunner:
    """Runs Research -> Search -> Writer -> Critic with supervisor monitoring."""

    def __init__(self, supervisor: AgentSupervisor | None = None) -> None:
        self.supervisor = supervisor or AgentSupervisor()
        self.research_agent = ResearchAgent()
        self.search_agent = SearchAgent()
        self.writer_agent = WriterAgent()
        self.critic_agent = CriticAgent()

    def run(self, topic: str, context: dict[str, Any] | None = None) -> WorkflowResult:
        context = dict(context or {})
        context["topic"] = topic

        outputs: dict[str, str] = {}

        research_result = self.supervisor.execute(
            step_name="research",
            agent=self.research_agent,
            task=f"Collect research for {topic}",
            context=context,
            fallback_agents=[self.search_agent],
            timeout_seconds=1.0,
        )
        outputs["research"] = research_result.output
        context["research"] = research_result.output

        search_result = self.supervisor.execute(
            step_name="search",
            agent=self.search_agent,
            task=f"Search references for {topic}",
            context=context,
            fallback_agents=[self.research_agent],
            timeout_seconds=1.0,
        )
        outputs["search"] = search_result.output
        context["search"] = search_result.output

        writer_result = self.supervisor.execute(
            step_name="writer",
            agent=self.writer_agent,
            task=f"Write a concise brief for {topic}",
            context=context,
            fallback_agents=[self.research_agent],
            timeout_seconds=1.0,
        )
        outputs["draft"] = writer_result.output
        context["draft"] = writer_result.output

        critic_result = self.supervisor.execute(
            step_name="critic",
            agent=self.critic_agent,
            task="Review the draft quality",
            context=context,
            fallback_agents=[self.writer_agent],
            timeout_seconds=1.0,
        )
        outputs["critique"] = critic_result.output

        return WorkflowResult(outputs=outputs, steps=list(self.supervisor.execution_history))
