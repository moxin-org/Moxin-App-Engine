//! `mofa plugin install` command implementation

use crate::CliError;
use crate::context::{CliContext, PluginSpecEntry, instantiate_plugin_from_spec};
use crate::plugin_catalog::{DEFAULT_PLUGIN_REPO_ID, find_catalog_entry};
use colored::Colorize;
use futures::StreamExt;
use mofa_kernel::agent::plugins::PluginRegistry;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Execute the `mofa plugin install` command
pub async fn run(
    ctx: &CliContext,
    name: &str,
    checksum: Option<&str>,
    verify_signature: bool,
) -> Result<(), CliError> {
    let normalized = name.trim();
    if normalized.is_empty() {
        return Err(CliError::PluginError("Plugin name cannot be empty".into()));
    }

    if verify_signature {
        return Err(CliError::PluginError(
            "Signature verification requested with --verify-signature, but this feature is not implemented yet. Remove the flag or use --checksum for integrity verification.".into(),
        ));
    }

    println!("{} Installing plugin: {}", "→".green(), normalized.cyan());

    let plugin_source = determine_plugin_source(normalized)?;

    if let PluginSource::Registry(_) = plugin_source {
        let (repo_id, plugin_id) = parse_plugin_reference(normalized)?;
        let entry = find_catalog_entry(&repo_id, &plugin_id).ok_or_else(|| {
            CliError::PluginError(format!(
                "Plugin '{}' not found in repository '{}'",
                plugin_id, repo_id
            ))
        })?;

        if ctx.plugin_registry.contains(&entry.id) {
            return Err(CliError::PluginError(format!(
                "Plugin '{}' is already installed",
                entry.id
            )));
        }

        if let Ok(Some(existing)) = ctx.plugin_store.get(&entry.id)
            && existing.enabled
        {
            return Err(CliError::PluginError(format!(
                "Plugin '{}' is already persisted as enabled. Use `mofa plugin uninstall` first if you want to reinstall.",
                entry.id
            )));
        }

        let spec = PluginSpecEntry {
            id: entry.id.clone(),
            kind: entry.kind.clone(),
            enabled: true,
            config: entry.config.clone(),
            description: Some(entry.description.clone()),
            repo_id: Some(entry.repo_id.clone()),
        };

        let plugin = instantiate_plugin_from_spec(&spec).ok_or_else(|| {
            CliError::PluginError(format!(
                "CLI installer does not support plugin kind '{}'",
                spec.kind
            ))
        })?;

        ctx.plugin_registry.register(plugin).map_err(|e| {
            CliError::PluginError(format!("Failed to register plugin '{}': {}", entry.id, e))
        })?;

        if let Err(e) = ctx.plugin_store.save(&spec.id, &spec) {
            let _ = ctx.plugin_registry.unregister(&spec.id);
            return Err(CliError::PluginError(format!(
                "Failed to persist plugin '{}': {}. Rolled back in-memory registration.",
                spec.id, e
            )));
        }

        println!(
            "{} Installed plugin '{}' from repository '{}'",
            "✓".green(),
            spec.id,
            repo_id
        );
        return Ok(());
    }

    // Determine plugin source type natively (Local / Url)
    use std::path::Path as StdPath;
    let mut plugin_id = normalized.to_string();
    if let PluginSource::LocalPath(path) = &plugin_source
        && StdPath::new(normalized) == path
    {
        plugin_id = normalized.replace(['/', '\\'], "_");
    }

    if ctx.plugin_store.get(&plugin_id)?.is_some() {
        return Err(CliError::PluginError(format!(
            "Plugin '{}' is already installed",
            plugin_id
        )));
    }

    let plugin_dir = match plugin_source {
        PluginSource::LocalPath(path) => {
            println!("  {} Source: Local path", "•".bright_black());
            install_from_local_path(&ctx.data_dir, &plugin_id, &path).await?
        }
        PluginSource::Url(url) => {
            println!("  {} Source: URL", "•".bright_black());
            install_from_url(&ctx.data_dir, &plugin_id, &url, checksum).await?
        }
        PluginSource::Registry(_) => unreachable!(),
    };

    validate_plugin_structure(&plugin_dir)?;

    let spec = PluginSpecEntry {
        id: plugin_id.clone(),
        kind: "external".to_string(),
        enabled: true,
        config: serde_json::json!({
            "path": plugin_dir.to_string_lossy(),
            "installed_at": chrono::Utc::now().to_rfc3339(),
        }),
        description: None,
        repo_id: None,
    };

    ctx.plugin_store.save(&plugin_id, &spec).map_err(|e| {
        CliError::PluginError(format!(
            "Failed to persist plugin spec for '{}': {}",
            plugin_id, e
        ))
    })?;

    println!(
        "{} Plugin '{}' installed successfully",
        "✓".green(),
        plugin_id
    );
    println!(
        "  {} Location: {}",
        "•".bright_black(),
        plugin_dir.display().to_string().cyan()
    );
    println!(
        "  {} Use {} to activate it",
        "•".bright_black(),
        "mofa plugin enable".yellow()
    );

    Ok(())
}

