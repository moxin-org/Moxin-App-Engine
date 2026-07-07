//! `mofa plugin repository` subcommands

use crate::CliError;
use crate::context::CliContext;
use crate::output::Table;
use crate::plugin_catalog::PluginRepoEntry;
use chrono::Utc;
use colored::Colorize;

/// List configured plugin repositories
pub async fn list(ctx: &CliContext) -> Result<(), CliError> {
    println!("{} Registered plugin repositories", "→".green());
    println!();

    let repos = ctx.plugin_repo_store.list()?;
    if repos.is_empty() {
        println!("  No repositories configured.");
        return Ok(());
    }

    let mut table = Table::builder().headers(&["ID", "URL", "Description", "Last Synced"]);
    for (_, repo) in repos {
        let last_synced = repo
            .last_synced
            .map(|ts| ts.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());
        table = table.add_row(&[
            repo.id.as_str(),
            repo.url.as_str(),
            repo.description.as_deref().unwrap_or(""),
            last_synced.as_str(),
        ]);
    }

    println!("{}", table.build());
    Ok(())
}

/// Add a new plugin repository
pub async fn add(
    ctx: &CliContext,
    id: &str,
    url: &str,
    description: Option<&str>,
) -> Result<(), CliError> {
    if ctx.plugin_repo_store.get(id)?.is_some() {
        return Err(CliError::PluginError(format!(
            "Repository '{}' already exists",
            id
        )));
    }

    let repo = PluginRepoEntry {
        id: id.to_string(),
        url: url.to_string(),
        description: description.map(|d| d.to_string()),
        last_synced: Some(Utc::now()),
    };

    ctx.plugin_repo_store.save(id, &repo)?;
    println!(
        "{} Repository '{}' added ({})",
        "✓".green(),
        id,
        repo.url.cyan()
    );
    Ok(())
}

/// Remove a plugin repository
pub async fn remove(ctx: &CliContext, id: &str) -> Result<(), CliError> {
    let removed = ctx.plugin_repo_store.delete(id)?;

    if removed {
        println!("{} Repository '{}' removed", "✓".green(), id);
        Ok(())
    } else {
        Err(CliError::PluginError(format!(
            "Repository '{}' not found",
            id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::CliContext;
    use tempfile::TempDir;

    #[tokio::test]
    async fn list_runs_without_error() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        list(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn add_persists_repository() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        add(
            &ctx,
            "custom",
            "https://example.org/plugins",
            Some("Example repo"),
        )
        .await
        .unwrap();

        let stored = ctx.plugin_repo_store.get("custom").unwrap().unwrap();
        assert_eq!(stored.url, "https://example.org/plugins");
        assert!(stored.last_synced.is_some());
    }

    #[tokio::test]
    async fn remove_deletes_existing_repository() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        add(
            &ctx,
            "custom",
            "https://example.org/plugins",
            Some("Example repo"),
        )
        .await
        .unwrap();

        remove(&ctx, "custom").await.unwrap();
        assert!(ctx.plugin_repo_store.get("custom").unwrap().is_none());
    }

    #[tokio::test]
    async fn remove_returns_error_for_missing_repository() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        let err = remove(&ctx, "missing").await.unwrap_err();
        match err {
            CliError::PluginError(message) => {
                assert!(message.contains("not found"));
            }
            other => panic!("Unexpected error type: {}", other),
        }
    }
}
