"""CLI demo for supervised multi-agent workflow execution."""

from __future__ import annotations

import argparse
import logging
import pathlib
import sys
from pprint import pprint

PROJECT_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from supervisor import AgentSupervisor, OutputEvaluator, RecoveryManager
from workflow import WorkflowRunner


def build_context(scenario: str) -> dict[str, object]:
    if scenario == "clean":
        return {"failure_modes": {}}

    if scenario == "retry":
        return {"failure_modes": {"writer_agent": ["empty", "ok"]}}

    if scenario == "fallback":
        return {"failure_modes": {"research_agent": ["error", "error"]}}

    if scenario == "timeout":
        return {
            "failure_modes": {"search_agent": ["timeout", "ok"]},
            "timeout_sleep_seconds": 2.0,
        }

    raise ValueError(f"Unsupported scenario: {scenario}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Run a supervised multi-agent workflow demo")
    parser.add_argument("--topic", default="AI governance", help="Topic for the workflow")
    parser.add_argument(
        "--scenario",
        choices=["clean", "retry", "fallback", "timeout"],
        default="retry",
        help="Failure simulation scenario",
    )
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s | %(levelname)s | %(name)s | %(message)s",
    )

    supervisor = AgentSupervisor(
        evaluator=OutputEvaluator(minimum_score=0.6),
        recovery_manager=RecoveryManager(max_retries=1),
    )
    runner = WorkflowRunner(supervisor=supervisor)

    context = build_context(args.scenario)
    result = runner.run(topic=args.topic, context=context)

    print("\n=== Workflow Outputs ===")
    pprint(result.outputs)

    print("\n=== Step History ===")
    for step in result.steps:
        print(
            f"step={step.step_name:<8} agent={step.agent_name:<14} "
            f"attempt={step.attempt:<2} status={step.status:<8} "
            f"score={step.evaluation.score:<4} trigger={step.recovery_trigger}"
        )

    print("\n=== Recovery Actions ===")
    for action in supervisor.recovery_manager.actions:
        print(
            f"action={action.action_type:<9} from={action.from_agent:<14} "
            f"to={action.to_agent:<14} detail={action.detail}"
        )


if __name__ == "__main__":
    main()
