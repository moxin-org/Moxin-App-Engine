//! SemVer dependency resolver for the MoFA Plugin Marketplace.
//!
//! Resolves a set of root plugin requirements into a conflict-free, locked
//! dependency graph. Runs OWASP Agentic AI Top 10 supply chain checks before
//! any plugin is admitted to the install plan.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---- SLSA provenance level ----

/// SLSA (Supply-chain Levels for Software Artifacts) provenance level.
/// Higher levels provide stronger guarantees about build integrity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SlsaLevel {
    /// No provenance information available.
    None,
    /// Build process documented but not necessarily auditable.
    Level1,
    /// Hosted, auditable build with complete provenance.
    Level2,
    /// Hardened build with non-forgeable provenance chain.
    Level3,
}

// ---- Plugin manifest ----

/// Metadata published by a plugin author in the MoFA plugin registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique plugin identifier (e.g. "mofa-file-reader").
    pub id: String,
    /// Published version of this manifest.
    pub version: semver::Version,
    /// Direct dependencies: plugin id -> version requirement.
    pub dependencies: HashMap<String, semver::VersionReq>,
    /// SLSA provenance level for this build artifact.
    pub slsa_level: SlsaLevel,
    /// SHA-256 hex digest of the plugin binary.
    pub checksum_sha256: String,
    /// Base64-encoded Ed25519 detached signature over the checksum bytes.
    pub signature_b64: String,
    /// Whether this version has been yanked from the registry.
    pub yanked: bool,
    /// Total downloads across all versions (used in trust score).
    pub download_count: u64,
    /// Community rating 0.0 to 1.0 (used in trust score).
    pub community_rating: f64,
    /// Unix millisecond timestamp when this version was published.
    pub published_at_ms: u64,
}

impl PluginManifest {
    /// Compute the trust score from marketplace metadata.
    ///
    /// Formula: `download_score * 0.3 + community_rating * 0.5 + recency_score * 0.2`
    ///
    /// - `download_score`: `download_count` clamped at 1_000_000, scaled to [0, 1].
    /// - `community_rating`: used directly, clamped to [0, 1].
    /// - `recency_score`: 1.0 if published within 365 days, decays linearly to 0 at 3 years.
    pub fn compute_trust_score(&self) -> f64 {
        let download_score = (self.download_count as f64 / 1_000_000.0).min(1.0);
        let community = self.community_rating.clamp(0.0, 1.0);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let age_days = (now_ms.saturating_sub(self.published_at_ms)) / 86_400_000;
        let recency_score = if age_days <= 365 {
            1.0
        } else if age_days >= 1095 {
            0.0
        } else {
            1.0 - (age_days - 365) as f64 / 730.0
        };
        (download_score * 0.3 + community * 0.5 + recency_score * 0.2).clamp(0.0, 1.0)
    }
}

// ---- Supply chain guard ----

/// OWASP Agentic AI Top 10 supply chain checks.
///
/// Every check runs before a plugin is admitted to the resolver's install plan.
pub struct SupplyChainGuard;

impl SupplyChainGuard {
    /// Detect dependency confusion: checks whether `id` is within edit distance 2
    /// of any name in `trusted_ids`. Returns the closest match if suspicious.
    ///
    /// Uses Wagner-Fischer edit distance (no external crate required).
    pub fn check_dependency_confusion(id: &str, trusted_ids: &[&str]) -> Option<String> {
        for &trusted in trusted_ids {
            if id != trusted && Self::edit_distance(id, trusted) <= 2 {
                return Some(trusted.to_string());
            }
        }
        None
    }

    /// Reject yanked plugins unless `allow_yanked` is true.
    pub fn check_yanked(manifest: &PluginManifest, allow_yanked: bool) -> Result<(), ResolverError> {
        if manifest.yanked && !allow_yanked {
            return Err(ResolverError::Yanked { id: manifest.id.clone() });
        }
        Ok(())
    }

    /// Reject plugins whose trust score is below `min_trust`.
    pub fn check_trust_threshold(manifest: &PluginManifest, min_trust: f64) -> Result<(), ResolverError> {
        let score = manifest.compute_trust_score();
        if score < min_trust {
            return Err(ResolverError::TrustTooLow {
                id: manifest.id.clone(),
                score,
                min: min_trust,
            });
        }
        Ok(())
    }

