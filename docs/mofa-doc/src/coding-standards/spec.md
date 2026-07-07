# Rust Project Development Standards

Based on issues discovered during code reviews, the following general development standards have been compiled:

---

## I. Error Handling Standards

### 1. Unified Error System

- **MUST** define a unified error type in the crate root (e.g., `KernelError`)
- **MUST** establish a clear error hierarchy where module errors can be unified through the `From` trait
- **MUST NOT** use `anyhow::Result` as a public API return type in library code; use `thiserror` to define typed errors
- **MUST NOT** implement blanket `From<anyhow::Error>` for error types, as this erases structured error information

### 2. Error Type Design

```rust
// Recommended: Use thiserror to define clear error enums
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KernelError {
    #[error("Agent error: {0}")]
    Agent(#[from] AgentError),
    #[error("Config error: {0}")]
    Config(#[from] ConfigError),
    // ...
}
```

---

## II. Type Design and API Stability

### 1. Enum Extensibility

- **MUST** add the `#[non_exhaustive]` attribute to public enums to ensure backward compatibility
- **MUST** treat foundation-facing public enums as forward-compatible API surface; additive variants should not require downstream exhaustive-match rewrites

```rust
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AgentState {
    Idle,
    Running,
    // New variants can be safely added in the future
}
```

### 2. Derive Trait Standards

- Comparable/testable types **MUST** derive `PartialEq`, `Eq`
- Debug output types **MUST** derive `Debug`; for fields that cannot be auto-derived, implement manually
- Serializable types **MUST** derive `Clone` (unless there's a special reason not to)

---

## III. Naming and Module Design

### 1. Naming Uniqueness

- **MUST NOT** define types with the same name representing different concepts within the same crate
- Checklist: `AgentConfig`, `AgentEvent`, `TaskPriority`, and other core type names

### 2. Module Export Control

- **MUST** use `pub(crate)` to limit internal module visibility
- **MUST** carefully design the public API surface through `lib.rs` or `prelude`
- **MUST NOT** directly `pub mod` export all modules

```rust
// Recommended lib.rs structure
pub mod error;
pub mod agent;
pub use error::KernelError;
pub use agent::{Agent, AgentContext};
mod internal; // Internal implementation
```

### 3. Prelude Design

- **SHOULD** provide a crate-level prelude module that aggregates commonly used types

```rust
// src/prelude.rs
pub use crate::error::KernelError;
pub use crate::agent::{Agent, AgentContext, AgentState};
// ...
```

---

## IV. Performance and Dependency Management

### 1. Async Features

- In Rust 1.75+ environments, **SHOULD** use native `async fn in trait` instead of `#[async_trait]`
- Only use `async` on methods that genuinely require async; synchronous operations should not be marked as async

### 2. Avoid Repeated Computation

- Objects with high compilation costs like regular expressions **MUST** be cached using `LazyLock` or `OnceLock`

```rust
use std::sync::LazyLock;
static ENV_VAR_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{([^}]+)\}").unwrap()
});
```

### 3. Timestamp Handling

- Timestamp generation logic **MUST** be abstracted into a single utility function
- **SHOULD** provide an injectable clock abstraction for testing

```rust
pub trait Clock: Send + Sync {
    fn now_millis(&self) -> u64;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
```

### 4. Avoid Reinventing the Wheel

- **MUST NOT** hand-write logic for Base64, encryption algorithms, etc. that have mature implementations
- Prioritize using widely validated community crates

---

## V. Type Safety

### 1. Reduce Dynamic Type Usage

- **MUST NOT** abuse `serde_json::Value` in scenarios where generic constraints can be used
- **AVOID** using `Box<dyn Any + Send + Sync>` as generic storage; prefer generics or trait objects with specific traits

### 2. Mutability and Interface Consistency

- The choice between `&self` and `&mut self` in trait method signatures **MUST** be consistent
- If internal state modification through `&self` is needed (e.g., `Arc<RwLock<_>>`), document side effects clearly

---

## VI. Interface Consistency

### 1. Parameter Type Conventions

- Constructor parameter types **SHOULD** be unified: prefer `impl Into<String>` or `&str`

```rust
// Recommended
pub fn new(id: impl Into<String>) -> Self { ... }
// Avoid
pub fn new(id: String) -> Self { ... }
```

### 2. Builder Pattern Validation

- Builder methods **MUST** validate invalid input or return `Result`

```rust
pub fn with_weight(mut self, weight: f64) -> Result<Self, &'static str> {
    if weight < 0.0 {
        return Err("Weight must be non-negative");
    }
    self.weight = Some(weight);
    Ok(self)
}
```

### 3. Naming Conventions

- **MUST NOT** create custom method names that conflict with standard trait method names (e.g., `to_string_output` vs `to_string`)

---

## VII. Code Correctness

### 1. Manual Ord/Eq Implementation

- **MUST** write complete tests covering all branches for manually implemented `Ord` trait
- Recommend using `derive` or simplified implementations based on discriminants

### 2. Type Conversion Safety

- Numeric type conversions **MUST** explicitly handle potential overflow

```rust
// Avoid
let ts = as_millis() as u64;
// Recommended
let ts = u64::try_from(as_millis()).unwrap_or(u64::MAX);
```

---

## VIII. Serialization and Compatibility

### 1. Message Protocol Versioning

- Binary serialization **MUST** include version identifiers

```rust
#[derive(Serialize, Deserialize)]
struct MessageEnvelope {
    version: u8,
    payload: Vec<u8>,
}
```

### 2. Serialization Abstraction

- Message buses **SHOULD** support pluggable serialization backends

```rust
pub trait Serializer: Send + Sync {
    fn serialize<T: Serialize>(&self, value: &T) -> Result<Vec<u8>>;
    fn deserialize<T: DeserializeOwned>(&self, data: &[u8]) -> Result<T>;
}
```

---

## IX. Testing Standards

### 1. Test Coverage

- **MUST** include: boundary values, null values, invalid input, concurrent scenarios
- **MUST NOT** only test the happy path

### 2. Unit Tests and Integration Tests

- **MUST** write unit tests for core logic
- **SHOULD** write integration tests for inter-module interactions

### 3. Testability Design

- External dependencies (clock, random numbers, network) **MUST** be injectable through traits for mock implementations

---

## X. Feature Isolation

### 1. Feature Flag Standards

- Dependencies behind feature gates **MUST** be marked with `optional = true` in `Cargo.toml`
- **MUST NOT** feature gate partial code while the dependency is still compiled unconditionally

```toml
[dependencies]
config = { version = "0.14", optional = true }

[features]
default = []
config-loader = ["dep:config"]
```

---

## Checklist Template

| Check Item | Requirement | Status |
|------------|-------------|--------|
| Public enums have `#[non_exhaustive]` | Must | ☐ |
| Public error types are unified | Must | ☐ |
| No types with same name but different meanings | Forbidden | ☐ |
| Traits have unnecessary async usage | Check | ☐ |
| Numeric conversions have overflow risk | Check | ☐ |
| Time-related code is testable | Must | ☐ |
| Builders have input validation | Must | ☐ |
| Regex etc. use caching | Must | ☐ |
| Integration tests exist | Should | ☐ |
| Error path test coverage | Must | ☐ |

---
