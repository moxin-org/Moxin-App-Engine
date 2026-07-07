//! Helpers for loading deterministic fixture files from `tests/fixtures`.

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

/// Return the absolute path to the fixture root directory.
pub fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Return the absolute path to a fixture relative to the fixture root.
pub fn fixture_path(relative_path: impl AsRef<Path>) -> PathBuf {
    fixtures_root().join(relative_path)
}

/// Load a fixture from `tests/fixtures`, parsing JSON, YAML, or YML by extension.
pub fn load_fixture<T>(relative_path: impl AsRef<Path>) -> Result<T>
where
    T: DeserializeOwned,
{
    let path = fixture_path(relative_path);
    load_fixture_from_path(path)
}

fn load_fixture_from_path<T>(path: PathBuf) -> Result<T>
where
    T: DeserializeOwned,
{
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read fixture '{}'", path.display()))?;
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "json" => serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse JSON fixture '{}'", path.display())),
        "yaml" | "yml" => serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse YAML fixture '{}'", path.display())),
        other => bail!(
            "unsupported fixture extension '{}' for '{}'",
            other,
            path.display()
        ),
    }
}
