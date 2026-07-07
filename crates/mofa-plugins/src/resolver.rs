//! SemVer aware dependency resolution for marketplace plugins.
//!
//! Manifests store dependency requirements, resolution returns a plugin-version
//! map, capabilities can be composed from the selected set, and install helpers
//! perform trust gating and Ed25519 verification before writing artifacts to disk.
//!
//! The resolver backtracks across candidate versions to avoid false conflicts in
//! solvable graphs and produces a dependency safe install order.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::SystemTime;
use thiserror::Error;

use crate::wasm_runtime::{PluginCapability, PluginDep, PluginManifest};

pub type PluginId = String;
pub type ResolvedVersions = HashMap<PluginId, Version>;

/// Registry of available plugin manifests grouped by name.
#[derive(Debug, Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, Vec<PluginManifest>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&mut self, manifest: PluginManifest) {
        let versions = self.plugins.entry(manifest.name.clone()).or_default();
        versions.push(manifest);
        versions.sort_by(|a, b| a.version.cmp(&b.version));
    }

    pub fn resolve_one(&self, name: &str, req: &VersionReq) -> Option<&PluginManifest> {
        let requirements = vec![req.clone()];
        self.candidates_for(name, &requirements, None, None)
            .ok()?
            .into_iter()
            .next()
    }

    fn candidates_for(
        &self,
        name: &str,
        requirements: &[VersionReq],
        runtime_version: Option<&Version>,
        min_trust: Option<f32>,
    ) -> Result<Vec<&PluginManifest>, ResolveError> {
        let versions = self.plugins.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut candidates = Vec::new();
        for manifest in versions {
            if manifest.yanked {
                continue;
            }
            if !requirements.iter().all(|req| req.matches(&manifest.version)) {
                continue;
            }
            if let Some(threshold) = min_trust
                && manifest.trust_score < threshold
            {
                continue;
            }
            if let Some(runtime) = runtime_version
                && let Some(req) = manifest.min_runtime_version.as_deref()
            {
                let min_version =
                    Version::parse(req).map_err(|_| ResolveError::InvalidRuntimeVersion {
                        name: manifest.name.clone(),
                        value: req.to_string(),
                    })?;
                if runtime < &min_version {
                    continue;
                }
            }
            candidates.push(manifest);
        }

        candidates.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(candidates)
    }

    // Fetch the exact manifest selected by the resolver.
    fn get_version(&self, name: &str, version: &Version) -> Option<&PluginManifest> {
        self.plugins
            .get(name)?
            .iter()
            .find(|manifest| &manifest.version == version)
    }

    // Report installable versions using the same eligibility checks as resolution.
    fn available_versions_filtered(
        &self,
        name: &str,
        runtime_version: Option<&Version>,
        min_trust: Option<f32>,
    ) -> Result<Vec<String>, ResolveError> {
        let versions = self.plugins.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut filtered = Vec::new();
        for manifest in versions {
            if manifest.yanked {
                continue;
            }
            if let Some(threshold) = min_trust
                && manifest.trust_score < threshold
            {
                continue;
            }
            if let Some(runtime) = runtime_version
                && let Some(req) = manifest.min_runtime_version.as_deref()
            {
                let min_version =
                    Version::parse(req).map_err(|_| ResolveError::InvalidRuntimeVersion {
                        name: manifest.name.clone(),
                        value: req.to_string(),
                    })?;
                if runtime < &min_version {
                    continue;
                }
            }
            filtered.push(manifest.version.to_string());
        }
        Ok(filtered)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDep {
    pub name: String,
    pub version: Version,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginLock {
    pub resolved: Vec<ResolvedDep>,
    pub generated_at: SystemTime,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveError {
    #[error("Dependency not found: {name}")]
    NotFound { name: String },
    #[error("Version conflict for {name}: {required}, available={available:?}")]
    Conflict {
        name: String,
        required: String,
        available: Vec<String>,
    },
    #[error("Invalid runtime version requirement for {name}: {value}")]
    InvalidRuntimeVersion { name: String, value: String },
    #[error("Dependency cycle detected: {cycle:?}")]
    Cycle { cycle: Vec<String> },
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompositionError {
    #[error("Selected plugin version not found in registry: {name}@{version}")]
    MissingSelectedVersion { name: String, version: Version },
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VerificationError {
    #[error("Missing public key for plugin {name}")]
    MissingPublicKey { name: String },
    #[error("Invalid public key for plugin {name}")]
    InvalidPublicKey { name: String },
    #[error("Invalid signature encoding for plugin {name}")]
    InvalidSignature { name: String },
    #[error("Signature verification failed for plugin {name}")]
    VerificationFailed { name: String },
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InstallError {
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Verify(#[from] VerificationError),
    #[error("Missing artifact bytes for plugin {name}")]
    MissingArtifact { name: String },
    #[error("Selected plugin version not found in registry: {name}@{version}")]
    MissingSelectedVersion { name: String, version: Version },
    #[error("Failed to create install directory {path}: {source}")]
    CreateDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to write plugin artifact {path}: {source}")]
    WriteArtifact {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Default)]
pub struct PluginResolver;

impl PluginResolver {
    // Resolve using default filters only.
    pub fn resolve(
        registry: &PluginRegistry,
        roots: &[PluginDep],
    ) -> Result<ResolvedVersions, ResolveError> {
        resolve(registry, roots)
    }

    // Resolve while enforcing a minimum runtime version.
    pub fn resolve_with_runtime(
        registry: &PluginRegistry,
        roots: &[PluginDep],
        runtime_version: &Version,
    ) -> Result<ResolvedVersions, ResolveError> {
        resolve_with_runtime(registry, roots, runtime_version)
    }

    pub fn resolve_with_options(
        registry: &PluginRegistry,
        roots: &[PluginDep],
        runtime_version: Option<&Version>,
        min_trust: Option<f32>,
    ) -> Result<ResolvedVersions, ResolveError> {
        resolve_with_options(registry, roots, runtime_version, min_trust)
    }

    pub fn resolve_install_order(
        registry: &PluginRegistry,
        roots: &[PluginDep],
        runtime_version: Option<&Version>,
        min_trust: Option<f32>,
    ) -> Result<Vec<ResolvedDep>, ResolveError> {
        resolve_install_order(registry, roots, runtime_version, min_trust)
    }

    pub fn compose_capabilities(
        selected: &ResolvedVersions,
        registry: &PluginRegistry,
    ) -> Result<Vec<PluginCapability>, CompositionError> {
        compose_capabilities(selected, registry)
    }

    pub fn verify_plugin_signature(
        manifest: &PluginManifest,
        artifact_bytes: &[u8],
        public_key_hex: &str,
    ) -> Result<(), VerificationError> {
        verify_plugin_signature(manifest, artifact_bytes, public_key_hex)
    }

    pub fn install_resolved_plugins(
        registry: &PluginRegistry,
        roots: &[PluginDep],
        artifacts: &HashMap<String, Vec<u8>>,
        public_keys: &HashMap<String, String>,
        install_dir: &Path,
        runtime_version: Option<&Version>,
        min_trust: Option<f32>,
    ) -> Result<PluginLock, InstallError> {
        install_resolved_plugins(
            registry,
            roots,
            artifacts,
            public_keys,
            install_dir,
            runtime_version,
            min_trust,
        )
    }
}

pub type DependencyResolver = PluginResolver;

// Resolve with no runtime or trust filtering.
pub fn resolve(
    registry: &PluginRegistry,
    roots: &[PluginDep],
) -> Result<ResolvedVersions, ResolveError> {
    resolve_with_options(registry, roots, None, None)
}

// Resolve while enforcing a runtime floor.
pub fn resolve_with_runtime(
    registry: &PluginRegistry,
    roots: &[PluginDep],
    runtime_version: &Version,
) -> Result<ResolvedVersions, ResolveError> {
    resolve_with_options(registry, roots, Some(runtime_version), None)
}

// Shared entrypoint for all resolver filter combinations.
pub fn resolve_with_options(
    registry: &PluginRegistry,
    roots: &[PluginDep],
    runtime_version: Option<&Version>,
    min_trust: Option<f32>,
) -> Result<ResolvedVersions, ResolveError> {
    let (resolved, selected_manifests) = resolve_internal(registry, roots, runtime_version, min_trust)?;
    if let Some(cycle) = detect_cycle(&selected_manifests) {
        return Err(ResolveError::Cycle { cycle });
    }

    Ok(resolved)
}

// Return the resolved set in dependency-safe install order.
pub fn resolve_install_order(
    registry: &PluginRegistry,
    roots: &[PluginDep],
    runtime_version: Option<&Version>,
    min_trust: Option<f32>,
) -> Result<Vec<ResolvedDep>, ResolveError> {
    let (resolved, selected_manifests) = resolve_internal(registry, roots, runtime_version, min_trust)?;
    if let Some(cycle) = detect_cycle(&selected_manifests) {
        return Err(ResolveError::Cycle { cycle });
    }

    topological_install_order(&resolved, &selected_manifests)
}

// Merge unique capabilities from the resolved plugin set.
pub fn compose_capabilities(
    selected: &ResolvedVersions,
    registry: &PluginRegistry,
) -> Result<Vec<PluginCapability>, CompositionError> {
    let mut capabilities = Vec::new();
    let mut names: Vec<_> = selected.keys().cloned().collect();
    names.sort();

    for name in names {
        let version = selected.get(&name).expect("name collected from selected");
        let manifest = registry
            .get_version(&name, version)
            .ok_or_else(|| CompositionError::MissingSelectedVersion {
                name: name.clone(),
                version: version.clone(),
            })?;
        for capability in &manifest.capabilities {
            if !capabilities.contains(capability) {
                capabilities.push(capability.clone());
            }
        }
    }

    Ok(capabilities)
}

// Verify an artifact against the manifest signature using the caller-provided public key.
pub fn verify_plugin_signature(
    manifest: &PluginManifest,
    artifact_bytes: &[u8],
    public_key_hex: &str,
) -> Result<(), VerificationError> {
    let public_key_bytes =
        hex::decode(public_key_hex).map_err(|_| VerificationError::InvalidPublicKey {
            name: manifest.name.clone(),
        })?;
    let public_key_bytes: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| VerificationError::InvalidPublicKey {
            name: manifest.name.clone(),
        })?;
    let public_key = VerifyingKey::from_bytes(&public_key_bytes).map_err(|_| {
        VerificationError::InvalidPublicKey {
            name: manifest.name.clone(),
        }
    })?;

    let signature_bytes =
        hex::decode(&manifest.signature).map_err(|_| VerificationError::InvalidSignature {
            name: manifest.name.clone(),
        })?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| {
        VerificationError::InvalidSignature {
            name: manifest.name.clone(),
        }
    })?;

    let artifact_hash = Sha256::digest(artifact_bytes);
    public_key
        .verify(&artifact_hash, &signature)
        .map_err(|_| VerificationError::VerificationFailed {
            name: manifest.name.clone(),
        })
}

// Resolve, verify, and write artifacts in dependency-safe order.
pub fn install_resolved_plugins(
    registry: &PluginRegistry,
    roots: &[PluginDep],
    artifacts: &HashMap<String, Vec<u8>>,
    public_keys: &HashMap<String, String>,
    install_dir: &Path,
    runtime_version: Option<&Version>,
    min_trust: Option<f32>,
) -> Result<PluginLock, InstallError> {
    let ordered = resolve_install_order(registry, roots, runtime_version, min_trust)?;

    std::fs::create_dir_all(install_dir).map_err(|source| InstallError::CreateDir {
        path: install_dir.display().to_string(),
        source,
    })?;

    for dep in &ordered {
        let manifest = registry
            .get_version(&dep.name, &dep.version)
            .ok_or_else(|| InstallError::MissingSelectedVersion {
                name: dep.name.clone(),
                version: dep.version.clone(),
            })?;
        let artifact = artifacts
            .get(&dep.name)
            .ok_or_else(|| InstallError::MissingArtifact {
                name: dep.name.clone(),
            })?;
        let public_key = public_keys
            .get(&dep.name)
            .ok_or_else(|| VerificationError::MissingPublicKey {
                name: dep.name.clone(),
            })?;

        verify_plugin_signature(manifest, artifact, public_key)?;

        let artifact_path = install_dir.join(format!("{}-{}.wasm", dep.name, dep.version));
        std::fs::write(&artifact_path, artifact).map_err(|source| InstallError::WriteArtifact {
            path: artifact_path.display().to_string(),
            source,
        })?;
    }

    Ok(PluginLock {
        resolved: ordered,
        generated_at: SystemTime::now(),
    })
}

// Seed the solver with root constraints and defer version choice to backtracking.
fn resolve_internal(
    registry: &PluginRegistry,
    roots: &[PluginDep],
    runtime_version: Option<&Version>,
    min_trust: Option<f32>,
) -> Result<(ResolvedVersions, HashMap<String, PluginManifest>), ResolveError> {
    let mut constraints: HashMap<String, Vec<VersionReq>> = HashMap::new();
    let mut queue: VecDeque<PluginDep> = VecDeque::from(roots.to_vec());

    while let Some(dep) = queue.pop_front() {
        constraints
            .entry(dep.name.clone())
            .or_default()
            .push(dep.req.clone());
    }

    solve_versions(
        registry,
        &constraints,
        &HashMap::new(),
        runtime_version,
        min_trust,
    )
}

// Explore candidate versions package-by-package until all constraints are satisfied.
fn solve_versions(
    registry: &PluginRegistry,
    constraints: &HashMap<String, Vec<VersionReq>>,
    selected: &HashMap<String, PluginManifest>,
    runtime_version: Option<&Version>,
    min_trust: Option<f32>,
) -> Result<(ResolvedVersions, HashMap<String, PluginManifest>), ResolveError> {
    if let Some(name) = inconsistent_selected_package(constraints, selected) {
        let requirements = constraints.get(&name).cloned().unwrap_or_default();
        return Err(ResolveError::Conflict {
            name: name.clone(),
            required: format_requirements(&requirements),
            available: registry.available_versions_filtered(&name, runtime_version, min_trust)?,
        });
    }

    let Some(next_name) = select_next_package(registry, constraints, selected, runtime_version, min_trust)? else {
        let mut resolved = ResolvedVersions::new();
        for (name, manifest) in selected {
            resolved.insert(name.clone(), manifest.version.clone());
        }
        return Ok((resolved, selected.clone()));
    };

    let requirements = constraints.get(&next_name).cloned().unwrap_or_default();
    let candidates = registry.candidates_for(&next_name, &requirements, runtime_version, min_trust)?;
    if candidates.is_empty() {
        if !registry.plugins.contains_key(&next_name) {
            return Err(ResolveError::NotFound {
                name: next_name.clone(),
            });
        }
        return Err(ResolveError::Conflict {
            name: next_name.clone(),
            required: format_requirements(&requirements),
            available: registry.available_versions_filtered(&next_name, runtime_version, min_trust)?,
        });
    }

    let mut last_err = None;
    for candidate in candidates {
        let mut next_selected = selected.clone();
        next_selected.insert(next_name.clone(), candidate.clone());

        let mut next_constraints = constraints.clone();
        for dep in candidate.dependency_requirements() {
            next_constraints.entry(dep.name).or_default().push(dep.req);
        }

        match solve_versions(
            registry,
            &next_constraints,
            &next_selected,
            runtime_version,
            min_trust,
        ) {
            Ok(solution) => return Ok(solution),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| ResolveError::Conflict {
        name: next_name.clone(),
        required: format_requirements(&requirements),
        available: registry
            .available_versions_filtered(&next_name, runtime_version, min_trust)
            .unwrap_or_default(),
    }))
}

// Pick the unresolved package with the fewest viable candidates first.
fn select_next_package(
    registry: &PluginRegistry,
    constraints: &HashMap<String, Vec<VersionReq>>,
    selected: &HashMap<String, PluginManifest>,
    runtime_version: Option<&Version>,
    min_trust: Option<f32>,
) -> Result<Option<String>, ResolveError> {
    let mut best: Option<(String, usize)> = None;

    for (name, requirements) in constraints {
        if selected.contains_key(name) {
            continue;
        }

        let candidate_count = registry
            .candidates_for(name, requirements, runtime_version, min_trust)?
            .len();
        match &best {
            Some((best_name, best_count))
                if candidate_count > *best_count
                    || (candidate_count == *best_count && name >= best_name) => {}
            _ => best = Some((name.clone(), candidate_count)),
        }
    }

    Ok(best.map(|(name, _)| name))
}

fn inconsistent_selected_package(
    constraints: &HashMap<String, Vec<VersionReq>>,
    selected: &HashMap<String, PluginManifest>,
) -> Option<String> {
    for (name, requirements) in constraints {
        if let Some(manifest) = selected.get(name)
            && !requirements.iter().all(|req| req.matches(&manifest.version))
        {
            return Some(name.clone());
        }
    }

    None
}

fn format_requirements(requirements: &[VersionReq]) -> String {
    requirements
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" && ")
}

// Convert the resolved graph into an install order where dependencies come first.
fn topological_install_order(
    resolved: &ResolvedVersions,
    selected_manifests: &HashMap<String, PluginManifest>,
) -> Result<Vec<ResolvedDep>, ResolveError> {
    let mut indegree: HashMap<String, usize> = resolved.keys().cloned().map(|name| (name, 0)).collect();
    let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();

    for (name, manifest) in selected_manifests {
        let mut seen = HashSet::new();
        for dep in manifest.dependency_requirements() {
            if !resolved.contains_key(&dep.name) || !seen.insert(dep.name.clone()) {
                continue;
            }
            outgoing.entry(dep.name.clone()).or_default().push(name.clone());
            if let Some(entry) = indegree.get_mut(name) {
                *entry += 1;
            }
        }
    }

    let mut ready: Vec<String> = indegree
        .iter()
        .filter_map(|(name, degree)| (*degree == 0).then_some(name.clone()))
        .collect();
    ready.sort();

    let mut queue = VecDeque::from(ready);
    let mut ordered = Vec::with_capacity(resolved.len());

    while let Some(name) = queue.pop_front() {
        ordered.push(ResolvedDep {
            version: resolved.get(&name).expect("resolved entry exists").clone(),
            name: name.clone(),
        });

        if let Some(children) = outgoing.get(&name) {
            for child in children {
                if let Some(entry) = indegree.get_mut(child) {
                    *entry -= 1;
                    if *entry == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }
    }

    if ordered.len() != resolved.len() {
        let cycle: Vec<String> = indegree
            .into_iter()
            .filter_map(|(name, degree)| (degree > 0).then_some(name))
            .collect();
        return Err(ResolveError::Cycle { cycle });
    }

    Ok(ordered)
}

// Detect cycles on the selected graph so cycle errors only reflect chosen versions.
fn detect_cycle(selected: &HashMap<String, PluginManifest>) -> Option<Vec<String>> {
    let mut visiting: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();

    fn dfs(
        name: &str,
        selected: &HashMap<String, PluginManifest>,
        visiting: &mut HashMap<String, usize>,
        stack: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if let Some(&idx) = visiting.get(name) {
            return Some(stack[idx..].to_vec());
        }

        visiting.insert(name.to_string(), stack.len());
        stack.push(name.to_string());

        if let Some(manifest) = selected.get(name) {
            for dep in manifest.dependency_requirements() {
                if selected.contains_key(&dep.name)
                    && let Some(cycle) = dfs(&dep.name, selected, visiting, stack)
                {
                    return Some(cycle);
                }
            }
        }

        visiting.remove(name);
        stack.pop();
        None
    }

    let names: Vec<String> = selected.keys().cloned().collect();
    for name in names {
        if let Some(cycle) = dfs(&name, selected, &mut visiting, &mut stack) {
            return Some(cycle);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm_runtime::types::AuditStatus;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::TempDir;

    fn manifest(name: &str, version: &str, deps: &[PluginDep]) -> PluginManifest {
        let mut manifest = PluginManifest::new_from_str(name, version).unwrap();
        for dep in deps {
            manifest = manifest.with_dependency(dep.clone());
        }
        manifest
    }

    fn dep(name: &str, req: &str) -> PluginDep {
        PluginDep::parse(name, req).unwrap()
    }

    #[test]
    fn test_resolve_single_dep() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("alpha", "1.0.0", &[]));

        let resolved = resolve(&registry, &[dep("alpha", ">=1.0")]).unwrap();
        assert_eq!(resolved.get("alpha"), Some(&Version::new(1, 0, 0)));
    }

    #[test]
    fn test_resolve_picks_highest_matching() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("alpha", "1.0.0", &[]));
        registry.publish(manifest("alpha", "1.2.0", &[]));

        let resolved = resolve(&registry, &[dep("alpha", ">=1.0")]).unwrap();
        assert_eq!(resolved.get("alpha"), Some(&Version::new(1, 2, 0)));
    }

    #[test]
    fn test_resolve_transitive_deps() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("gamma", "1.0.0", &[]));
        registry.publish(manifest("beta", "1.0.0", &[dep("gamma", "^1.0")]));
        registry.publish(manifest("alpha", "1.0.0", &[dep("beta", ">=1.0")]));

        let resolved = resolve(&registry, &[dep("alpha", ">=1.0")]).unwrap();
        assert_eq!(resolved.len(), 3);
        assert_eq!(resolved.get("gamma"), Some(&Version::new(1, 0, 0)));
    }

    #[test]
    fn test_resolve_missing_dep_errors() {
        let registry = PluginRegistry::new();
        let err = resolve(&registry, &[dep("missing", ">=1.0")]).unwrap_err();
        assert!(matches!(err, ResolveError::NotFound { name } if name == "missing"));
    }

    #[test]
    fn test_resolve_version_conflict_errors() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("core", "1.0.0", &[]));
        registry.publish(manifest("core", "2.0.0", &[]));

        let err = resolve(&registry, &[dep("core", "<2.0"), dep("core", ">=2.0")]).unwrap_err();
        assert!(matches!(err, ResolveError::Conflict { name, .. } if name == "core"));
    }

    #[test]
    fn test_resolve_semver_req_respected() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("agent", "1.2.0", &[]));
        registry.publish(manifest("agent", "2.0.0", &[]));

        let resolved = resolve(&registry, &[dep("agent", ">=1.2.0, <2.0")]).unwrap();
        assert_eq!(resolved.get("agent"), Some(&Version::new(1, 2, 0)));
    }

    #[test]
    fn test_resolve_skips_yanked_versions() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("alpha", "2.0.0", &[]).with_yanked(true));
        registry.publish(manifest("alpha", "1.9.0", &[]).with_deprecated(true));
        registry.publish(manifest("alpha", "1.5.0", &[]));

        let resolved = resolve(&registry, &[dep("alpha", ">=1.0")]).unwrap();
        assert_eq!(resolved.get("alpha"), Some(&Version::new(1, 9, 0)));
    }

    #[test]
    fn test_resolve_with_min_runtime_version() {
        let mut registry = PluginRegistry::new();
        let mut v2 = manifest("beta", "2.0.0", &[]);
        v2.min_runtime_version = Some("2.0.0".to_string());
        let mut v1 = manifest("beta", "1.0.0", &[]);
        v1.min_runtime_version = Some("1.0.0".to_string());
        registry.publish(v2);
        registry.publish(v1);

        let runtime = Version::new(1, 5, 0);
        let resolved = resolve_with_options(&registry, &[dep("beta", ">=1.0")], Some(&runtime), None)
            .unwrap();
        assert_eq!(resolved.get("beta"), Some(&Version::new(1, 0, 0)));
    }

    #[test]
    fn test_resolve_with_trust_score_filter() {
        let mut registry = PluginRegistry::new();
        let low = manifest("gamma", "2.0.0", &[]).with_trust_score(0.2);
        let high = manifest("gamma", "1.5.0", &[])
            .with_community_rating(0.9)
            .with_download_count(10_000)
            .with_audit_status(AuditStatus::Passed);
        registry.publish(low);
        registry.publish(high);

        let resolved =
            resolve_with_options(&registry, &[dep("gamma", ">=1.0")], None, Some(0.5)).unwrap();
        assert_eq!(resolved.get("gamma"), Some(&Version::new(1, 5, 0)));
    }

    #[test]
    fn test_large_graph_resolves() {
        let mut registry = PluginRegistry::new();
        let node_count = 25;
        for i in (0..node_count).rev() {
            let name = format!("node-{i}");
            let deps = if i + 1 < node_count {
                vec![dep(&format!("node-{}", i + 1), ">=1.0")]
            } else {
                Vec::new()
            };
            registry.publish(manifest(&name, "1.0.0", &deps));
        }

        let resolved = resolve(&registry, &[dep("node-0", ">=1.0")]).unwrap();
        assert_eq!(resolved.len(), node_count);
    }

    #[test]
    fn test_resolve_diamond_solvable() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("alpha", "1.0.0", &[dep("bravo", ">=1.0")]));
        registry.publish(manifest("charlie", "1.0.0", &[dep("bravo", ">=1.1")]));
        registry.publish(manifest("bravo", "1.2.0", &[]));
        registry.publish(manifest("bravo", "1.0.0", &[]));

        let resolved = resolve(&registry, &[dep("alpha", ">=1.0"), dep("charlie", ">=1.0")])
            .unwrap();
        assert_eq!(resolved.get("bravo"), Some(&Version::new(1, 2, 0)));
    }

    #[test]
    fn test_resolve_diamond_unsolvable() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("alpha", "1.0.0", &[dep("bravo", ">=2.0")]));
        registry.publish(manifest("charlie", "1.0.0", &[dep("bravo", "<2.0")]));
        registry.publish(manifest("bravo", "1.5.0", &[]));
        registry.publish(manifest("bravo", "2.1.0", &[]));

        let err = resolve(&registry, &[dep("alpha", ">=1.0"), dep("charlie", ">=1.0")])
            .unwrap_err();
        assert!(matches!(err, ResolveError::Conflict { name, .. } if name == "bravo"));
    }

    #[test]
    fn test_publish_cycle_detection() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("alpha", "1.0.0", &[dep("bravo", ">=1.0")]));
        registry.publish(manifest("bravo", "1.0.0", &[dep("alpha", ">=1.0")]));

        let err = resolve(&registry, &[dep("alpha", ">=1.0")]).unwrap_err();
        assert!(matches!(err, ResolveError::Cycle { .. }));
    }

    #[test]
    fn test_resolve_backtracks_to_satisfy_later_constraint() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("app", "1.0.0", &[dep("shared", ">=1.0, <3.0")]));
        registry.publish(manifest("addon", "1.0.0", &[dep("shared", "<2.0")]));
        registry.publish(manifest("shared", "2.0.0", &[dep("core", ">=2.0")]));
        registry.publish(manifest("shared", "1.5.0", &[dep("core", ">=1.0, <2.0")]));
        registry.publish(manifest("core", "2.0.0", &[]));
        registry.publish(manifest("core", "1.0.0", &[]));

        let resolved = resolve(&registry, &[dep("app", ">=1.0"), dep("addon", ">=1.0")])
            .unwrap();
        assert_eq!(resolved.get("shared"), Some(&Version::new(1, 5, 0)));
        assert_eq!(resolved.get("core"), Some(&Version::new(1, 0, 0)));
    }

    #[test]
    fn test_resolve_install_order_places_dependencies_first() {
        let mut registry = PluginRegistry::new();
        registry.publish(manifest("alpha", "1.0.0", &[dep("bravo", ">=1.0")]));
        registry.publish(manifest("bravo", "1.0.0", &[dep("charlie", ">=1.0")]));
        registry.publish(manifest("charlie", "1.0.0", &[]));

        let ordered = resolve_install_order(&registry, &[dep("alpha", ">=1.0")], None, None)
            .unwrap();
        let names: Vec<_> = ordered.iter().map(|dep| dep.name.as_str()).collect();
        assert_eq!(names, vec!["charlie", "bravo", "alpha"]);
    }

    #[test]
    fn test_manual_trust_score_override_is_stable() {
        let manifest = manifest("alpha", "1.0.0", &[])
            .with_trust_score(0.2)
            .with_community_rating(1.0)
            .with_download_count(1_000_000)
            .with_audit_status(AuditStatus::Passed);

        assert_eq!(manifest.trust_score, 0.2);
    }

    #[test]
    fn test_compose_capabilities_from_selected_plugins() {
        let mut registry = PluginRegistry::new();
        registry.publish(
            manifest("alpha", "1.0.0", &[])
                .with_capability(PluginCapability::HttpClient)
                .with_capability(PluginCapability::Custom("summarise".to_string())),
        );
        registry.publish(
            manifest("bravo", "1.0.0", &[])
                .with_capability(PluginCapability::HttpClient)
                .with_capability(PluginCapability::Storage),
        );

        let mut selected = ResolvedVersions::new();
        selected.insert("alpha".to_string(), Version::new(1, 0, 0));
        selected.insert("bravo".to_string(), Version::new(1, 0, 0));

        let capabilities = compose_capabilities(&selected, &registry).unwrap();
        assert!(capabilities.contains(&PluginCapability::HttpClient));
        assert!(capabilities.contains(&PluginCapability::Storage));
        assert!(capabilities.contains(&PluginCapability::Custom("summarise".to_string())));
    }

    #[test]
    fn test_verify_plugin_signature() {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let public_key = hex::encode(signing_key.verifying_key().as_bytes());
        let artifact = b"plugin-bytes";
        let artifact_hash = Sha256::digest(artifact);
        let signature = signing_key.sign(&artifact_hash);

        let manifest =
            manifest("alpha", "1.0.0", &[]).with_signature(hex::encode(signature.to_bytes()));
        verify_plugin_signature(&manifest, artifact, &public_key).unwrap();
    }

    #[test]
    fn test_install_resolved_plugins() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let public_key = hex::encode(signing_key.verifying_key().as_bytes());
        let artifact = b"wasm-module".to_vec();
        let artifact_hash = Sha256::digest(&artifact);
        let signature = signing_key.sign(&artifact_hash);

        let manifest = manifest("alpha", "1.0.0", &[])
            .with_signature(hex::encode(signature.to_bytes()))
            .with_community_rating(0.8)
            .with_download_count(10_000)
            .with_audit_status(AuditStatus::Passed);

        let mut registry = PluginRegistry::new();
        registry.publish(manifest);

        let mut artifacts = HashMap::new();
        artifacts.insert("alpha".to_string(), artifact);
        let mut public_keys = HashMap::new();
        public_keys.insert("alpha".to_string(), public_key);

        let temp = TempDir::new().unwrap();
        let lock = install_resolved_plugins(
            &registry,
            &[dep("alpha", ">=1.0")],
            &artifacts,
            &public_keys,
            temp.path(),
            None,
            Some(0.5),
        )
        .unwrap();

        assert_eq!(lock.resolved.len(), 1);
        assert!(temp.path().join("alpha-1.0.0.wasm").exists());
    }
}
