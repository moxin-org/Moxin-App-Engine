use mofa_plugin_loader::{hot_reload_enabled, load_all_plugins, PluginRegistry};
use tempfile::TempDir;
use std::fs;

#[test]
fn hot_reload_disabled_by_default() {
    std::env::remove_var("MOFA_HOT_RELOAD");
    assert!(!hot_reload_enabled());
}

#[test]
fn hot_reload_enabled_when_var_is_one() {
    std::env::set_var("MOFA_HOT_RELOAD", "1");
    assert!(hot_reload_enabled());
    std::env::remove_var("MOFA_HOT_RELOAD");
}

#[test]
fn load_all_plugins_empty_dir_returns_zero() {
    let dir = TempDir::new().unwrap();
    let registry = PluginRegistry::new();
    let loaded = load_all_plugins(dir.path(), &registry);
    assert_eq!(loaded, 0);
}

#[test]
fn load_all_plugins_skips_dirs_without_manifest() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("no_manifest")).unwrap();
    let registry = PluginRegistry::new();
    let loaded = load_all_plugins(dir.path(), &registry);
    assert_eq!(loaded, 0);
}

#[test]
fn load_plugin_handle_missing_lib_returns_error() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("plugin.toml"),
        r#"
[plugin]
id      = "test-plugin"
version = "0.1.0"
entry   = "libdoes_not_exist.so"
"#,
    )
    .unwrap();

    let result = mofa_plugin_loader::PluginHandle::load(dir.path());
    assert!(result.is_err(), "expected an error when .so is missing");
}
