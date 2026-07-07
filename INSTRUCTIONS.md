# INSTRUCTIONS.md — MoFA AI Development Guidelines (LLM-Optimized Edition)

This document defines **strict, production-grade Rust development standards** for the MoFA microkernel project.

It is optimized for:
- Human developers
- AI coding assistants (Copilot, Aider, Cody, ChatGPT)

---

# 🧠 CORE PRINCIPLES

- Strong typing over dynamic behavior
- Clear architecture boundaries
- Unified error handling
- Predictable APIs
- High testability
- Zero ambiguity for AI tools

---

# 🏗️ ARCHITECTURE OVERVIEW

```
mofa-sdk → mofa-runtime → mofa-foundation → mofa-kernel → mofa-plugins
```

## Layer Responsibilities

| Layer | Responsibility |
|------|---------------|
| kernel | Traits + core types ONLY |
| foundation | Concrete implementations + business logic |
| runtime | Execution lifecycle (event loop, registry) |
| sdk | Public API surface |
| plugins | Extensions and adapters |

## 🚨 Golden Rules

- Kernel MUST NOT contain business logic  
- Foundation MUST NOT redefine kernel traits  
- Dependencies must flow downward only  

```
Foundation → Kernel ✅  
Plugins → Kernel ✅  
Plugins → Foundation ✅  
Kernel → Foundation ❌ FORBIDDEN  
```

---

# ❗ ERROR HANDLING STANDARDS

## ✅ DO

- Define unified error type at crate root  
- Use `thiserror`  
- Use `From` for error conversion  

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KernelError {
    #[error("Agent error: {0}")]
    Agent(#[from] AgentError),

    #[error("Config error: {0}")]
    Config(#[from] ConfigError),
}
```

## ❌ DON'T

- Use `anyhow::Result` in public APIs  
- Implement `From<anyhow::Error>`  

## 💡 WHY

- Preserves structured errors  
- Improves debugging  
- Helps LLMs reason correctly  

---

# 🧱 TYPE DESIGN

## ✅ RULES

- Public enums MUST use `#[non_exhaustive]`  
- Comparable types MUST derive `PartialEq, Eq`  
- Debuggable types MUST derive `Debug`  
- Clone when needed  

```rust
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AgentState {
    Idle,
    Running,
}
```

## 💡 WHY

- Prevents breaking changes  
- Ensures forward compatibility  

---

# 📦 MODULE DESIGN

## ✅ DO

```rust
pub mod error;
pub mod agent;

pub use error::KernelError;
pub use agent::{Agent, AgentContext};

mod internal;
```

## ❌ DON'T

```rust
pub mod everything; // BAD
```

## 💡 WHY

- Controls public API surface  
- Avoids accidental exposure  

---

# ⚡ PERFORMANCE RULES

## ✅ DO

- Cache expensive objects (Regex, etc.)  
- Use `LazyLock` / `OnceLock`  

```rust
use std::sync::LazyLock;

static REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("pattern").unwrap()
});
```

## ❌ DON'T

- Recompute expensive objects repeatedly  

## 💡 WHY

- Reduces runtime cost  

---

# 🧠 TYPE SAFETY

## ❌ AVOID

- `serde_json::Value`  
- `Box<dyn Any + Send + Sync>`  

## ✅ PREFER

- Generics  
- Strongly typed structs  

## 💡 WHY

- LLMs perform better with structured types  
- Prevents runtime errors  

---

# 🔁 INTERFACE CONSISTENCY

## ✅ GOOD

```rust
pub fn new(id: impl Into<String>) -> Self
```

## ❌ BAD

```rust
pub fn new(id: String) -> Self
```

## BUILDER VALIDATION

```rust
pub fn with_weight(mut self, weight: f64) -> Result<Self, &'static str> {
    if weight < 0.0 {
        return Err("Weight must be non-negative");
    }
    self.weight = Some(weight);
    Ok(self)
}
```

---

# 🔢 SAFE CONVERSIONS

## ❌ BAD

```rust
let ts = millis as u64;
```

## ✅ GOOD

```rust
let ts = u64::try_from(millis).unwrap_or(u64::MAX);
```

## 💡 WHY

- Prevents overflow bugs  

---

# 💾 SERIALIZATION

## ✅ MUST

- Include versioning  

```rust
#[derive(Serialize, Deserialize)]
struct MessageEnvelope {
    version: u8,
    payload: Vec<u8>,
}
```

## ✅ SHOULD

- Use pluggable serializers  

```rust
pub trait Serializer {
    fn serialize<T: Serialize>(&self, value: &T) -> Result<Vec<u8>>;
    fn deserialize<T: DeserializeOwned>(&self, data: &[u8]) -> Result<T>;
}
```

---

# 🧪 TESTING STANDARDS

## MUST TEST

- Edge cases  
- Invalid inputs  
- Error paths  
- Concurrency  

## SHOULD

- Integration tests  

## 💡 WHY

- Ensures reliability  
- Prevents silent failures  

---

# 🧩 FEATURE FLAGS

## ✅ CORRECT

```toml
[dependencies]
config = { version = "0.14", optional = true }

[features]
default = []
config-loader = ["dep:config"]
```

## ❌ WRONG

- Feature-gating code but not dependency  

---

# 🚫 ANTI-PATTERNS

- Using `anyhow` in public APIs  
- Re-defining kernel traits in foundation  
- Overusing `serde_json::Value`  
- Exporting all modules blindly  
- Using async unnecessarily  
- Ignoring error handling  

---

# 🧩 MICROKERNEL RULES

## Trait Definition

- MUST be in kernel  
- NEVER in foundation  

## Implementation

- MUST be in foundation  
- NEVER in kernel  

## Data Types

- Core types → kernel  
- Business types → foundation  

---

# 📁 PROJECT STRUCTURE

```
mofa/
 ├── kernel/
 ├── foundation/
 ├── runtime/
 ├── sdk/
 └── plugins/
```

---

# 🤖 AI ASSISTANT INSTRUCTIONS

When generating code:

1. Always define traits in kernel  
2. Never add logic in kernel  
3. Implement logic only in foundation  
4. Use strong types (NO dynamic types)  
5. Follow unified error handling  
6. Avoid unnecessary async  
7. Validate all inputs  
8. Ensure testability (inject dependencies)  

---

# ✅ PR CHECKLIST

- [ ] Public enums use `#[non_exhaustive]`  
- [ ] Unified error handling used  
- [ ] No `anyhow` in public API  
- [ ] No trait duplication  
- [ ] No circular dependencies  
- [ ] Safe numeric conversions  
- [ ] Builder validation present  
- [ ] Regex caching used  
- [ ] Tests include edge cases  
- [ ] Error paths tested  

---

# 🔥 FINAL NOTE

This project enforces:

- Strict architecture  
- Maximum type safety  
- Zero ambiguity  
- AI-friendly patterns  

Following these rules ensures:
- Scalable system design  
- Maintainable codebase  
- High-quality AI-generated code  

---
