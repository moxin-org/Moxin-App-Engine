//! ModelPool - Lifecycle Management for Local Models
//!
//! This module provides ModelPool for managing local model lifecycle including:
//! - On-demand model loading with async initialization
//! - LRU-based model cache with configurable size
//! - Idle timeout-based automatic unloading
//! - Graceful shutdown with state preservation
//!
//! # Example
//!
//! ```rust,no_run
//! use mofa_foundation::model_pool::{ModelPool, ModelPoolConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = ModelPoolConfig::default()
//!         .with_max_models(3)
//!         .with_idle_timeout_secs(300);
//!
//!     let pool = ModelPool::new(config);
//!     // ... use the pool
//! }
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Errors produced by ModelPool operations
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModelPoolError {
    /// A model backend reported an error during load/unload
    #[error("backend error: {0}")]
    Backend(String),
    /// The requested model was not found in the pool
    #[error("model not found: {0}")]
    NotFound(String),
}

/// Convenience alias used throughout this module
pub type Result<T> = std::result::Result<T, ModelPoolError>;

/// Configuration for ModelPool
#[derive(Debug, Clone)]
pub struct ModelPoolConfig {
    /// Maximum number of models to keep in memory
    pub max_models: usize,
    /// Idle timeout in seconds before unloading a model
    pub idle_timeout_secs: u64,
    /// Preload models on startup
    pub preload_models: Vec<String>,
    /// Check interval for idle models
    pub eviction_check_interval_secs: u64,
}

impl Default for ModelPoolConfig {
    fn default() -> Self {
        Self {
            max_models: 2,
            idle_timeout_secs: 600, // 10 minutes
            preload_models: vec![],
            eviction_check_interval_secs: 60, // Check every minute
        }
    }
}

impl ModelPoolConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_models(mut self, max: usize) -> Self {
        self.max_models = max;
        self
    }

    pub fn with_idle_timeout_secs(mut self, secs: u64) -> Self {
        self.idle_timeout_secs = secs;
        self
    }

    pub fn with_preload_models(mut self, models: Vec<String>) -> Self {
        self.preload_models = models;
        self
    }
}

/// Model state in the pool
#[derive(Debug, Clone, PartialEq)]
pub enum ModelState {
    /// Model is being loaded
    Loading,
    /// Model is ready to use
    Ready,
    /// Model is being unloaded
    Unloading,
    /// Model failed to load
    Error(String),
}

/// Information about a loaded model
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub model_id: String,
    pub state: ModelState,
    pub loaded_at: Instant,
    pub last_accessed: Instant,
    pub memory_usage_bytes: usize,
}

/// Trait for model backends that can be loaded into the pool
#[async_trait]
pub trait ModelBackend: Send + Sync {
    /// Unique identifier for this model
    fn model_id(&self) -> &str;

    /// Load the model into memory
    async fn load(&self) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Unload the model from memory
    async fn unload(&self) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Check if model is loaded
    fn is_loaded(&self) -> bool;

    /// Get estimated memory usage in bytes
    fn memory_usage(&self) -> usize;

    /// Generate text (placeholder for actual inference)
    async fn generate(&self, prompt: &str) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

/// Entry in the LRU cache
#[derive(Clone)]
struct CacheEntry {
    backend: Arc<dyn ModelBackend>,
    info: ModelInfo,
}

/// Shared inner state for ModelPool
struct ModelPoolInner {
    config: ModelPoolConfig,
    cache: RwLock<Vec<CacheEntry>>,
    is_running: AtomicBool,
    shutdown_flag: AtomicBool,
    background_handle: Mutex<Option<JoinHandle<()>>>,
}

/// ModelPool - LRU cache for model lifecycle management
///
/// This type is cheaply cloneable — all clones share the same underlying state.
#[derive(Clone)]
pub struct ModelPool {
    inner: Arc<ModelPoolInner>,
}

impl ModelPool {
    /// Create a new ModelPool with the given configuration
    pub fn new(config: ModelPoolConfig) -> Self {
        Self {
            inner: Arc::new(ModelPoolInner {
                config,
                cache: RwLock::new(Vec::new()),
                is_running: AtomicBool::new(false),
                shutdown_flag: AtomicBool::new(false),
                background_handle: Mutex::new(None),
            }),
        }
    }

