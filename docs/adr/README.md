# Architecture Decision Records (ADRs)

This directory records architecture decisions for the MoFA project.

## Format

Each ADR is a markdown file named with a sequential number and a short title, e.g.:

- `001-dual-layer-plugin-architecture.md`
- `002-reasoning-strategy-abstraction.md`

## Template

```markdown
# ADR NNN: Short Title

* **Status**: Proposed | Accepted | Rejected | Deprecated | Superseded by [NNN](NNN-title.md)
* **Date**: YYYY-MM-DD
* **Author(s)**: Your Name (@github-handle)

## Context and Problem Statement

Describe the problem or decision being addressed. What is changed? What is the driving requirement?

## Considered Options

Option 1: Description...
Option 2: Description...

## Decision Outcome

Chosen option: **Option Name**

Explain why this option was selected over others.

### Positive Consequences

- What good things does this enable?
- Simplifies what?

### Negative Consequences

- What downsides or trade-offs were accepted?
- What increased complexity?
```

## How to Contribute

1. Propose a new ADR by copying the template above.
2. Discuss on GitHub Issues or Discord to gather feedback.
3. Update status to `Accepted` once consensus is reached.
4. Keep ADRs up to date when decisions evolve.
