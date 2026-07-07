//! Integration tests for hot-reload manager.
//!
//! Focused on deterministic API-level behavior and fault paths.

use std::time::Duration;

use mofa_plugins::hot_reload::{
    HotReloadConfig, HotReloadManager, PluginInfo, PluginVersion, ReloadError, ReloadEvent,
    ReloadStrategy,
};
use tempfile::tempdir;
use tokio::time::timeout;

#[test]
fn hot_reload_config_builder_applies_fields() {
    let cfg = HotReloadConfig::new()
        .with_strategy(ReloadStrategy::Manual)
        .with_preserve_state(true)
        .with_auto_rollback(true)
        .with_max_attempts(7)
        .with_reload_cooldown(Duration::from_secs(3))
        .with_shutdown_timeout(Duration::from_secs(9))
        .with_parallel_reload(true);

    assert!(matches!(cfg.base.strategy, ReloadStrategy::Manual));
    assert!(cfg.base.preserve_state);
    assert!(cfg.base.auto_rollback);
    assert_eq!(cfg.base.max_reload_attempts, 7);
    assert_eq!(cfg.base.reload_cooldown, Duration::from_secs(3));
    assert_eq!(cfg.shutdown_timeout, Duration::from_secs(9));
    assert!(cfg.parallel_reload);
}

#[tokio::test]
async fn manager_defaults_to_not_running_and_no_plugins() {
    let manager = HotReloadManager::default();

    assert!(!manager.is_running().await);
    assert!(manager.list_plugins().await.is_empty());
}

#[tokio::test]
async fn unload_missing_plugin_returns_not_found() {
    let manager = HotReloadManager::default();

    let err = manager
        .unload_plugin("missing-plugin")
        .await
        .expect_err("missing plugin should fail unload");

    assert!(matches!(err, ReloadError::PluginNotFound(id) if id == "missing-plugin"));
}

#[tokio::test]
async fn reload_missing_plugin_returns_not_found() {
    let manager = HotReloadManager::default();

    let err = manager
        .reload_plugin("missing-plugin")
        .await
        .expect_err("missing plugin should fail reload");

    assert!(matches!(err, ReloadError::PluginNotFound(id) if id == "missing-plugin"));
}

#[tokio::test]
async fn reload_registered_missing_library_propagates_load_error_and_emits_event() {
    let manager = HotReloadManager::default();
    let mut event_rx = manager.subscribe();

    let tmp = tempdir().expect("create tempdir");
    let missing_lib = tmp.path().join("missing_test_plugin.dll");

    let info = PluginInfo::new(
        "missing-lib",
        "Missing Library Plugin",
        PluginVersion::new(1, 0, 0),
    )
    .with_library_path(&missing_lib);
    manager
        .registry()
        .register(info)
        .await
        .expect("register plugin info");

    let err = manager
        .reload_plugin("missing-lib")
        .await
        .expect_err("reload should fail when library file does not exist");

    assert!(matches!(err, ReloadError::LoadError(_)));

    let mut saw_failed = false;
    for _ in 0..6 {
        let recv = timeout(Duration::from_millis(300), event_rx.recv()).await;
        if let Ok(Ok(ReloadEvent::ReloadFailed {
            plugin_id, path, ..
        })) = recv
            && plugin_id == "missing-lib"
            && path == missing_lib
        {
            saw_failed = true;
            break;
        }
    }

    assert!(
        saw_failed,
        "expected ReloadFailed event for missing registered library"
    );
}

#[tokio::test]
async fn add_watch_path_nonexistent_is_ok() {
    let manager = HotReloadManager::default();
    let tmp = tempdir().expect("create tempdir");
    let nonexistent = tmp.path().join("does_not_exist");

    manager
        .add_watch_path(&nonexistent)
        .await
        .expect("nonexistent watch path should be accepted");
}
