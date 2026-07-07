// Demonstrates resolver behavior:
// - yanked/deprecated filtering
// - runtime version gating
// - trust score filtering
// - multi-root dependency resolution
use mofa_plugins::{resolve_with_options, PluginDep, PluginRegistry};
use mofa_plugins::wasm_runtime::types::PluginManifest;
use semver::Version;

fn dep(name: &str, req: &str) -> PluginDep {
    PluginDep::parse(name, req).expect("valid semver requirement")
}

fn manifest(name: &str, version: &str, deps: &[PluginDep]) -> PluginManifest {
    let mut manifest = PluginManifest::new_from_str(name, version)
        .expect("valid semver version")
        .with_dependency_list(deps);
    manifest
}

trait ManifestExt {
    fn with_dependency_list(self, deps: &[PluginDep]) -> Self;
}

impl ManifestExt for PluginManifest {
    fn with_dependency_list(mut self, deps: &[PluginDep]) -> Self {
        for dep in deps {
            self = self.with_dependency(dep.clone());
        }
        self
    }
}

fn main() {
    // Demo registry showing: yanked/deprecated filtering, runtime gating, and trust filtering.
    let mut registry = PluginRegistry::new();

    // alpha -> bravo >=1.0
    let mut alpha = manifest("alpha", "1.0.0", &[dep("bravo", ">=1.0")]);
    alpha.trust_score = 0.9;

    // bravo v1 is valid for runtime 1.5.0
    let mut bravo_v1 = manifest("bravo", "1.5.0", &[]);
    bravo_v1.trust_score = 0.8;
    bravo_v1.min_runtime_version = Some("1.0.0".to_string());

    // bravo v2 is yanked + requires runtime >=2.0, so it should be skipped.
    let mut bravo_v2 = manifest("bravo", "2.0.0", &[]).with_yanked(true);
    bravo_v2.trust_score = 0.95;
    bravo_v2.min_runtime_version = Some("2.0.0".to_string());

    // charlie -> bravo >=1.0 (meets trust threshold)
    let mut charlie = manifest("charlie", "1.0.0", &[dep("bravo", ">=1.0")]);
    charlie.trust_score = 0.6;

    // delta has two versions; trust filter should pick the higher-trust one.
    let low_trust = manifest("delta", "2.0.0", &[]).with_trust_score(0.2);
    let high_trust = manifest("delta", "1.0.0", &[]).with_trust_score(0.9);

    registry.publish(alpha);
    registry.publish(bravo_v1);
    registry.publish(bravo_v2);
    registry.publish(charlie);
    registry.publish(low_trust);
    registry.publish(high_trust);

    // Resolve with runtime 1.5.0 and min_trust=0.5.
    let runtime = Version::new(1, 5, 0);
    let resolved = resolve_with_options(
        &registry,
        &[
            dep("alpha", ">=1.0"),
            dep("charlie", ">=1.0"),
            dep("delta", ">=1.0"),
        ],
        Some(&runtime),
        Some(0.5),
    );

    match resolved {
        Ok(resolved) => {
            println!("Resolved dependencies (runtime={}, min_trust=0.5):", runtime);
            let mut names: Vec<_> = resolved.keys().cloned().collect();
            names.sort();
            for name in names {
                println!("- {}@{}", name, resolved[&name]);
            }
        }
        Err(err) => {
            eprintln!("Resolution failed: {err}");
        }
    }
}