    /// Start the ModelPool background tasks (idle eviction)
    pub fn start(&self) {
        if self.inner.is_running.load(Ordering::SeqCst) {
            warn!("ModelPool is already running");
            return;
        }

        self.inner.is_running.store(true, Ordering::SeqCst);
        self.inner.shutdown_flag.store(false, Ordering::SeqCst);

        info!(
            "ModelPool started with config: max_models={}, idle_timeout={}s",
            self.inner.config.max_models, self.inner.config.idle_timeout_secs
        );

        // Start background eviction task
        let pool = self.clone();
        Self::start_background_tasks(pool);
    }

    /// Stop the ModelPool and gracefully shutdown
    pub async fn shutdown(&self) {
        if !self.inner.is_running.load(Ordering::SeqCst) {
            return;
        }

        info!("ModelPool shutting down...");

        // Signal shutdown to background task
        self.inner.shutdown_flag.store(true, Ordering::SeqCst);

        // Wait for background task to finish
        if let Some(handle) = self.inner.background_handle.lock().await.take() {
            let _ = handle.await;
        }

        // Unload all models
        let model_ids: Vec<String> = self.list_models().await;
        for model_id in model_ids {
            if let Err(e) = self.unload_model(&model_id).await {
                error!("Error unloading model {}: {}", model_id, e);
            }
        }

        self.inner.is_running.store(false, Ordering::SeqCst);
        info!("ModelPool shutdown complete");
    }

    /// Load a model into the pool using the provided backend
    pub async fn load_model(
        &self,
        model_id: &str,
        backend: Arc<dyn ModelBackend>,
    ) -> Result<()> {
        // Check if already loaded
        {
            let cache = self.inner.cache.read().await;
            if cache
                .iter()
                .any(|e| e.backend.model_id() == model_id && e.info.state == ModelState::Ready)
            {
                drop(cache);
                self.touch_model(model_id).await?;
                return Ok(());
            }
        }

        // Check if we need to evict models
        self.maybe_evict().await?;

        // Load the model
        backend
            .load()
            .await
            .map_err(|e| ModelPoolError::Backend(e.to_string()))?;

        let info = ModelInfo {
            model_id: model_id.to_string(),
            state: ModelState::Ready,
            loaded_at: Instant::now(),
            last_accessed: Instant::now(),
            memory_usage_bytes: backend.memory_usage(),
        };

        // Add to cache
        let entry = CacheEntry { backend, info };
        self.inner.cache.write().await.push(entry);

        // Move to front (most recently used)
        self.move_to_front(model_id).await?;

        info!("Loaded model: {}", model_id);
        Ok(())
    }

    /// Unload a model from the pool
    pub async fn unload_model(&self, model_id: &str) -> Result<()> {
        let backend_to_unload = {
            let mut cache = self.inner.cache.write().await;
            cache
                .iter()
                .position(|e| e.backend.model_id() == model_id)
                .map(|pos| cache.remove(pos).backend)
        };

        if let Some(backend) = backend_to_unload {
            backend
                .unload()
                .await
                .map_err(|e| ModelPoolError::Backend(e.to_string()))?;
            info!("Unloaded model: {}", model_id);
        }

        Ok(())
    }

    /// Get a model from the pool if it is loaded
    pub async fn get_model(&self, model_id: &str) -> Option<Arc<dyn ModelBackend>> {
        let backend = {
            let cache = self.inner.cache.read().await;
            cache
                .iter()
                .find(|e| e.backend.model_id() == model_id && e.info.state == ModelState::Ready)
                .map(|e| e.backend.clone())
        };

        if backend.is_some() {
            let _ = self.touch_model(model_id).await;
        }

        backend
    }

