//! Example: Workflow Versioning Lifecycle
//!
//! Shows a complete lifecycle:
//!   create v1.0.0 as draft → publish → create v2.0.0 → roll back to v1.0.0 → print history

use mofa_foundation::workflow::VersionStore;
use mofa_foundation::workflow::versioning::{InMemoryVersionStore, VersionManager};

fn main() {
    let store = InMemoryVersionStore::new();
    let manager = VersionManager::new(store);

    let workflow_id = "my-workflow".to_string();
    let definition_v1 = r#"{"name":"greeting","steps":["say_hello"]}"#;
    let definition_v2 = r#"{"name":"greeting","steps":["say_hello","say_goodbye"]}"#;

    // Create v1.0.0 as draft, then publish
    println!("Creating v1.0.0 as draft...");
    let draft = manager
        .create_draft(
            workflow_id.clone(),
            definition_v1,
            "1.0.0".to_string(),
            "Initial workflow".to_string(),
        )
        .unwrap();
    println!(
        "  Draft created: {} (status: {:?})",
        draft.version, draft.status
    );

    println!("Publishing v1.0.0...");
    let published = manager.publish_version(&workflow_id, "1.0.0").unwrap();
    println!(
        "  Published: {} (status: {:?})",
        published.version, published.status
    );

    // Create v2.0.0 directly (published)
    println!("Creating v2.0.0...");
    let v2 = manager
        .create_version(
            workflow_id.clone(),
            definition_v2,
            "2.0.0".to_string(),
            "Added farewell step".to_string(),
        )
        .unwrap();
    println!("  Created: {} (status: {:?})", v2.version, v2.status);

    // Diff v1 vs v2 — not yet implemented (only hash is stored, not content)
    match manager.diff(&workflow_id, "1.0.0", "2.0.0") {
        Ok(diff) => println!(
            "Diff {from} -> {to}: {n} structural changes",
            from = diff.from_version,
            to = diff.to_version,
            n = diff.changes.len(),
        ),
        Err(e) => println!("Diff not available: {e}"),
    };

    // Roll back to v1.0.0
    println!("Rolling back to v1.0.0...");
    let rolled_back = manager.rollback(&workflow_id, "1.0.0").unwrap();
    println!("  Current version: {}", rolled_back.version);

    // Print full version history
    println!("\nVersion history for '{}':", workflow_id);
    let versions = manager.store().list_versions(&workflow_id).unwrap();
    let history = manager.store().load_history(&workflow_id).unwrap().unwrap();
    for v_str in &versions {
        let v = history.get_version(v_str).unwrap();
        let current_marker = if history.current_version.as_deref() == Some(v_str) {
            " <-- current"
        } else {
            ""
        };
        println!(
            "  {} | {:?} | {}{}",
            v.version, v.status, v.changelog, current_marker
        );
    }
}