fn parse_plugin_reference(value: &str) -> Result<(String, String), CliError> {
    if let Some((repo, plugin)) = value.split_once('/') {
        let repo = repo.trim();
        let plugin = plugin.trim();

        if repo.is_empty() || plugin.is_empty() {
            return Err(CliError::PluginError(
                "Plugin reference must be '<repo>/<plugin>'".into(),
            ));
        }

        Ok((repo.to_string(), plugin.to_string()))
    } else {
        Ok((DEFAULT_PLUGIN_REPO_ID.to_string(), value.to_string()))
    }
}

/// Determine the source type of a plugin
fn determine_plugin_source(name: &str) -> Result<PluginSource, CliError> {
    // Check if it's a URL
    if name.starts_with("http://") || name.starts_with("https://") {
        return Ok(PluginSource::Url(name.to_string()));
    }

    // Check if it's a local path
    let path = Path::new(name);
    if path.exists() {
        return Ok(PluginSource::LocalPath(path.to_path_buf()));
    }

    // Otherwise treat as registry name
    Ok(PluginSource::Registry(name.to_string()))
}

/// Install plugin from a local path
async fn install_from_local_path(
    data_dir: &Path,
    plugin_name: &str,
    source_path: &Path,
) -> Result<PathBuf, CliError> {
    let plugins_dir = data_dir.join("plugins");
    tokio::fs::create_dir_all(&plugins_dir)
        .await
        .map_err(|e| CliError::PluginError(format!("Failed to create plugins directory: {}", e)))?;

    let dest_dir = plugins_dir.join(plugin_name);

    // Remove existing directory if present
    if dest_dir.exists() {
        tokio::fs::remove_dir_all(&dest_dir).await.map_err(|e| {
            CliError::PluginError(format!(
                "Failed to remove existing plugin directory '{}': {}",
                dest_dir.display(),
                e
            ))
        })?;
    }

    // Copy plugin files
    copy_dir_recursive(source_path, &dest_dir).await?;

    Ok(dest_dir)
}

/// Install plugin from a URL
async fn install_from_url(
    data_dir: &Path,
    plugin_name: &str,
    url: &str,
    expected_checksum: Option<&str>,
) -> Result<PathBuf, CliError> {
    let plugins_dir = data_dir.join("plugins");
    tokio::fs::create_dir_all(&plugins_dir)
        .await
        .map_err(|e| CliError::PluginError(format!("Failed to create plugins directory: {}", e)))?;

    // Download the file with progress bar
    println!("  {} Downloading from {}", "•".bright_black(), url.cyan());

    let response = reqwest::get(url).await.map_err(|e| {
        CliError::PluginError(format!("Failed to download plugin from {}: {}", url, e))
    })?;

    if !response.status().is_success() {
        return Err(CliError::PluginError(format!(
            "Download failed with status: {}",
            response.status()
        )));
    }

    // Get content length for progress bar
    let total_size = response.content_length();
    let pb = indicatif::ProgressBar::new(total_size.unwrap_or(0));
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Download with progress tracking
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| CliError::PluginError(format!("Failed to read download chunk: {}", e)))?;
        bytes.extend_from_slice(&chunk);
        pb.inc(chunk.len() as u64);
    }
    pb.finish_with_message("Downloaded");

    // Verify checksum if provided
    if let Some(expected) = expected_checksum {
        println!("  {} Verifying checksum...", "•".bright_black());
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let computed = hasher.finalize();
        let computed_hex = hex::encode(computed);

        if computed_hex.to_lowercase() != expected.to_lowercase() {
            return Err(CliError::PluginError(format!(
                "Checksum mismatch!\n  Expected: {}\n  Computed: {}\n\nPlugin may be corrupted or tampered with.",
                expected, computed_hex
            )));
        }
        println!("  {} Checksum verified", "✓".green());
    }

    // Determine if it's an archive or single file
    let dest_dir = plugins_dir.join(plugin_name);
    tokio::fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| CliError::PluginError(format!("Failed to create plugin directory: {}", e)))?;

    // For simplicity, assume it's a tar.gz or zip based on URL
    if url.ends_with(".tar.gz") || url.ends_with(".tgz") {
        extract_tar_gz(&bytes, &dest_dir)?;
    } else if url.ends_with(".zip") {
        extract_zip(&bytes, &dest_dir)?;
    } else {
        // Treat as single file, save it directly
        let filename = url.split('/').next_back().unwrap_or("plugin");
        let file_path = dest_dir.join(filename);
        tokio::fs::write(&file_path, &bytes)
            .await
            .map_err(|e| CliError::PluginError(format!("Failed to write plugin file: {}", e)))?;
    }

    Ok(dest_dir)
}