    /// Check if a model is loaded
    pub async fn is_model_loaded(&self, model_id: &str) -> bool {
        let cache = self.inner.cache.read().await;
        cache
            .iter()
            .any(|e| e.backend.model_id() == model_id && e.info.state == ModelState::Ready)
    }

    /// List all loaded models
    pub async fn list_models(&self) -> Vec<String> {
        let cache = self.inner.cache.read().await;
        cache.iter().map(|e| e.info.model_id.clone()).collect()
    }

    /// Get model info
    pub async fn get_model_info(&self, model_id: &str) -> Option<ModelInfo> {
        let cache = self.inner.cache.read().await;
        cache
            .iter()
            .find(|e| e.backend.model_id() == model_id)
            .map(|e| e.info.clone())
    }

    /// Get pool statistics
    pub async fn stats(&self) -> PoolStats {
        let cache = self.inner.cache.read().await;
        let total_memory: usize = cache.iter().map(|e| e.info.memory_usage_bytes).sum();

        PoolStats {
            loaded_models: cache.len(),
            max_models: self.inner.config.max_models,
            total_memory_bytes: total_memory,
        }
    }

    // Internal methods

    async fn touch_model(&self, model_id: &str) -> Result<()> {
        self.move_to_front(model_id).await
    }

    async fn move_to_front(&self, model_id: &str) -> Result<()> {
        let mut cache = self.inner.cache.write().await;
        if let Some(pos) = cache.iter().position(|e| e.backend.model_id() == model_id) {
            let entry = cache.remove(pos);
            let mut info = entry.info;
            info.last_accessed = Instant::now();
            cache.insert(
                0,
                CacheEntry {
                    backend: entry.backend,
                    info,
                },
            );
        }
        Ok(())
    }

    async fn maybe_evict(&self) -> Result<()> {
        let cache = self.inner.cache.read().await;

        // Check if we're at capacity
        if cache.len() < self.inner.config.max_models {
            return Ok(());
        }

        // Find the least recently used model
        let lru_model_id = cache
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.info.last_accessed)
            .map(|(_, e)| e.info.model_id.clone());

        drop(cache);

        if let Some(model_id) = lru_model_id {
            info!("Evicting model due to capacity: {}", model_id);
            self.unload_model(&model_id).await?;
        }

        Ok(())
    }

    async fn evict_idle_models(&self) {
        let idle_timeout = Duration::from_secs(self.inner.config.idle_timeout_secs);
        let now = Instant::now();

        let models_to_evict: Vec<String> = {
            let cache = self.inner.cache.read().await;
            cache
                .iter()
                .filter(|e| e.info.state == ModelState::Ready)
                .filter(|e| now.duration_since(e.info.last_accessed) > idle_timeout)
                .map(|e| e.info.model_id.clone())
                .collect()
        };

        for model_id in models_to_evict {
            if let Err(e) = self.unload_model(&model_id).await {
                error!("Error evicting idle model {}: {}", model_id, e);
            }
        }
    }

    fn start_background_tasks(pool: ModelPool) {
        let pool_for_store = pool.clone();
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(
                pool.inner.config.eviction_check_interval_secs,
            ));

            loop {
                interval.tick().await;

                // Check shutdown flag
                if pool.inner.shutdown_flag.load(Ordering::SeqCst) {
                    debug!("ModelPool background tasks shutting down");
                    break;
                }

                pool.evict_idle_models().await;
            }
        });

        // Store the handle — we need a separate task since start() is sync
        tokio::spawn(async move {
            *pool_for_store.inner.background_handle.lock().await = Some(handle);
        });
    }
}

