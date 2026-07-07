# ADR 001: Dual-Layer Plugin Architecture

* **Status**: Accepted
* **Date**: 2025-04-08 (or current)
* **Author(s)**: MoFA Core Team

## Context and Problem Statement

MoFA aims to provide an extensible AI agent framework that balances performance with runtime flexibility. A key design question is how to allow users to extend functionality (tools, memory, reasoning, etc.) without sacrificing the performance of core operations, while also enabling dynamic changes without recompilation.

Key requirements:
- Zero-cost abstractions for performance-critical paths (LLM inference, data processing).
- Runtime programmability for business logic and workflow orchestration.
- Hot-reload capability for certain plugins.
- Support for multiple language bindings (Python, Java, Go, Swift) via UniFFI.

## Considered Options

1. **Single-layer plugin system (Rust traits only):**
   - Define all plugin points as Rust traits.
   - Users implement traits and compile as native Rust.
   - Pros: Zero-cost, strong typing, simple.
   - Cons: No dynamic loading without recompilation, unsuitable for rapid iteration, excludes non-Rust users.

2. **Single-layer scripting (e.g., Rhai only):**
   - All plugins are Rhai scripts loaded at runtime.
   - Pros: Extremely flexible, hot-reloadable, easy for non-Rust users.
   - Cons: Performance overhead for complex operations, limited access to native Rust ecosystem.

3. **Dual-layer (Rust/WASM + scripting):**
   - Compile-time layer: Rust/WASM plugins for performance-critical components.
   - Runtime layer: Rhai scripts for dynamic business logic.
   - Both layers implement the same kernel trait interfaces and can be composed.
   - Pros: Best of both worlds; performance where needed, flexibility where appropriate.
   - Cons: Slightly higher initial complexity; need to clearly document what belongs where.

## Decision Outcome

Chosen option: **Dual-layer (Rust/WASM + scripting)**.

This design aligns with MoFA's microkernel architecture: the kernel (mofa-kernel) defines trait interfaces; the compile-time layer (Rust) provides high-performance implementations; the runtime layer (Rhai) allows dynamic reconfiguration without recompilation. The two layers interoperate through the same unified API, giving users the freedom to choose the appropriate implementation technology per use case.

### Positive Consequences

- Performance-critical code (LLM adapters, vector stores) can be native Rust/WASM.
- Workflow orchestration, rules, and dynamic behavior can be Rhai scripts hot-reloaded at runtime.
- Clear separation of concerns and deployment models.
- Enables future WASM sandboxing for secure third-party plugins.

### Negative Consequences

- Need to maintain both Rust and Rhai integration points.
- Users must learn two extension mechanisms (though they share the same interface).
- Potential for misuse: placing performance-sensitive logic in Rhai where it doesn’t belong.
- Slightly increased testing surface.