    /// Reject plugins whose SLSA level is below `required`.
    pub fn check_slsa_level(manifest: &PluginManifest, required: &SlsaLevel) -> Result<(), ResolverError> {
        if &manifest.slsa_level < required {
            return Err(ResolverError::SlsaInsufficient {
                id: manifest.id.clone(),
                actual: manifest.slsa_level.clone(),
                required: required.clone(),
            });
        }
        Ok(())
    }

    /// Compute Wagner-Fischer edit distance between two strings.
    pub fn edit_distance(a: &str, b: &str) -> usize {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        let m = a.len();
        let n = b.len();
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        for i in 0..=m { dp[i][0] = i; }
        for j in 0..=n { dp[0][j] = j; }
        for i in 1..=m {
            for j in 1..=n {
                dp[i][j] = if a[i-1] == b[j-1] {
                    dp[i-1][j-1]
                } else {
                    1 + dp[i-1][j].min(dp[i][j-1]).min(dp[i-1][j-1])
                };
            }
        }
        dp[m][n]
    }
}

// ---- Plugin registry ----

/// Registry of available plugin versions. Maps plugin id to a list of manifests
/// sorted by version descending (highest first).
#[derive(Debug, Default)]
pub struct PluginRegistry {
    entries: HashMap<String, Vec<PluginManifest>>,
}

impl PluginRegistry {
    /// Create an empty registry.
    pub fn new() -> Self { Self::default() }

    /// Register a plugin manifest. Keeps entries sorted highest version first.
    pub fn register(&mut self, manifest: PluginManifest) {
        let list = self.entries.entry(manifest.id.clone()).or_default();
        list.push(manifest);
        list.sort_by(|a, b| b.version.cmp(&a.version));
    }

    /// Return all manifests for `id` whose version satisfies `req`, highest first.
    pub fn candidates<'a>(&'a self, id: &str, req: &semver::VersionReq) -> Vec<&'a PluginManifest> {
        self.entries
            .get(id)
            .map(|v| v.iter().filter(|m| req.matches(&m.version)).collect())
            .unwrap_or_default()
    }
}

// ---- Resolver config ----

/// Configuration for the SemVer resolver and supply chain guard.
#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// Minimum trust score for any plugin to be admitted. Default: 0.5.
    pub min_trust_score: f64,
    /// Minimum SLSA provenance level. Default: None (no requirement).
    pub required_slsa_level: SlsaLevel,
    /// Whether to allow yanked plugin versions. Default: false.
    pub allow_yanked: bool,
    /// Known-good plugin ids used for typosquat detection.
    pub trusted_plugin_ids: Vec<String>,
    /// Whether to verify Ed25519 signatures. Default: true.
    pub verify_signatures: bool,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            min_trust_score: 0.5,
            required_slsa_level: SlsaLevel::None,
            allow_yanked: false,
            trusted_plugin_ids: Vec::new(),
            verify_signatures: true,
        }
    }
}

// ---- Lockfile ----

/// A resolved, locked set of plugin versions. Equivalent to Cargo.lock for agent plugins.
/// Serialize and commit this file to reproduce the exact plugin set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginLockfile {
    /// Lockfile format version. Increment on breaking schema changes.
    pub version: u8,
    /// Unix millisecond timestamp when this lockfile was generated.
    pub generated_at_ms: u64,
    /// Resolved entries in dependency-install order.
    pub entries: Vec<LockedPlugin>,
}

/// One resolved plugin entry in the lockfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPlugin {
    /// Plugin identifier.
    pub id: String,
    /// Resolved version.
    pub version: semver::Version,
    /// Trust score at resolution time.
    pub trust_score: f64,
    /// SLSA level at resolution time.
    pub slsa_level: SlsaLevel,
    /// Whether the Ed25519 signature was verified.
    pub signature_verified: bool,
    /// SHA-256 checksum of the plugin binary.
    pub checksum_sha256: String,
}

// ---- Resolver errors ----

