"""Tests for output evaluation heuristics."""

from __future__ import annotations

import pathlib
import sys
import unittest

PROJECT_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from supervisor import OutputEvaluator


class OutputEvaluatorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.evaluator = OutputEvaluator(minimum_score=0.6)

    def test_empty_output_fails(self) -> None:
        result = self.evaluator.evaluate("   ")
        self.assertFalse(result.passed)
        self.assertEqual(result.score, 0.0)

    def test_error_keyword_fails(self) -> None:
        result = self.evaluator.evaluate("Traceback: error occurred")
        self.assertFalse(result.passed)
        self.assertLess(result.score, 0.6)

    def test_rich_output_passes(self) -> None:
        result = self.evaluator.evaluate(
            "Detailed summary: includes evidence, metrics, and an actionable plan."
        )
        self.assertTrue(result.passed)
        self.assertGreaterEqual(result.score, 0.6)


if __name__ == "__main__":
    unittest.main()
