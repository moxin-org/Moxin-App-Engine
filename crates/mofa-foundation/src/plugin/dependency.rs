use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub dependencies: Vec<PluginDependency>,
    pub optional_dependencies: Vec<PluginDependency>,
    pub conflicts: Vec<PluginConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    pub plugin_id: String,
    pub version_constraint: VersionConstraint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionConstraint {
    pub min_version: Option<String>,
    pub max_version: Option<String>,
    pub exact_version: Option<String>,
}

/// A parsed semantic version (major.minor.patch).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl SemanticVersion {
    fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .split('.')
            .collect();
        if parts.len() != 3 {
            return None;
        }
        Some(Self {
            major: parts[0].parse().ok()?,
            minor: parts[1].parse().ok()?,
            patch: parts[2].parse().ok()?,
        })
    }
}

impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl VersionConstraint {
    pub fn exact(version: impl Into<String>) -> Self {
        Self {
            min_version: None,
            max_version: None,
            exact_version: Some(version.into()),
        }
    }

    pub fn range(min: impl Into<String>, max: impl Into<String>) -> Self {
        Self {
            min_version: Some(min.into()),
            max_version: Some(max.into()),
            exact_version: None,
        }
    }

    pub fn caret(version: impl Into<String>) -> Self {
        Self {
            min_version: Some(format!("^{}", version.into())),
            max_version: None,
            exact_version: None,
        }
    }

    pub fn satisfies(&self, version: &str) -> bool {
        if let Some(ref exact) = self.exact_version {
            // Exact match: strip leading "="
            let exact_clean = exact.trim_start_matches('=');
            return version == exact_clean;
        }

        let ver = match SemanticVersion::parse(version) {
            Some(v) => v,
            None => return false,
        };

        if let Some(ref min) = self.min_version {
            // Caret constraint: ^major.minor.patch means >=min <(major+1).0.0
            if min.starts_with('^') {
                let base = match SemanticVersion::parse(min.trim_start_matches('^')) {
                    Some(v) => v,
                    None => return false,
                };
                let max = SemanticVersion {
                    major: base.major + 1,
                    minor: 0,
                    patch: 0,
                };
                return ver >= base && ver < max;
            }
            // Range: min is ">=" prefixed
            let min_clean = min.trim_start_matches(">=");
            if matches!(SemanticVersion::parse(min_clean), Some(min_ver) if ver < min_ver) {
                return false;
            }
        }

        if let Some(ref max) = self.max_version {
            // max is "<" prefixed
            let max_clean = max.trim_start_matches('<');
            if matches!(SemanticVersion::parse(max_clean), Some(max_ver) if ver >= max_ver) {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConflict {
    pub plugin_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginNode {
    pub manifest: PluginManifest,
    pub state: PluginLoadState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginLoadState {
    NotLoaded,
    Loading,
    Loaded,
    Failed(String),
}

pub struct DependencyGraph {
    pub(crate) nodes: HashMap<String, PluginNode>,
    edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    pub fn add_plugin(&mut self, manifest: PluginManifest) -> Result<(), DependencyError> {
        let plugin_id = manifest.plugin_id.clone();

        if self.nodes.contains_key(&plugin_id) {
            return Err(DependencyError::DuplicatePlugin(plugin_id));
        }

        self.nodes.insert(
            plugin_id.clone(),
            PluginNode {
                manifest: manifest.clone(),
                state: PluginLoadState::NotLoaded,
            },
        );

        let dependencies: Vec<String> = manifest
            .dependencies
            .iter()
            .map(|d| d.plugin_id.clone())
            .collect();

        self.edges.insert(plugin_id, dependencies);

        Ok(())
    }

    pub fn resolve_load_order(&self) -> Result<Vec<String>, DependencyError> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        let mut result: Vec<String> = Vec::new();

        for (plugin_id, deps) in &self.edges {
            let degree = deps.len();
            in_degree.insert(plugin_id.clone(), degree);
        }

        for (plugin_id, degree) in &in_degree {
            if *degree == 0 {
                queue.push_back(plugin_id.clone());
            }
        }

        while let Some(plugin_id) = queue.pop_front() {
            result.push(plugin_id.clone());

            for (other_id, deps) in &self.edges {
                if deps.contains(&plugin_id) && in_degree.contains_key(other_id.as_str()) {
                    let degree = in_degree.get_mut(other_id).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(other_id.clone());
                    }
                }
            }
        }

        if result.len() != self.nodes.len() {
            return Err(DependencyError::CircularDependency);
        }

        // Check for conflicts among the resolved plugins
        let present: HashSet<&String> = result.iter().collect();
        for (plugin_id, node) in &self.nodes {
            for conflict in &node.manifest.conflicts {
                if present.contains(&conflict.plugin_id) && present.contains(plugin_id) {
                    return Err(DependencyError::Conflict {
                        plugin: plugin_id.clone(),
                        conflicting: conflict.plugin_id.clone(),
                        reason: conflict.reason.clone(),
                    });
                }
            }
        }

        Ok(result)
    }

    pub fn detect_cycles(&self) -> Result<(), DependencyError> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut recursion_stack: HashSet<String> = HashSet::new();

        for plugin_id in self.nodes.keys() {
            if !visited.contains(plugin_id) {
                self.detect_cycles_helper(plugin_id, &mut visited, &mut recursion_stack)?;
            }
        }

        Ok(())
    }

    fn detect_cycles_helper(
        &self,
        plugin_id: &str,
        visited: &mut HashSet<String>,
        recursion_stack: &mut HashSet<String>,
    ) -> Result<(), DependencyError> {
        visited.insert(plugin_id.to_string());
        recursion_stack.insert(plugin_id.to_string());

        if let Some(deps) = self.edges.get(plugin_id) {
            for dep in deps {
                if !visited.contains(dep) {
                    self.detect_cycles_helper(dep, visited, recursion_stack)?;
                } else if recursion_stack.contains(dep) {
                    return Err(DependencyError::CircularDependency);
                }
            }
        }

        recursion_stack.remove(plugin_id);
        Ok(())
    }

    pub fn get_dependencies(&self, plugin_id: &str) -> Option<Vec<String>> {
        self.edges.get(plugin_id).cloned()
    }

    pub fn get_dependents(&self, plugin_id: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|(_, deps)| deps.contains(&plugin_id.to_string()))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn validate(&self) -> Result<(), DependencyError> {
        self.detect_cycles()?;

        for (plugin_id, node) in &self.nodes {
            for dep in &node.manifest.dependencies {
                if !self.nodes.contains_key(&dep.plugin_id) {
                    return Err(DependencyError::MissingDependency {
                        plugin: plugin_id.clone(),
                        dependency: dep.plugin_id.clone(),
                    });
                }

                let dep_version = &self.nodes[&dep.plugin_id].manifest.version;
                if !dep.version_constraint.satisfies(dep_version) {
                    return Err(DependencyError::VersionMismatch {
                        plugin: plugin_id.clone(),
                        dependency: dep.plugin_id.clone(),
                        required: format!("{:?}", dep.version_constraint),
                        found: dep_version.clone(),
                    });
                }
            }

            for conflict in &node.manifest.conflicts {
                if self.nodes.contains_key(&conflict.plugin_id) {
                    return Err(DependencyError::Conflict {
                        plugin: plugin_id.clone(),
                        conflicting: conflict.plugin_id.clone(),
                        reason: conflict.reason.clone(),
                    });
                }
            }
        }

        Ok(())
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DependencyError {
    #[error("Circular dependency detected")]
    CircularDependency,

    #[error("Duplicate plugin: {0}")]
    DuplicatePlugin(String),

    #[error("Missing dependency: {plugin} requires {dependency}")]
    MissingDependency { plugin: String, dependency: String },

    #[error("Version mismatch: {plugin} requires {dependency} but found {found}")]
    VersionMismatch {
        plugin: String,
        dependency: String,
        required: String,
        found: String,
    },

    #[error("Conflict: {plugin} conflicts with {conflicting}")]
    Conflict {
        plugin: String,
        conflicting: String,
        reason: Option<String>,
    },
}

pub type DependencyResult<T> = Result<T, DependencyError>;

pub struct PluginRegistry<G: PluginRegistryStorage> {
    graph: DependencyGraph,
    storage: G,
    pub(crate) loaded: HashSet<String>,
}

impl<G: PluginRegistryStorage> PluginRegistry<G> {
    pub fn new(storage: G) -> Self {
        Self {
            graph: DependencyGraph::new(),
            storage,
            loaded: HashSet::new(),
        }
    }

    pub fn register(&mut self, manifest: PluginManifest) -> DependencyResult<()> {
        self.graph.add_plugin(manifest.clone())?;
        self.storage.save_manifest(manifest)?;
        Ok(())
    }

    pub fn load_plugin(&mut self, plugin_id: &str) -> DependencyResult<()> {
        let mut to_load: Vec<String> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        self.collect_transitive_deps(plugin_id, &mut visited, &mut to_load)?;

        for id in to_load {
            self.load_single_plugin(&id)?;
        }
        Ok(())
    }

    /// Collect `plugin_id` and all its transitive dependencies in load order (deps first).
    fn collect_transitive_deps(
        &self,
        plugin_id: &str,
        visited: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> DependencyResult<()> {
        if visited.contains(plugin_id) {
            return Ok(());
        }
        visited.insert(plugin_id.to_string());

        if let Some(deps) = self.graph.get_dependencies(plugin_id) {
            for dep in deps {
                self.collect_transitive_deps(&dep, visited, order)?;
            }
        }
        // Push after all deps so deps come first
        order.push(plugin_id.to_string());
        Ok(())
    }

    fn load_single_plugin(&mut self, plugin_id: &str) -> DependencyResult<()> {
        if self.loaded.contains(plugin_id) {
            return Ok(()); // already loaded — lazy skip
        }
        if let Some(node) = self.graph.nodes.get_mut(plugin_id) {
            node.state = PluginLoadState::Loading;
            node.state = PluginLoadState::Loaded;
            self.loaded.insert(plugin_id.to_string());
            Ok(())
        } else {
            Err(DependencyError::MissingDependency {
                plugin: plugin_id.to_string(),
                dependency: String::new(),
            })
        }
    }

    pub fn is_loaded(&self, plugin_id: &str) -> bool {
        self.loaded.contains(plugin_id)
    }

    pub fn get_load_order(&self) -> DependencyResult<Vec<String>> {
        self.graph.resolve_load_order()
    }

    pub fn validate_all(&self) -> DependencyResult<()> {
        self.graph.validate()
    }
}

pub trait PluginRegistryStorage: Send + Sync {
    fn save_manifest(&self, manifest: PluginManifest) -> Result<(), DependencyError>;
    fn load_manifest(&self, plugin_id: &str) -> Result<Option<PluginManifest>, DependencyError>;
    fn list_plugins(&self) -> Result<Vec<String>, DependencyError>;
}

pub struct InMemoryPluginStorage {
    manifests: Mutex<HashMap<String, PluginManifest>>,
}

impl InMemoryPluginStorage {
    pub fn new() -> Self {
        Self {
            manifests: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryPluginStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistryStorage for InMemoryPluginStorage {
    fn save_manifest(&self, manifest: PluginManifest) -> Result<(), DependencyError> {
        self.manifests
            .lock()
            .expect("mutex not poisoned")
            .insert(manifest.plugin_id.clone(), manifest);
        Ok(())
    }

    fn load_manifest(&self, plugin_id: &str) -> Result<Option<PluginManifest>, DependencyError> {
        Ok(self
            .manifests
            .lock()
            .expect("mutex not poisoned")
            .get(plugin_id)
            .cloned())
    }

    fn list_plugins(&self) -> Result<Vec<String>, DependencyError> {
        Ok(self
            .manifests
            .lock()
            .expect("mutex not poisoned")
            .keys()
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest(
        id: &str,
        version: &str,
        deps: Vec<(&str, &str)>,
        conflicts: Vec<&str>,
    ) -> PluginManifest {
        PluginManifest {
            plugin_id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            dependencies: deps
                .into_iter()
                .map(|(dep_id, constraint)| PluginDependency {
                    plugin_id: dep_id.to_string(),
                    version_constraint: VersionConstraint::exact(constraint),
                })
                .collect(),
            optional_dependencies: Vec::new(),
            conflicts: conflicts
                .into_iter()
                .map(|c| PluginConflict {
                    plugin_id: c.to_string(),
                    reason: None,
                })
                .collect(),
        }
    }

    #[test]
    fn test_topological_sort_simple_chain() {
        // A depends on B, B depends on C → load order: [C, B, A]
        let mut graph = DependencyGraph::new();
        graph
            .add_plugin(make_manifest("C", "1.0.0", vec![], vec![]))
            .unwrap();
        graph
            .add_plugin(make_manifest("B", "1.0.0", vec![("C", "1.0.0")], vec![]))
            .unwrap();
        graph
            .add_plugin(make_manifest("A", "1.0.0", vec![("B", "1.0.0")], vec![]))
            .unwrap();

        let order = graph.resolve_load_order().unwrap();
        let pos_a = order.iter().position(|x| x == "A").unwrap();
        let pos_b = order.iter().position(|x| x == "B").unwrap();
        let pos_c = order.iter().position(|x| x == "C").unwrap();
        assert!(pos_c < pos_b, "C must load before B");
        assert!(pos_b < pos_a, "B must load before A");
    }

    #[test]
    fn test_topological_sort_diamond() {
        // A depends on [B, C], B and C both depend on D → D first, A last
        let mut graph = DependencyGraph::new();
        graph
            .add_plugin(make_manifest("D", "1.0.0", vec![], vec![]))
            .unwrap();
        graph
            .add_plugin(make_manifest("B", "1.0.0", vec![("D", "1.0.0")], vec![]))
            .unwrap();
        graph
            .add_plugin(make_manifest("C", "1.0.0", vec![("D", "1.0.0")], vec![]))
            .unwrap();
        graph
            .add_plugin(make_manifest(
                "A",
                "1.0.0",
                vec![("B", "1.0.0"), ("C", "1.0.0")],
                vec![],
            ))
            .unwrap();

        let order = graph.resolve_load_order().unwrap();
        let pos_d = order.iter().position(|x| x == "D").unwrap();
        let pos_a = order.iter().position(|x| x == "A").unwrap();
        assert_eq!(order[0], "D", "D must load first");
        assert_eq!(order.last().unwrap(), "A", "A must load last");
        assert!(pos_d < pos_a);
    }

    #[test]
    fn test_circular_dependency_detected() {
        let mut graph = DependencyGraph::new();
        graph
            .add_plugin(make_manifest("A", "1.0.0", vec![("B", "1.0.0")], vec![]))
            .unwrap();
        graph
            .add_plugin(make_manifest("B", "1.0.0", vec![("C", "1.0.0")], vec![]))
            .unwrap();
        graph
            .add_plugin(make_manifest("C", "1.0.0", vec![("A", "1.0.0")], vec![]))
            .unwrap();

        let result = graph.resolve_load_order();
        assert!(matches!(result, Err(DependencyError::CircularDependency)));
    }

    #[test]
    fn test_missing_dependency_detected() {
        let mut graph = DependencyGraph::new();
        graph
            .add_plugin(make_manifest("A", "1.0.0", vec![("B", "1.0.0")], vec![]))
            .unwrap();
        // B is not registered

        let result = graph.validate();
        assert!(matches!(
            result,
            Err(DependencyError::MissingDependency { .. })
        ));
    }

    #[test]
    fn test_version_constraint_caret() {
        // ^1.0.0 matches 1.2.3 but not 2.0.0
        let c = VersionConstraint::caret("1.0.0");
        assert!(c.satisfies("1.0.0"), "1.0.0 should satisfy ^1.0.0");
        assert!(c.satisfies("1.2.3"), "1.2.3 should satisfy ^1.0.0");
        assert!(c.satisfies("1.9.9"), "1.9.9 should satisfy ^1.0.0");
        assert!(!c.satisfies("2.0.0"), "2.0.0 should NOT satisfy ^1.0.0");
        assert!(!c.satisfies("0.9.9"), "0.9.9 should NOT satisfy ^1.0.0");
    }

    #[test]
    fn test_version_constraint_range() {
        // >=1.0.0 <2.0.0 matches 1.9.9 but not 2.0.0
        let c = VersionConstraint::range(">=1.0.0", "<2.0.0");
        assert!(c.satisfies("1.0.0"), "1.0.0 should satisfy range");
        assert!(c.satisfies("1.9.9"), "1.9.9 should satisfy range");
        assert!(!c.satisfies("2.0.0"), "2.0.0 should NOT satisfy range");
        assert!(!c.satisfies("0.9.9"), "0.9.9 should NOT satisfy range");
    }

    #[test]
    fn test_version_constraint_exact() {
        // =1.2.3 matches only 1.2.3
        let c = VersionConstraint::exact("1.2.3");
        assert!(c.satisfies("1.2.3"), "1.2.3 should satisfy =1.2.3");
        assert!(!c.satisfies("1.2.4"), "1.2.4 should NOT satisfy =1.2.3");
        assert!(!c.satisfies("1.2.2"), "1.2.2 should NOT satisfy =1.2.3");
    }

    #[test]
    fn test_conflict_detection() {
        let mut graph = DependencyGraph::new();
        // Plugin A conflicts with Plugin B
        graph
            .add_plugin(make_manifest("A", "1.0.0", vec![], vec!["B"]))
            .unwrap();
        graph
            .add_plugin(make_manifest("B", "1.0.0", vec![], vec![]))
            .unwrap();

        let result = graph.resolve_load_order();
        assert!(matches!(result, Err(DependencyError::Conflict { .. })));
    }

    #[test]
    fn test_optional_dependency_not_required() {
        let mut graph = DependencyGraph::new();
        // A has an optional dep on B (which is absent), but no required deps
        let manifest = PluginManifest {
            plugin_id: "A".to_string(),
            name: "A".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec![],
            optional_dependencies: vec![PluginDependency {
                plugin_id: "B".to_string(),
                version_constraint: VersionConstraint::exact("1.0.0"),
            }],
            conflicts: vec![],
        };
        graph.add_plugin(manifest).unwrap();

        // Should resolve without error — B is optional
        let order = graph.resolve_load_order().unwrap();
        assert_eq!(order, vec!["A"]);
    }

    #[test]
    fn test_plugin_registry_lazy_load() {
        let storage = InMemoryPluginStorage::new();
        let mut registry = PluginRegistry::new(storage);

        registry
            .register(make_manifest("A", "1.0.0", vec![], vec![]))
            .unwrap();

        assert!(!registry.is_loaded("A"));
        registry.load_plugin("A").unwrap();
        assert!(registry.is_loaded("A"));

        // Load again — should not error and A stays in loaded set
        registry.load_plugin("A").unwrap();
        assert!(registry.is_loaded("A"));
        // Verify loaded set has exactly one entry for A (not duplicated)
        assert_eq!(
            registry.loaded.iter().filter(|x| x.as_str() == "A").count(),
            1
        );
    }
}