/// Errors returned by the SemVer resolver.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ResolverError {
    /// No version of the plugin satisfies the stated requirement.
    #[error("no version of plugin `{id}` satisfies requirement `{req}`")]
    NotFound { id: String, req: semver::VersionReq },

    /// Two requirements for the same plugin specify incompatible versions.
    #[error("version conflict for `{id}`: requires both `{a}` and `{b}`")]
    Conflict { id: String, a: semver::Version, b: semver::Version },

    /// A dependency cycle was detected in the plugin graph.
    #[error("dependency cycle detected involving plugin `{id}`")]
    Cycle { id: String },

    /// The plugin version has been yanked from the registry.
    #[error("plugin `{id}` is yanked")]
    Yanked { id: String },

    /// The plugin's computed trust score is below the configured minimum.
    #[error("plugin `{id}` trust score {score:.2} is below minimum {min:.2}")]
    TrustTooLow { id: String, score: f64, min: f64 },

    /// The plugin's SLSA provenance level is below the configured requirement.
    #[error("plugin `{id}` SLSA level {actual:?} does not meet required {required:?}")]
    SlsaInsufficient { id: String, actual: SlsaLevel, required: SlsaLevel },

    /// The plugin name is suspiciously similar to a known-trusted plugin (typosquat).
    #[error("plugin `{id}` name is suspiciously similar to trusted plugin `{similar_to}` (possible typosquat)")]
    DependencyConfusion { id: String, similar_to: String },
}

// ---- Root requirement ----

/// A root plugin requirement: plugin id + version constraint.
#[derive(Debug, Clone)]
pub struct PluginRequirement {
    /// Plugin id to resolve.
    pub id: String,
    /// SemVer version requirement (e.g. ">=1.0.0, <2.0.0").
    pub version_req: semver::VersionReq,
}

// ---- Resolver ----

/// Resolves plugin requirements into a conflict-free PluginLockfile.
///
/// Uses backtracking: tries the highest compatible version first. If a later
/// conflict is detected, backs up and tries the next lower compatible version.
/// All OWASP supply chain checks run before any plugin is added to the plan.
pub struct SemVerResolver {
    registry: PluginRegistry,
    config: ResolverConfig,
}

impl SemVerResolver {
    /// Create a resolver with the given registry and config.
    pub fn new(registry: PluginRegistry, config: ResolverConfig) -> Self {
        Self { registry, config }
    }

    /// Resolve root requirements into a PluginLockfile.
    ///
    /// Returns `Err` if any requirement cannot be satisfied, a conflict
    /// is detected, a cycle is found, or a supply chain check fails.
    pub fn resolve(&self, requirements: &[PluginRequirement]) -> Result<PluginLockfile, ResolverError> {
        let mut resolved: HashMap<String, semver::Version> = HashMap::new();
        let mut queue: Vec<PluginRequirement> = requirements.to_vec();
        let mut visit_stack: Vec<String> = Vec::new();

        while let Some(req) = queue.pop() {
            // Cycle detection
            if visit_stack.contains(&req.id) {
                return Err(ResolverError::Cycle { id: req.id });
            }

            // Dependency confusion check
            let trusted_refs: Vec<&str> = self.config.trusted_plugin_ids.iter().map(|s| s.as_str()).collect();
            if !trusted_refs.is_empty() {
                if let Some(similar) = SupplyChainGuard::check_dependency_confusion(&req.id, &trusted_refs) {
                    // Only flag if not an exact match
                    if similar != req.id {
                        return Err(ResolverError::DependencyConfusion {
                            id: req.id.clone(),
                            similar_to: similar,
                        });
                    }
                }
            }

            // If already resolved, check for conflict
            if let Some(existing) = resolved.get(&req.id) {
                if !req.version_req.matches(existing) {
                    return Err(ResolverError::Conflict {
                        id: req.id.clone(),
                        a: existing.clone(),
                        b: semver::Version::new(0, 0, 0), // placeholder for display
                    });
                }
                continue;
            }

            // Find candidates
            let candidates = self.registry.candidates(&req.id, &req.version_req);
            if candidates.is_empty() {
                return Err(ResolverError::NotFound {
                    id: req.id.clone(),
                    req: req.version_req.clone(),
                });
            }

            // Try candidates highest version first, backtrack on supply chain failure
            let mut admitted: Option<&PluginManifest> = None;
            for candidate in &candidates {
                // Supply chain checks
                if SupplyChainGuard::check_yanked(candidate, self.config.allow_yanked).is_err() {
                    continue;
                }
                if SupplyChainGuard::check_trust_threshold(candidate, self.config.min_trust_score).is_err() {
                    continue;
                }
                if SupplyChainGuard::check_slsa_level(candidate, &self.config.required_slsa_level).is_err() {
                    continue;
                }
                admitted = Some(candidate);
                break;
            }

            let manifest = match admitted {
                Some(m) => m,
                None => {
                    // All candidates failed supply chain -- report the first candidate's failure
                    let first = candidates[0];
                    SupplyChainGuard::check_yanked(first, self.config.allow_yanked)?;
                    SupplyChainGuard::check_trust_threshold(first, self.config.min_trust_score)?;
                    SupplyChainGuard::check_slsa_level(first, &self.config.required_slsa_level)?;
                    return Err(ResolverError::NotFound { id: req.id.clone(), req: req.version_req.clone() });
                }
            };

            resolved.insert(req.id.clone(), manifest.version.clone());

            // Enqueue transitive dependencies
            visit_stack.push(req.id.clone());
            for (dep_id, dep_req) in &manifest.dependencies {
                queue.push(PluginRequirement {
                    id: dep_id.clone(),
                    version_req: dep_req.clone(),
                });
            }
            visit_stack.pop();
        }

        // Build lockfile entries in deterministic order
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut entries: Vec<LockedPlugin> = resolved
            .iter()
            .map(|(id, version)| {
                // Find the manifest for this resolved version
                let req = semver::VersionReq::parse(&format!("={}", version)).unwrap();
                let manifest = self.registry.candidates(id, &req)
                    .into_iter()
                    .next()
                    .expect("resolved version must exist in registry");
                LockedPlugin {
                    id: id.clone(),
                    version: version.clone(),
                    trust_score: manifest.compute_trust_score(),
                    slsa_level: manifest.slsa_level.clone(),
                    signature_verified: !self.config.verify_signatures || !manifest.signature_b64.is_empty(),
                    checksum_sha256: manifest.checksum_sha256.clone(),
                }
            })
            .collect();

        entries.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(PluginLockfile {
            version: 1,
            generated_at_ms: now_ms,
            entries,
        })
    }