/// Validate that the plugin directory has required structure
fn validate_plugin_structure(plugin_dir: &Path) -> Result<(), CliError> {
    if !plugin_dir.exists() {
        return Err(CliError::PluginError(format!(
            "Plugin directory does not exist: {}",
            plugin_dir.display()
        )));
    }

    if !plugin_dir.is_dir() {
        return Err(CliError::PluginError(format!(
            "Plugin path is not a directory: {}",
            plugin_dir.display()
        )));
    }

    // Check for at least one file (skip . and .. entries)
    let mut has_files = false;
    let mut entry_count = 0;
    let entries = std::fs::read_dir(plugin_dir).map_err(|e| {
        CliError::PluginError(format!(
            "Failed to read plugin directory '{}': {}",
            plugin_dir.display(),
            e
        ))
    })?;
    for entry in entries {
        entry_count += 1;
        match entry {
            Ok(entry) => {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // Skip hidden files and directories starting with .
                if !name_str.starts_with('.') {
                    has_files = true;
                    break;
                }
            }
            Err(_) => {
                // Continue on error - some entries might fail
                continue;
            }
        }
    }

    if !has_files {
        return Err(CliError::PluginError(format!(
            "Plugin directory is empty or contains only hidden files (found {} entries): {}",
            entry_count,
            plugin_dir.display()
        )));
    }

    // Plugin validation passed
    Ok(())
}

/// Recursively copy a directory
fn copy_dir_recursive<'a>(
    src: &'a Path,
    dest: &'a Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), CliError>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(dest).await.map_err(|e| {
            CliError::PluginError(format!(
                "Failed to create directory '{}': {}",
                dest.display(),
                e
            ))
        })?;

        let mut entries = tokio::fs::read_dir(src).await.map_err(|e| {
            CliError::PluginError(format!(
                "Failed to read source directory '{}': {}",
                src.display(),
                e
            ))
        })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| CliError::PluginError(format!("Failed to read directory entry: {}", e)))?
        {
            let entry_path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if entry_path.is_dir() {
                copy_dir_recursive(&entry_path, &dest_path).await?;
            } else {
                tokio::fs::copy(&entry_path, &dest_path)
                    .await
                    .map_err(|e| {
                        CliError::PluginError(format!(
                            "Failed to copy file from {} to {}: {}",
                            entry_path.display(),
                            dest_path.display(),
                            e
                        ))
                    })?;
            }
        }

        Ok(())
    })
}

/// Extract a tar.gz archive
fn extract_tar_gz(bytes: &[u8], dest_dir: &Path) -> Result<(), CliError> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(bytes);
    let mut archive = Archive::new(gz);

    let entries = archive
        .entries()
        .map_err(|e| CliError::PluginError(format!("Failed to read tar.gz entries: {}", e)))?;

    for entry in entries {
        let mut entry = entry.map_err(|e| {
            CliError::PluginError(format!("Failed to read tar.gz entry metadata: {}", e))
        })?;
        let entry_path = entry
            .path()
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());

        let unpacked = entry.unpack_in(dest_dir).map_err(|e| {
            CliError::PluginError(format!(
                "Failed to extract tar.gz entry '{}': {}",
                entry_path, e
            ))
        })?;

        if !unpacked {
            return Err(CliError::PluginError(format!(
                "Unsafe archive entry escapes destination: {}",
                entry_path
            )));
        }
    }

    Ok(())
}

