"""Output quality evaluation for agent responses."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(slots=True)
class EvaluationResult:
    """Represents quality evaluation metadata for an agent output."""

    passed: bool
    score: float
    reason: str


class OutputEvaluator:
    """Evaluates agent responses and flags low-quality outputs."""

    def __init__(self, minimum_score: float = 0.6) -> None:
        self.minimum_score = minimum_score

    def evaluate(self, output: str | None) -> EvaluationResult:
        if output is None:
            return EvaluationResult(passed=False, score=0.0, reason="Output is None")

        normalized = output.strip()
        if not normalized:
            return EvaluationResult(passed=False, score=0.0, reason="Output is empty")

        lowered = normalized.lower()
        if "error" in lowered or "exception" in lowered or "traceback" in lowered:
            return EvaluationResult(
                passed=False,
                score=0.1,
                reason="Output appears to contain an error message",
            )

        # Light-weight heuristic: short answers are penalized, but practical summaries can pass.
        length_score = min(len(normalized) / 100.0, 1.0)
        richness_score = 1.0 if any(ch in normalized for ch in [".", ":", "-"]) else 0.7
        score = round((length_score * 0.7) + (richness_score * 0.3), 3)
        passed = score >= self.minimum_score

        reason = "Output passed quality threshold" if passed else "Output quality is below threshold"
        return EvaluationResult(passed=passed, score=score, reason=reason)
