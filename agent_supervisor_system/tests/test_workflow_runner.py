"""Integration-style test for the full supervised workflow."""

from __future__ import annotations

import pathlib
import sys
import unittest

PROJECT_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from workflow import WorkflowRunner


class WorkflowRunnerTests(unittest.TestCase):
    def test_workflow_completes_with_recovery(self) -> None:
        runner = WorkflowRunner()
        context = {"failure_modes": {"writer_agent": ["empty", "ok"]}}

        result = runner.run(topic="resilient systems", context=context)

        self.assertIn("research", result.outputs)
        self.assertIn("search", result.outputs)
        self.assertIn("draft", result.outputs)
        self.assertIn("critique", result.outputs)

        writer_steps = [step for step in result.steps if step.step_name == "writer"]
        self.assertGreaterEqual(len(writer_steps), 2)
        self.assertTrue(any(step.recovery_trigger.startswith("retry") for step in writer_steps))


if __name__ == "__main__":
    unittest.main()