/// Extract a zip archive
fn extract_zip(bytes: &[u8], dest_dir: &Path) -> Result<(), CliError> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| CliError::PluginError(format!("Failed to read zip archive: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CliError::PluginError(format!("Failed to read zip entry {}: {}", i, e)))?;

        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath).map_err(|e| {
                CliError::PluginError(format!(
                    "Failed to create directory '{}': {}",
                    outpath.display(),
                    e
                ))
            })?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CliError::PluginError(format!(
                        "Failed to create parent directory '{}': {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(|e| {
                CliError::PluginError(format!(
                    "Failed to create file '{}': {}",
                    outpath.display(),
                    e
                ))
            })?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| {
                CliError::PluginError(format!("Failed to write file contents: {}", e))
            })?;
        }
    }

    Ok(())
}

/// Plugin source types
enum PluginSource {
    LocalPath(PathBuf),
    Url(String),
    Registry(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::CliContext;
    use crate::plugin_catalog::DEFAULT_PLUGIN_REPO_ID;
    use mofa_kernel::agent::plugins::PluginRegistry as PluginRegistryTrait;
    use tempfile::TempDir;

    fn disable_default_http_plugin(ctx: &CliContext) {
        let _ = ctx.plugin_registry.unregister("http-plugin");
        if let Ok(Some(mut spec)) = ctx.plugin_store.get("http-plugin") {
            spec.enabled = false;
            ctx.plugin_store.save("http-plugin", &spec).unwrap();
        }
    }

    #[tokio::test]
    async fn test_install_registers_and_persists_builtin_plugin() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        disable_default_http_plugin(&ctx);

        run(&ctx, "http-plugin", None, false).await.unwrap();

        assert!(PluginRegistryTrait::contains(
            ctx.plugin_registry.as_ref(),
            "http-plugin"
        ));

        let spec = ctx.plugin_store.get("http-plugin").unwrap().unwrap();
        assert!(spec.enabled);
        assert_eq!(spec.repo_id.as_deref(), Some(DEFAULT_PLUGIN_REPO_ID));
    }

    #[test]
    fn test_determine_plugin_source_url() {
        let source = determine_plugin_source("https://example.com/plugin.tar.gz").unwrap();
        assert!(matches!(source, PluginSource::Url(_)));
    }

    #[test]
    fn test_determine_plugin_source_local() {
        let temp_dir = TempDir::new().unwrap();
        let source = determine_plugin_source(temp_dir.path().to_str().unwrap()).unwrap();
        assert!(matches!(source, PluginSource::LocalPath(_)));
    }

    #[test]
    fn test_determine_plugin_source_registry() {
        let source = determine_plugin_source("my-plugin").unwrap();
        assert!(matches!(source, PluginSource::Registry(_)));
    }

    #[tokio::test]
    async fn test_validate_plugin_structure() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("test-plugin");
        tokio::fs::create_dir(&plugin_dir).await.unwrap();

        // Empty directory should fail
        assert!(validate_plugin_structure(&plugin_dir).is_err());

