//! Swarm integration helpers for plugin dependency resolution.

use mofa_plugins::{PluginLock, PluginRegistry, ResolveError, resolve_install_order};
use mofa_plugins::wasm_runtime::types::{PluginDep, PluginManifest};
use std::time::SystemTime;
use semver::Version;

/// Resolve plugin dependencies for a swarm run using the plugin registry.
///
/// This is a thin integration wrapper that constructs a registry from the
/// available manifests and applies the resolver with runtime and trust filters.
pub fn resolve_swarm_plugins(
    manifests: &[PluginManifest],
    roots: &[PluginDep],
    runtime_version: Option<&Version>,
    min_trust: Option<f32>,
) -> Result<PluginLock, ResolveError> {
    let mut registry = PluginRegistry::new();
    for manifest in manifests {
        registry.publish(manifest.clone());
    }

    let resolved = resolve_install_order(&registry, roots, runtime_version, min_trust)?;
    Ok(PluginLock {
        resolved,
        generated_at: SystemTime::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep(name: &str, req: &str) -> PluginDep {
        PluginDep::parse(name, req).unwrap()
    }

    fn manifest(name: &str, version: &str, deps: &[PluginDep]) -> PluginManifest {
        let mut manifest = PluginManifest::new_from_str(name, version).unwrap();
        for dep in deps {
            manifest = manifest.with_dependency(dep.clone());
        }
        manifest
    }

    #[test]
    fn test_resolve_swarm_plugins_success() {
        let manifests = vec![
            manifest("alpha", "1.0.0", &[dep("bravo", ">=1.0")]),
            manifest("bravo", "1.2.0", &[]),
        ];

        let lock = resolve_swarm_plugins(&manifests, &[dep("alpha", ">=1.0")], None, None)
            .expect("resolution should succeed");
        assert_eq!(lock.resolved.len(), 2);
    }
}