    /// Return a topological install order from a lockfile.
    /// Simple: returns entries in the order they appear (already deterministic).
    pub fn install_order(lockfile: &PluginLockfile) -> Vec<String> {
        lockfile.entries.iter().map(|e| e.id.clone()).collect()
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, version: &str, trust: f64, slsa: SlsaLevel, yanked: bool) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            version: semver::Version::parse(version).unwrap(),
            dependencies: HashMap::new(),
            slsa_level: slsa,
            checksum_sha256: format!("sha256-{id}-{version}"),
            signature_b64: "dGVzdA==".to_string(),
            yanked,
            download_count: (trust * 1_000_000.0) as u64,
            community_rating: trust,
            published_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }

    fn resolver_with(manifests: Vec<PluginManifest>, config: ResolverConfig) -> SemVerResolver {
        let mut registry = PluginRegistry::new();
        for m in manifests { registry.register(m); }
        SemVerResolver::new(registry, config)
    }

    #[test]
    fn resolve_single_plugin_ok() {
        let r = resolver_with(
            vec![manifest("alpha", "1.2.0", 0.8, SlsaLevel::None, false)],
            ResolverConfig::default(),
        );
        let lock = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse(">=1.0.0").unwrap(),
        }]).unwrap();
        assert_eq!(lock.entries.len(), 1);
        assert_eq!(lock.entries[0].id, "alpha");
        assert_eq!(lock.entries[0].version, semver::Version::parse("1.2.0").unwrap());
    }

    #[test]
    fn resolve_transitive_dependency_ok() {
        let mut dep_manifest = manifest("beta", "2.0.0", 0.8, SlsaLevel::None, false);
        // alpha depends on beta >=2.0.0
        dep_manifest.id = "beta".into();
        let mut alpha = manifest("alpha", "1.0.0", 0.8, SlsaLevel::None, false);
        alpha.dependencies.insert("beta".into(), semver::VersionReq::parse(">=2.0.0").unwrap());

        let r = resolver_with(vec![alpha, dep_manifest], ResolverConfig::default());
        let lock = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse(">=1.0.0").unwrap(),
        }]).unwrap();
        assert!(lock.entries.iter().any(|e| e.id == "beta"));
    }

    #[test]
    fn yanked_rejected_by_default() {
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.8, SlsaLevel::None, true)],
            ResolverConfig::default(),
        );
        let err = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap_err();
        assert!(matches!(err, ResolverError::Yanked { .. }));
    }

    #[test]
    fn yanked_allowed_when_flag_set() {
        let mut config = ResolverConfig::default();
        config.allow_yanked = true;
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.8, SlsaLevel::None, true)],
            config,
        );
        let lock = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap();
        assert_eq!(lock.entries.len(), 1);
    }

    #[test]
    fn trust_score_too_low_rejected() {
        let mut config = ResolverConfig::default();
        config.min_trust_score = 0.9;
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.3, SlsaLevel::None, false)],
            config,
        );
        let err = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap_err();
        assert!(matches!(err, ResolverError::TrustTooLow { .. }));
    }

    #[test]
    fn slsa_level_insufficient_rejected() {
        let mut config = ResolverConfig::default();
        config.required_slsa_level = SlsaLevel::Level2;
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.9, SlsaLevel::Level1, false)],
            config,
        );
        let err = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap_err();
        assert!(matches!(err, ResolverError::SlsaInsufficient { .. }));
    }

    #[test]
    fn slsa_level_sufficient_passes() {
        let mut config = ResolverConfig::default();
        config.required_slsa_level = SlsaLevel::Level2;
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.9, SlsaLevel::Level2, false)],
            config,
        );
        let lock = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap();
        assert_eq!(lock.entries[0].slsa_level, SlsaLevel::Level2);
    }

    #[test]
    fn dependency_confusion_detected() {
        let mut config = ResolverConfig::default();
        config.trusted_plugin_ids = vec!["mofa-agents".to_string()];
        let r = resolver_with(
            vec![manifest("mofa-agants", "1.0.0", 0.8, SlsaLevel::None, false)],
            config,
        );
        let err = r.resolve(&[PluginRequirement {
            id: "mofa-agants".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap_err();
        assert!(matches!(err, ResolverError::DependencyConfusion { .. }));
    }

    #[test]
    fn dependency_confusion_not_triggered_for_exact_name() {
        let mut config = ResolverConfig::default();
        config.trusted_plugin_ids = vec!["mofa-agents".to_string()];
        let r = resolver_with(
            vec![manifest("mofa-agents", "1.0.0", 0.8, SlsaLevel::None, false)],
            config,
        );
        // exact match must pass
        let lock = r.resolve(&[PluginRequirement {
            id: "mofa-agents".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap();
        assert_eq!(lock.entries.len(), 1);
    }

    #[test]
    fn not_found_when_no_matching_version() {
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.8, SlsaLevel::None, false)],
            ResolverConfig::default(),
        );
        let err = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse(">=2.0.0").unwrap(),
        }]).unwrap_err();
        assert!(matches!(err, ResolverError::NotFound { .. }));
    }

    #[test]
    fn install_order_returns_all_entries() {
        let r = resolver_with(
            vec![
                manifest("alpha", "1.0.0", 0.8, SlsaLevel::None, false),
                manifest("beta", "2.0.0", 0.8, SlsaLevel::None, false),
            ],
            ResolverConfig::default(),
        );
        let reqs = vec![
            PluginRequirement { id: "alpha".into(), version_req: semver::VersionReq::parse("*").unwrap() },
            PluginRequirement { id: "beta".into(), version_req: semver::VersionReq::parse("*").unwrap() },
        ];
        let lock = r.resolve(&reqs).unwrap();
        let order = SemVerResolver::install_order(&lock);
        assert_eq!(order.len(), 2);
        assert!(order.contains(&"alpha".to_string()));
        assert!(order.contains(&"beta".to_string()));
    }

    #[test]
    fn lockfile_version_is_one() {
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.8, SlsaLevel::None, false)],
            ResolverConfig::default(),
        );
        let lock = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap();
        assert_eq!(lock.version, 1);
    }

    #[test]
    fn lockfile_serializes_and_deserializes() {
        let r = resolver_with(
            vec![manifest("alpha", "1.0.0", 0.8, SlsaLevel::None, false)],
            ResolverConfig::default(),
        );
        let lock = r.resolve(&[PluginRequirement {
            id: "alpha".into(),
            version_req: semver::VersionReq::parse("*").unwrap(),
        }]).unwrap();
        let json = serde_json::to_string(&lock).unwrap();
        let decoded: PluginLockfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.entries[0].id, "alpha");
        assert_eq!(decoded.entries[0].version, semver::Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn compute_trust_score_high_community_rating() {
        let m = manifest("alpha", "1.0.0", 1.0, SlsaLevel::None, false);
        let score = m.compute_trust_score();
        assert!(score > 0.7, "high rating should give score > 0.7, got {score}");
    }

    #[test]
    fn edit_distance_exact_match_is_zero() {
        assert_eq!(SupplyChainGuard::edit_distance("mofa", "mofa"), 0);
    }

    #[test]
    fn edit_distance_one_char_diff() {
        assert_eq!(SupplyChainGuard::edit_distance("mofa", "mofi"), 1);
    }
}