        // Directory with a file should pass
        tokio::fs::write(plugin_dir.join("plugin.rs"), b"// test")
            .await
            .unwrap();
        assert!(validate_plugin_structure(&plugin_dir).is_ok());
    }

    #[tokio::test]
    async fn test_install_from_local_path() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        let err = run(&ctx, "http-plugin", None, false).await.unwrap_err();
        assert!(err.to_string().contains("already installed"));
    }

    #[tokio::test]
    async fn test_install_rejects_empty_name() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        let err = run(&ctx, "   ", None, false).await.unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_install_rejects_verify_signature_until_implemented() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        let err = run(&ctx, "http-plugin", None, true).await.unwrap_err();
        assert!(err.to_string().contains("not implemented yet"));
        assert!(err.to_string().contains("--verify-signature"));
    }

    #[tokio::test]
    async fn test_install_supports_repo_prefixed_reference() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        disable_default_http_plugin(&ctx);

        run(&ctx, "official/http-plugin", None, false)
            .await
            .unwrap();

        let spec = ctx.plugin_store.get("http-plugin").unwrap().unwrap();
        assert_eq!(spec.repo_id.as_deref(), Some(DEFAULT_PLUGIN_REPO_ID));
        assert!(spec.enabled);
    }

    #[tokio::test]
    async fn test_install_rejects_unknown_catalog_entry() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        let err = run(&ctx, "official/not-real", None, false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found in repository"));
    }

    #[tokio::test]
    async fn test_install_from_local_path2() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();
        // Create a source plugin directory
        let source_plugin = temp.path().join("source-plugin");
        tokio::fs::create_dir_all(&source_plugin).await.unwrap();
        tokio::fs::write(
            source_plugin.join("plugin.toml"),
            b"[plugin]\nname = \"test\"\n",
        )
        .await
        .unwrap();
        tokio::fs::write(source_plugin.join("main.rs"), b"fn main() {}\n")
            .await
            .unwrap();

        // Ensure files are synced to disk
        tokio::fs::metadata(&source_plugin).await.unwrap();

        // Install the plugin using the path as the name parameter
        let plugin_path_str = source_plugin.to_str().unwrap();
        let result = run(&ctx, plugin_path_str, None, false).await;
        if let Err(e) = &result {
            eprintln!("Installation failed: {}", e);
        }
        assert!(result.is_ok(), "Plugin installation should succeed");

        // Verify plugin was installed (plugin name is the full path, sanitized)
        let plugin_name = plugin_path_str.replace('/', "_").replace('\\', "_");
        let plugin_dir = ctx.data_dir.join("plugins").join(&plugin_name);
        assert!(
            plugin_dir.exists(),
            "Plugin dir should exist at: {}",
            plugin_dir.display()
        );
        assert!(plugin_dir.join("plugin.toml").exists());
        assert!(plugin_dir.join("main.rs").exists());

        // Verify plugin spec was saved
        let spec = ctx.plugin_store.get(&plugin_name).unwrap();
        assert!(spec.is_some());
        assert!(spec.unwrap().enabled);
    }

    #[tokio::test]
    async fn test_install_already_exists() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        // Create a plugin directory and install it first
        let source_plugin = temp.path().join("test-plugin");
        tokio::fs::create_dir_all(&source_plugin).await.unwrap();
        tokio::fs::write(source_plugin.join("file.txt"), b"test")
            .await
            .unwrap();

        // First installation should succeed
        let result1 = run(&ctx, source_plugin.to_str().unwrap(), None, false).await;
        assert!(result1.is_ok());

        // Try to install again - should fail because plugin already exists
        let result2 = run(&ctx, source_plugin.to_str().unwrap(), None, false).await;
        assert!(result2.is_err());
        assert!(
            result2
                .unwrap_err()
                .to_string()
                .contains("already installed")
        );
    }

    #[tokio::test]
    async fn test_install_invalid_plugin_structure() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        // Create an empty source directory (invalid)
        let source_plugin = temp.path().join("empty-plugin");
        tokio::fs::create_dir_all(&source_plugin).await.unwrap();

        // Try to install - should fail validation
        let result = run(&ctx, source_plugin.to_str().unwrap(), None, false).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("empty") || err_msg.contains("Plugin directory"));
    }

    #[test]
    fn test_extract_tar_gz_rejects_path_traversal() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        use tar::{Builder, Header};

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path().join("plugin");
        std::fs::create_dir_all(&dest_dir).unwrap();

        let mut tar_builder = Builder::new(Vec::new());
        let payload = b"malicious";
        let mut header = Header::new_gnu();
        {
            let old = header.as_old_mut();
            old.name.fill(0);
            let path = b"../escape.txt";
            old.name[..path.len()].copy_from_slice(path);
        }
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar_builder.append(&header, &payload[..]).unwrap();
        let tar_bytes = tar_builder.into_inner().unwrap();

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        let tar_gz_bytes = encoder.finish().unwrap();

        let err = extract_tar_gz(&tar_gz_bytes, &dest_dir).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("Unsafe archive entry escapes destination")
                || message.contains("must not have `..`")
        );
        assert!(!temp_dir.path().join("escape.txt").exists());
    }
}
