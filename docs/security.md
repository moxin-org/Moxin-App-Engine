# MoFA Security Guide

This comprehensive guide covers security considerations, best practices, and recommendations for using and deploying MoFA agents in production environments. It includes both general security practices and documentation for MoFA's built-in Security Governance layer.

## Table of Contents

- [Security Governance](#security-governance)
- [Credential Management](#credential-management)
- [Runtime Scripting Security](#runtime-scripting-security)
- [Plugin Security](#plugin-security)
- [Production Deployment Security](#production-deployment-security)
- [Threat Model](#threat-model)
- [Secure Configuration Patterns](#secure-configuration-patterns)
- [Monitoring and Auditing](#monitoring-and-auditing)

## Security Governance

MoFA includes a built-in Security Governance layer that provides role-based access control (RBAC), personally identifiable information (PII) redaction, content moderation, and prompt injection defense capabilities.

### Overview

The Security Governance layer provides production-ready security features:

- **RBAC**: Role-based access control with inheritance
- **PII Redaction**: GDPR-compliant data protection with multiple strategies
- **Content Moderation**: Harmful content filtering with flagging support
- **Prompt Injection Defense**: Attack prevention with confidence scoring
- **Audit Logging**: Complete compliance trail

**Status**: Complete - Production Ready  
**Total Tests**: 41 tests (31 unit + 10 integration)

### Architecture

The security governance implementation follows MoFA's microkernel architecture principles:

- **Runtime Layer (`mofa-runtime`)**: Security enforcement infrastructure
  - Trait definitions (`Authorizer`, `PiiDetector`, `ContentModerator`, `PromptGuard`)
  - Security service orchestrator
  - Configuration and event types
  - Audit logging helpers

- **Foundation Layer (`mofa-foundation`)**: Concrete implementations
  - RBAC implementations (`DefaultAuthorizer`, `RbacPolicy`, `Role`)
  - PII detection and redaction (`RegexPiiDetector`, `RegexPiiRedactor`)
  - Content moderation (`KeywordModerator`, `ContentPolicy`)
  - Prompt injection guard (`RegexPromptGuard`)

- **Kernel Layer (`mofa-kernel`)**: Minimal involvement
  - No security business logic (respects microkernel principle)

### RBAC (Role-Based Access Control)

**Location**: `mofa-foundation/src/security/rbac/`

**Usage Example**:
```rust
use mofa_foundation::security::{DefaultAuthorizer, RbacPolicy, Role};

let mut policy = RbacPolicy::new();
let admin_role = Role::new("admin")
    .with_permission("execute:tool:delete");
policy.add_role(admin_role);
policy.assign_role("agent-1", "admin");

let authorizer = DefaultAuthorizer::new(policy);
let result = authorizer
    .check_permission("agent-1", "execute", "tool:delete")
    .await?;
```

**Key Features**:
- Role inheritance (parent roles)
- Permission-based access control
- Subject-to-role mappings
- Default role assignment

### PII Detection and Redaction

**Location**: `mofa-foundation/src/security/pii/`

**Supported PII Types**:
- Email addresses
- Phone numbers (multiple formats)
- Credit card numbers (with Luhn validation)
- Social Security Numbers (US format)
- IP addresses (IPv4)
- API keys and tokens

**Redaction Strategies**:
- `Mask`: Replace with `[REDACTED]` placeholder
- `Hash`: Replace with SHA-256 hash (for audit trail)
- `Remove`: Delete entirely
- `Replace`: Category-specific placeholders (e.g., `[EMAIL]`)

**Usage Example**:
```rust
use mofa_foundation::security::{RegexPiiDetector, RegexPiiRedactor};
use mofa_runtime::security::types::RedactionStrategy;

let detector = RegexPiiDetector::new();
let redactor = RegexPiiRedactor::new()
    .with_default_strategy(RedactionStrategy::Mask);

let text = "Contact: user@example.com";
let detections = detector.detect(text).await?;
let redacted = redactor.redact(text, RedactionStrategy::Mask).await?;
```

### Content Moderation

**Location**: `mofa-foundation/src/security/moderation/`

**Moderation Verdicts**:
- `Allow`: Content is safe
- `Flag`: Content is flagged but allowed (with reason)
- `Block`: Content is blocked (with reason)

**Usage Example**:
```rust
use mofa_foundation::security::KeywordModerator;

let moderator = KeywordModerator::new()
    .add_blocked("spam")
    .add_flagged("urgent");

let result = moderator.moderate("This is spam content").await?;
```

### Prompt Injection Guard

**Location**: `mofa-foundation/src/security/guard/`

**Detection Patterns**:
- Instruction override attempts ("ignore previous instructions")
- System prompt injection ("you are now a system prompt")
- Role manipulation ("act as", "pretend to be")
- Jailbreak attempts ("unrestricted", "no filter")
- Base64 encoded instructions

**Usage Example**:
```rust
use mofa_foundation::security::RegexPromptGuard;

let guard = RegexPromptGuard::new().with_threshold(0.5);
let result = guard.check_injection(prompt).await?;
if result.is_suspicious {
    // Handle injection attempt
}
```

### Configuration

The `SecurityConfig` struct provides feature flags and behavior configuration:

```rust
use mofa_runtime::security::{SecurityConfig, SecurityFailMode};

let config = SecurityConfig::strict() // Production: strict mode
    .with_rbac_enabled(true)
    .with_pii_redaction_enabled(true)
    .with_content_moderation_enabled(true)
    .with_prompt_guard_enabled(true)
    .with_fail_mode(SecurityFailMode::FailClosed);
```

**Preset Configurations**:
- `SecurityConfig::permissive()`: All features disabled, fail-open mode (development)
- `SecurityConfig::strict()`: All features enabled, fail-closed mode (production)
- `SecurityConfig::new()`: All features disabled, configurable

**Fail Modes**:
- `FailOpen`: Allow on error (more permissive, better UX, less secure)
- `FailClosed`: Deny on error (more secure, stricter, may impact UX)

### Runtime Integration

```rust
use mofa_runtime::{AgentBuilder, SecurityService};
use std::sync::Arc;

let security_service = Arc::new(security_service);
let runtime = AgentBuilder::new("agent-1", "My Agent")
    .with_agent(my_agent)
    .await?
    .with_security_service(security_service);
```

The runtime automatically checks permissions in `handle_event()` before processing events:
1. RBAC check: Verifies agent has permission to execute
2. PII redaction: Sanitizes event data (if configured)
3. Content moderation: Checks event content (if configured)
4. Prompt guard: Validates input (if configured)

### Test Coverage

- **31 unit tests** covering all components
- **10 integration tests** covering real-world scenarios
- Edge case handling (empty inputs, unicode, performance)
- Performance benchmarks (<5ms overhead verified)

**Running Tests**:
```bash
cargo test -p mofa-foundation --lib security
```

### Examples

Production-ready examples are available in `examples/security/secure_agent/`:

```bash
cd examples/security/secure_agent
cargo run
```

The example demonstrates:
- Multi-tenant RBAC setup
- GDPR-compliant PII redaction
- Content moderation configuration
- Prompt injection defense
- End-to-end security pipeline
- Audit logging

### Best Practices

**RBAC Design**:
- Use principle of least privilege
- Leverage role inheritance to avoid duplication
- Assign default roles for unconfigured subjects
- Review and update role assignments regularly

**PII Handling**:
- Remove or hash SSNs entirely for GDPR compliance
- Use hash strategy for audit trail requirements
- Use replace strategy to maintain readability
- Enable Luhn validation for credit cards

**Content Moderation**:
- Combine keyword and ML-based moderation
- Use flagging for borderline content
- Monitor and adjust keyword lists
- Configure different policies per tenant

**Prompt Injection Defense**:
- Adjust confidence threshold based on false positive rate
- Regularly update detection patterns
- Log all detected attempts for analysis

### Performance

Current performance characteristics (verified in tests):
- PII detection: <10ms for 100 iterations of typical text
- PII redaction: <20ms for 100 iterations
- Content moderation: <5ms for 100 iterations
- RBAC checks: <1ms per check (cached)

### Troubleshooting

**Permission Denied Errors**:
1. Verify agent has correct role assigned in RBAC policy
2. Check permission format matches policy definition
3. Verify RBAC is enabled in SecurityConfig
4. Check fail mode (fail-open allows on error)

**PII Not Detected**:
1. Verify pattern matches your data format
2. Check if validation is too strict (e.g., Luhn for credit cards)
3. Review regex patterns in `patterns.rs`
4. Add custom patterns if needed

**High False Positive Rate**:
1. Adjust confidence threshold for prompt guard
2. Review keyword lists for moderation
3. Add exceptions for common false positives
4. Consider ML-based approaches for better accuracy

For detailed implementation documentation, see the sections above.

## Credential Management

MoFA integrates with multiple LLM providers and services, each requiring API keys and credentials. Proper credential management is critical for security.

### Environment Variables (Recommended)

**Best Practice**: Use environment variables for all credentials.

```bash
# OpenAI
export OPENAI_API_KEY="sk-..."
export OPENAI_ORG_ID="org-..."

# Anthropic
export ANTHROPIC_API_KEY="sk-ant-..."

# Google
export GOOGLE_API_KEY="..."

# Database
export DATABASE_URL="postgresql://user:password@localhost/mofa"
```

```rust
use mofa_sdk::llm::openai_from_env;

// Credentials are automatically loaded from environment
let provider = openai_from_env()?;
```

**Advantages**:
- Never committed to version control
- Easy to rotate
- Standard practice across cloud platforms
- Works well with container orchestration (Kubernetes, Docker Swarm)

**Best Practices**:
- Use a `.env` file for local development (add to `.gitignore`)
- Never commit `.env` files to version control
- Use different credentials for development, staging, and production
- Rotate credentials regularly (recommended: every 90 days)
- Use least-privilege access principles

### Configuration Files

If you must use configuration files:

**DO**:
```toml
# config/production.toml
[llm]
provider = "openai"
# Use environment variable substitution
api_key = "${OPENAI_API_KEY}"
```

**DO NOT**:
```toml
# NEVER DO THIS
[llm]
api_key = "sk-abc123def456..."  # Plain text credentials in config
```

### Secret Rotation

**Production Strategy**:

1. **Use credential aliases**: Many providers allow API key aliases that can be rotated without changing the main key
2. **Implement rotation logic**:
```rust
use std::time::Duration;

// Reload provider periodically
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_hours(24));
    loop {
        interval.tick().await;
        // Reload credentials from environment
        match openai_from_env() {
            Ok(provider) => agent.update_provider(provider).await,
            Err(e) => eprintln!("Failed to reload credentials: {}", e),
        }
    }
});
```

3. **Monitor for expiring credentials**: Set up alerts for credentials nearing expiration
4. **Automate rotation**: Use cloud provider secret management services (AWS Secrets Manager, Azure Key Vault, etc.)

### DO NOT Commit Credentials

**Protection Measures**:

1. Add to `.gitignore`:
```
.env
.env.local
.env.*.local
*.key
*.pem
credentials.json
secrets/
```

2. Use pre-commit hooks to detect secrets:
```bash
# Install git-secrets
git secrets --install
git secrets --register-aws
git secrets --add 'sk-'
git secrets --add 'api_key\s*='
```

3. Use GitHub secret scanning (enabled by default for public repositories)

4. Scan repository for accidentally committed secrets:
```bash
# Use truffleHog
trufflehog git https://github.com/yourorg/repo --only-verified

# Or use gitleaks
gitleaks detect --source . --verbose
```

## Runtime Scripting Security

MoFA uses the Rhai scripting engine for runtime programmability. While powerful, scripts require careful security configuration.

### Rhai Engine Sandboxing

**Default Sandbox Configuration**:

```rust
use mofa_sdk::plugins::{RhaiPlugin, RhaiPluginConfig};

let config = RhaiPluginConfig::new()
    .with_max_operations(100_000)      // Limit operations
    .with_max_depth(32)                // Limit call stack depth
    .with_max_modules(0)               // Disable module loading
    .with_max_functions(50)            // Limit function definitions
    .with_max_variables(100)           // Limit variable count
    .with_timeout(Duration::from_secs(5));  // Execution timeout

let mut plugin = RhaiPlugin::new(config).await?;
```

**Security Boundaries**:

The Rhai sandbox provides:

- **No file system access** (unless explicitly registered)
- **No network access** (unless explicitly registered)
- **No shell access** (unless explicitly registered)
- **Memory limits** (configurable)
- **Operation limits** (configurable)
- **Execution timeouts** (configurable)

**WARNING**: The sandbox only limits what the script can access. If you register unsafe functions, the script can use them!

### Resource Limits Configuration

**Production Recommended Limits**:

| Setting | Development | Production | Rationale |
|---------|-------------|------------|-----------|
| `max_operations` | 1,000,000 | 100,000 | Prevent infinite loops |
| `max_depth` | 64 | 32 | Prevent stack overflow |
| `timeout` | 30s | 5s | Prevent hanging scripts |
| `max_modules` | 10 | 0 | Prevent unauthorized imports |
| `max_string_size` | 1MB | 100KB | Prevent memory exhaustion |
| `max_array_size` | 10,000 | 1,000 | Prevent memory exhaustion |

### Script Validation Best Practices

**Before Executing User-Provided Scripts**:

1. **Parse and Validate**:
```rust
use mofa_sdk::plugins::RhaiPlugin;

let plugin = RhaiPlugin::new(config).await?;

// Parse without executing
if let Err(e) = plugin.validate_script("fn main() { 1 + }") {
    eprintln!("Script syntax error: {}", e);
    return Err(e);
}

// Execute validated script
plugin.execute("validated_script.rhai").await?;
```

2. **Static Analysis**:
```rust
// Check for dangerous patterns
fn validate_script_content(script: &str) -> Result<(), Box<dyn std::error::Error>> {
    let forbidden_patterns = vec![
        "std::fs::",
        "std::net::",
        "std::process::",
        "shell(",
        "exec(",
    ];

    for pattern in forbidden_patterns {
        if script.contains(pattern) {
            return Err(format!("Script contains forbidden pattern: {}", pattern).into());
        }
    }

    Ok(())
}
```

3. **Sandbox Testing**: Test scripts in isolated environment before production deployment

4. **Code Review**: Review all scripts for security issues before deployment

### Hot-Reload Considerations

**Risks**:
- Malicious script could be introduced via hot-reload
- Scripts without validation could crash agents
- Race conditions during reload

**Safe Hot-Reload Pattern**:

```rust
use mofa_sdk::plugins::{RhaiPlugin, HotReloadableRhaiPromptPlugin};
use std::path::Path;

async fn safe_hot_reload(
    plugin: &mut HotReloadableRhaiPromptPlugin,
    script_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read script to temporary location
    let temp_path = script_path.with_extension("rhai.tmp");
    std::fs::copy(script_path, &temp_path)?;

    // 2. Validate the script
    let content = std::fs::read_to_string(&temp_path)?;
    validate_script_content(&content)?;

    // 3. Test in isolated environment
    let test_plugin = RhaiPlugin::new(config).await?;
    test_plugin.execute(&temp_path).await?;

    // 4. Atomic replace
    plugin.reload().await?;

    // 5. Cleanup
    std::fs::remove_file(&temp_path)?;

    Ok(())
}
```

**Production Recommendations**:
- Disable hot-reload in production environments
- If hot-reload is required, implement validation and testing
- Use atomic file operations to prevent partial reloads
- Monitor hot-reload operations for suspicious activity
- Log all hot-reload events for audit trails

### Script Signing (Future Enhancement)

For production deployments requiring stronger security guarantees:

**Recommended Pattern**:
```rust
// Verify script signature before execution
use ed25519_dalek::{Verifier, Signature, VerifyingKey};

fn verify_script(script_path: &Path, public_key: &VerifyingKey) -> bool {
    let script = std::fs::read(script_path).unwrap();
    let signature = std::fs::read(script_path.with_extension("sig")).unwrap();

    public_key.verify(&script, &Signature::from_bytes(&signature).unwrap()).is_ok()
}
```

## Plugin Security

MoFA's dual-layer plugin system provides flexibility but requires security considerations.

### WASM Plugin Isolation

**WebAssembly Sandboxing**:

WASM plugins provide strong isolation:

```rust
use mofa_sdk::plugins::WasmPlugin;

// WASM plugins run in isolated sandbox
let wasm_plugin = WasmPlugin::from_file("plugin.wasm")?
    .with_memory_limit(10 * 1024 * 1024)  // 10MB
    .with_timeout(Duration::from_secs(5))
    .build();
```

**Security Guarantees**:
- Memory isolation (separate linear memory)
- No direct file system access
- No network access (unless explicitly imported)
- Capability-based security
- Deterministic execution

**When to Use WASM**:
- Untrusted third-party plugins
- User-contributed code
- Plugins from external sources
- High-security requirements

### Plugin Verification

**Best Practices**:

1. **Verify Plugin Source**:
```rust
use std::collections::HashSet;

let allowed_sources = HashSet::from([
    "github.com/mofa-org/plugins",
    "internal.registry.company.com",
]);

fn verify_plugin_source(plugin_url: &str) -> bool {
    allowed_sources.iter().any(|src| plugin_url.starts_with(src))
}
```

2. **Checksum Verification**:
```rust
use sha2::{Sha256, Digest};

fn verify_plugin_checksum(plugin_path: &Path, expected: &str) -> bool {
    let content = std::fs::read(plugin_path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let result = hasher.finalize();
    format!("{:x}", result) == expected
}
```

3. **Version Pinning**:
```toml
# Cargo.toml
[dependencies]
mofa-plugin-name = "=0.1.5"  # Pin exact version
```

### SemVer Dependency Resolution and Supply Chain Security

PR #1493 adds a full SemVer resolver with OWASP Agentic AI Top 10 supply chain checks to the plugin install path. Before any plugin is written to disk:

1. The resolver builds a conflict-free, locked dependency graph using backtracking (tries highest compatible version first, backs down on conflict).
2. SupplyChainGuard checks: yanked status, trust score threshold, SLSA provenance level, dependency confusion (typosquat detection via Wagner-Fischer edit distance).
3. Ed25519 signature verification against the publisher key from the registry manifest.
4. PluginLockfile written -- serializable, committable, and reproducible across environments.

Minimum config for enterprise deployment:

```toml
[plugin_resolver]
min_trust_score = 0.7
required_slsa_level = "Level2"
verify_signatures = true
```

Trust score formula: `download_score * 0.3 + community_rating * 0.5 + recency_score * 0.2`

### Third-Party Plugin Risks

**Risk Assessment**:

| Risk | Mitigation |
|------|------------|
| Malicious code | Use WASM sandboxing, review source code |
| Vulnerabilities | Keep plugins updated, monitor security advisories |
| Dependency confusion | Use verified sources, checksum verification |
| Supply chain attacks | Use SBOM, sign plugins, verify provenance |

**Before Using Third-Party Plugins**:

1. **Review the Code**: Understand what the plugin does
2. **Check Dependencies**: Review the plugin's dependencies
3. **Test in Isolation**: Test in development environment first
4. **Monitor Behavior**: Monitor plugin behavior in production
5. **Limit Permissions**: Use principle of least privilege

### Plugin Development Security

**Secure Plugin Development**:

```rust
use mofa_sdk::plugins::{AgentPlugin, PluginContext};

struct SecurePlugin;

#[async_trait::async_trait]
impl AgentPlugin for SecurePlugin {
    async fn execute(&mut self, input: String) -> PluginResult<String> {
        // Validate input
        if input.len() > MAX_INPUT_SIZE {
            return Err("Input too large".into());
        }

        // Sanitize output
        let output = process_input(&input)?;
        if output.len() > MAX_OUTPUT_SIZE {
            return Err("Output too large".into());
        }

        Ok(output)
    }
}
```

**Security Checklist for Plugin Developers**:
- Validate all input parameters
- Sanitize all output data
- Use error handling (don't panic on invalid input)
- Limit resource consumption
- Avoid unsafe code when possible
- Document security considerations
- Follow secure coding practices
- Test for security vulnerabilities

## Production Deployment Security

Deploying MoFA agents to production requires additional security considerations.

### Network Security

**Inter-Agent Communication**:

```rust
use mofa_sdk::runtime::{SimpleRuntime, SecurityConfig};

let runtime = SimpleRuntime::new()
    .with_tls_enabled(true)
    .with_client_auth_required(true)
    .with_mtls_enabled(true);  // Mutual TLS
```

**Recommendations**:
- Use TLS for all agent communication
- Implement mutual TLS (mTLS) for zero-trust networks
- Use network segmentation to isolate agents
- Implement rate limiting to prevent DoS
- Monitor network traffic for anomalies

### Authentication and Authorization

**Multi-Agent Systems**:

```rust
use mofa_sdk::runtime::{AuthConfig, AuthorizationPolicy};

let auth_config = AuthConfig::new()
    .with_jwt_secret(std::env::var("JWT_SECRET")?)
    .with_token_expiry(Duration::from_hours(1))
    .with_refresh_token_enabled(true);

let authz_policy = AuthorizationPolicy::new()
    .allow("agent_a", ["send_message", "read_state"])
    .allow("agent_b", ["subscribe_topic"])
    .deny_all_unauthorized();
```

**Best Practices**:
- Use strong authentication (JWT, mTLS)
- Implement role-based access control (RBAC)
- Use least-privilege access
- Rotate authentication credentials regularly
- Audit all authorization decisions

### Audit Logging

**Security Event Logging**:

```rust
use mofa_sdk::monitoring::{AuditLogger, SecurityEvent};

let logger = AuditLogger::new()
    .with_log_destination("/var/log/mofa/audit.log")
    .with_log_level(LogLevel::Security)
    .with_structured_logging(true);

// Log security events
logger.log(SecurityEvent {
    event_type: "plugin_loaded",
    agent_id: "agent-001",
    plugin_name: "llm-plugin",
    timestamp: Utc::now(),
    metadata: serde_json::json!({"version": "1.0.0"}),
});
```

**What to Log**:
- Plugin load/unload events
- Script execution events
- Authentication successes/failures
- Authorization failures
- Configuration changes
- Credential access
- Network connections

**Log Protection**:
- Use append-only logs
- Encrypt logs at rest
- Forward logs to secure log aggregation system
- Regularly review logs for security events
- Retain logs according to compliance requirements

### Secure Configuration Patterns

**Environment-Specific Configuration**:

```rust
use mofa_sdk::config::Environment;

let config = match Environment::detect() {
    Environment::Production => {
        // Secure defaults for production
        SecurityConfig::new()
            .with_debug_mode(false)
            .with_verbose_logging(false)
            .with_stack_traces_enabled(false)
    }
    Environment::Development => {
        // Relaxed settings for development
        SecurityConfig::new()
            .with_debug_mode(true)
            .with_verbose_logging(true)
    }
};
```

**Configuration Security Checklist**:
- Never include credentials in configuration files
- Use environment-specific configuration
- Validate all configuration values
- Use secure defaults
- Implement configuration versioning
- Encrypt sensitive configuration values
- Audit configuration changes

## Threat Model

Understanding potential threats helps design effective security measures.

### Attack Surfaces

| Surface | Threat | Mitigation |
|---------|--------|------------|
| **LLM API Keys** | Credential theft | Use env vars, rotate regularly, monitor usage |
| **Rhai Scripts** | Code injection | Sandbox limits, input validation, script review |
| **Plugins** | Malicious code | WASM isolation, code review, verification |
| **Network** | Eavesdropping, MITM | TLS, mTLS, network segmentation |
| **Agent Messages** | Message tampering | Message signing, encryption |
| **Database** | SQL injection | Parameterized queries, input validation |
| **File System** | Unauthorized access | File permissions, sandboxing |

### Common Attack Vectors

1. **Credential Theft**
   - Attacker gains access to API keys
   - **Mitigation**: Use secret management, rotate credentials, monitor usage

2. **Script Injection**
   - Attacker provides malicious Rhai script
   - **Mitigation**: Sandbox limits, input validation, script review

3. **Plugin Compromise**
   - Attacker provides malicious plugin
   - **Mitigation**: WASM isolation, code review, verification

4. **Denial of Service**
   - Attacker overwhelms agent with requests
   - **Mitigation**: Rate limiting, resource limits, monitoring

5. **Message Tampering**
   - Attacker modifies messages between agents
   - **Mitigation**: Message signing, TLS, integrity checks

### Security Monitoring

**Key Metrics to Monitor**:
- Failed authentication attempts
- Authorization failures
- Unusual script execution patterns
- Plugin load failures
- High resource consumption
- Unexpected network connections
- Credential usage anomalies

**Alerting**:
```rust
use mofa_sdk::monitoring::{Alert, AlertSeverity};

// Set up alerts for security events
alert_manager
    .add_rule("failed_auth", AlertSeverity::High, 5, Duration::from_minutes(5))
    .add_rule("script_error", AlertSeverity::Medium, 10, Duration::from_minutes(1))
    .add_rule("high_memory", AlertSeverity::Warning, 1, Duration::from_minutes(1));
```

## Secure Configuration Patterns

### Credential Storage

**Environment Variables** (Recommended):
```bash
export OPENAI_API_KEY="sk-..."
export DATABASE_URL="postgresql://..."
```

**Secret Management Services** (Production):
- AWS Secrets Manager
- Azure Key Vault
- Google Secret Manager
- HashiCorp Vault

```rust
use aws_sdk_secretsmanager::Client;

async fn load_secret(secret_name: &str) -> Result<String, Error> {
    let client = Client::new(&aws_config::load_from_env().await);
    let response = client.get_secret_value()
        .secret_id(secret_name)
        .send()
        .await?;
    Ok(response.secret_string().unwrap().to_string())
}
```

### Secure Defaults

**Always Enable**:
- TLS for network communication
- Script resource limits
- Input validation
- Error handling
- Audit logging

**Always Disable** (in production):
- Debug mode
- Verbose logging
- Stack traces in errors
- Hot-reload of scripts/plugins
- Unsafe functions

## Monitoring and Auditing

### Security Metrics

**Track These Metrics**:
1. Authentication success/failure rate
2. Authorization failure rate
3. Script execution errors
4. Plugin load failures
5. Resource usage (CPU, memory, network)
6. Credential rotation status
7. Security advisory compliance

### Incident Response

**Security Incident Response Plan**:

1. **Detection**: Monitoring alerts identify suspicious activity
2. **Containment**: Isolate affected systems, revoke compromised credentials
3. **Eradication**: Remove malicious code, patch vulnerabilities
4. **Recovery**: Restore from clean backups, monitor for recurrence
5. **Lessons Learned**: Post-incident review, update security practices

**Example Response Procedure**:
```rust
async fn handle_security_incident(incident: SecurityIncident) {
    // 1. Contain
    disable_compromised_agents(&incident.agent_ids).await;

    // 2. Investigate
    let details = collect_forensic_data(&incident).await;

    // 3. Remediate
    apply_security_patches(&incident.vulnerabilities).await;

    // 4. Monitor
    monitor_for_recurrence(&incident.indicators).await;

    // 5. Report
    generate_incident_report(incident, details).await;
}
```

## Compliance Considerations

If your organization has compliance requirements (SOC 2, HIPAA, GDPR, etc.):

**Data Protection**:
- Encrypt data at rest and in transit
- Implement data retention policies
- Provide data export capabilities
- Support data deletion requests

**Access Control**:
- Implement authentication and authorization
- Maintain audit logs
- Provide access reporting
- Support access revocation

**Security Documentation**:
- Maintain security policies
- Conduct security assessments
- Provide security training
- Document security incidents

## Additional Resources

- [Rust Security Guidelines](https://doc.rust-lang.org/book/ch12-00-an-io-project.html)
- [OWASP Application Security Verification Standard](https://owasp.org/www-project-application-security-security-verification-standard/)
- [GitHub Security Best Practices](https://docs.github.com/en/code-security)
- [Rhai Security Documentation](https://rhai.rs/book/security/)
- [WebAssembly Security](https://webassembly.org/docs/security/)

## Reporting Security Issues

If you discover a security vulnerability in MoFA, please report it privately:

- **GitHub**: https://github.com/mofa-org/mofa/security/advisories
- **Email**: security@mofa.ai

See [SECURITY.md](../SECURITY.md) for detailed information on responsible disclosure.

---

**Last Updated**: 2025-02-20

For questions or feedback about this security guide, please open an issue on [GitHub](https://github.com/mofa-org/mofa/issues).

---

**English** | [简体中文](zh-CN/security.md)