/// Statistics about the model pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Number of currently loaded models
    pub loaded_models: usize,
    /// Maximum number of models allowed
    pub max_models: usize,
    /// Total memory used by loaded models in bytes
    pub total_memory_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock model backend for testing
    struct MockModelBackend {
        model_id: String,
        loaded: AtomicBool,
    }

    impl MockModelBackend {
        fn new(model_id: String) -> Self {
            Self {
                model_id,
                loaded: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl ModelBackend for MockModelBackend {
        fn model_id(&self) -> &str {
            &self.model_id
        }

        async fn load(&self) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.loaded.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn unload(&self) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.loaded.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn is_loaded(&self) -> bool {
            self.loaded.load(Ordering::SeqCst)
        }

        fn memory_usage(&self) -> usize {
            // Mock: 1GB per model
            1024 * 1024 * 1024
        }

        async fn generate(&self, prompt: &str) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok(format!("Mock response for: {}", prompt))
        }
    }

    fn mock_backend(id: &str) -> Arc<dyn ModelBackend> {
        Arc::new(MockModelBackend::new(id.to_string()))
    }

    #[tokio::test]
    async fn test_model_pool_basic() {
        let config = ModelPoolConfig::default()
            .with_max_models(2)
            .with_idle_timeout_secs(60);

        let pool = ModelPool::new(config);

        // Test load model
        pool.load_model("test-model", mock_backend("test-model"))
            .await
            .unwrap();

        // Test is_model_loaded
        assert!(pool.is_model_loaded("test-model").await);

        // Test list_models
        let models = pool.list_models().await;
        assert_eq!(models, vec!["test-model"]);

        // Test unload
        pool.unload_model("test-model").await.unwrap();
        assert!(!pool.is_model_loaded("test-model").await);
    }

    #[tokio::test]
    async fn test_model_pool_lru_eviction() {
        let config = ModelPoolConfig::default().with_max_models(2);

        let pool = ModelPool::new(config);

        // Load 3 models — should evict first one
        pool.load_model("model-1", mock_backend("model-1"))
            .await
            .unwrap();
        pool.load_model("model-2", mock_backend("model-2"))
            .await
            .unwrap();
        pool.load_model("model-3", mock_backend("model-3"))
            .await
            .unwrap();

        // model-1 should have been evicted
        assert!(!pool.is_model_loaded("model-1").await);
        assert!(pool.is_model_loaded("model-2").await);
        assert!(pool.is_model_loaded("model-3").await);
    }

    #[tokio::test]
    async fn test_model_pool_get_model() {
        let config = ModelPoolConfig::default();
        let pool = ModelPool::new(config);

        // Load a model
        pool.load_model("test-model", mock_backend("test-model"))
            .await
            .unwrap();

        // Get the model — should return the backend
        let backend = pool.get_model("test-model").await;
        assert!(backend.is_some());
        assert_eq!(backend.unwrap().model_id(), "test-model");

        // Getting a non-existent model returns None
        let missing = pool.get_model("no-such-model").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_model_pool_duplicate_load() {
        let config = ModelPoolConfig::default();
        let pool = ModelPool::new(config);

        pool.load_model("m1", mock_backend("m1")).await.unwrap();
        // Loading the same model again should be a no-op (already ready)
        pool.load_model("m1", mock_backend("m1")).await.unwrap();

        let models = pool.list_models().await;
        assert_eq!(models.len(), 1);
    }

    #[tokio::test]
    async fn test_model_pool_stats() {
        let config = ModelPoolConfig::default().with_max_models(3);
        let pool = ModelPool::new(config);

        pool.load_model("m1", mock_backend("m1")).await.unwrap();
        pool.load_model("m2", mock_backend("m2")).await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.loaded_models, 2);
        assert_eq!(stats.max_models, 3);
        assert!(stats.total_memory_bytes > 0);
    }

    #[tokio::test]
    async fn test_model_pool_shutdown() {
        let config = ModelPoolConfig::default();
        let pool = ModelPool::new(config);
        pool.start();

        pool.load_model("m1", mock_backend("m1")).await.unwrap();
        pool.shutdown().await;

        assert!(!pool.is_model_loaded("m1").await);
    }
}
