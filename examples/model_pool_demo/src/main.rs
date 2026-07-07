//! ModelPool Lifecycle Demo
//!
//! Demonstrates the core behaviors of `mofa_foundation::inference::model_pool::ModelPool`:
//!
//! 1. **Load models** into a capacity-constrained pool
//! 2. **LRU eviction** when the pool is full
//! 3. **Touch** to refresh a model's LRU timestamp
//! 4. **Idle timeout eviction** for unused models
//! 5. **Memory-pressure eviction** down to a target budget
//! 6. **Graceful cleanup** (explicit unload)
//!
//! No real LLM backend is needed — memory values are simulated.
//!
//! Run with:
//! ```sh
//! cargo run -p model_pool_demo
//! ```

use std::thread;
use std::time::Duration;

use mofa_foundation::inference::Precision;
use mofa_foundation::inference::model_pool::ModelPool;
use mofa_foundation::inference::RequestPriority;

fn main() {
    println!("=== MoFA ModelPool Lifecycle Demo ===\n");

    // ---------------------------------------------------------------
    // 1. Create a pool with capacity=2 and a 1-second idle timeout
    // ---------------------------------------------------------------
    let idle_timeout = Duration::from_secs(1);
    let mut pool = ModelPool::new(2, idle_timeout);

    println!("Pool created  (capacity=2, idle_timeout=1s)");
    println!(
        "  loaded: {}  memory: {} MB\n",
        pool.len(),
        pool.total_memory_mb()
    );

    // ---------------------------------------------------------------
    // 2. Load two models — pool is now full
    // ---------------------------------------------------------------
    println!("--- Loading models A and B ---");

    let evicted = pool.load("model-A", 4096, Precision::F16, RequestPriority::Normal);
    println!("  Loaded model-A (4096 MB, F16)  evicted: {:?}", evicted);

    // Small sleep so model-B has a strictly later timestamp than model-A
    thread::sleep(Duration::from_millis(10));

    let evicted = pool.load("model-B", 2048, Precision::Q8, RequestPriority::Normal);
    println!("  Loaded model-B (2048 MB, Q8)   evicted: {:?}", evicted);

    println!(
        "  Pool: {} models, {} MB total\n",
        pool.len(),
        pool.total_memory_mb()
    );
    assert_eq!(pool.len(), 2);
    assert_eq!(pool.total_memory_mb(), 4096 + 2048);

    // ---------------------------------------------------------------
    // 3. Load a third model — triggers LRU eviction of model-A
    // ---------------------------------------------------------------
    println!("--- Loading model-C (should evict LRU model-A) ---");

    let evicted = pool.load("model-C", 8192, Precision::F32, RequestPriority::Normal);
    println!("  Loaded model-C (8192 MB, F32)  evicted: {:?}", evicted);
    println!(
        "  model-A loaded? {}  model-B loaded? {}  model-C loaded? {}",
        pool.is_loaded("model-A"),
        pool.is_loaded("model-B"),
        pool.is_loaded("model-C"),
    );
    println!(
        "  Pool: {} models, {} MB total\n",
        pool.len(),
        pool.total_memory_mb()
    );
    assert_eq!(evicted, Some("model-A".to_string()));
    assert!(!pool.is_loaded("model-A"));

    // ---------------------------------------------------------------
    // 4. Touch model-B to refresh its LRU timestamp, then load D
    //    Now model-C (untouched) is the LRU candidate, not model-B
    // ---------------------------------------------------------------
    println!("--- Touch model-B, then load model-D ---");
    pool.touch("model-B");
    thread::sleep(Duration::from_millis(10));

    let evicted = pool.load("model-D", 1024, Precision::Q4, RequestPriority::Normal);
    println!("  Loaded model-D (1024 MB, Q4)   evicted: {:?}", evicted);
    println!(
        "  model-B loaded? {}  model-C loaded? {}  model-D loaded? {}",
        pool.is_loaded("model-B"),
        pool.is_loaded("model-C"),
        pool.is_loaded("model-D"),
    );
    println!(
        "  Pool: {} models, {} MB total\n",
        pool.len(),
        pool.total_memory_mb()
    );
    assert_eq!(evicted, Some("model-C".to_string()));

    // ---------------------------------------------------------------
    // 5. Retrieve model info
    // ---------------------------------------------------------------
    println!("--- Retrieve model info ---");
    if let Some(entry) = pool.get("model-D") {
        println!(
            "  model-D -> {} MB, precision={}",
            entry.memory_mb, entry.precision
        );
    }
    if let Some(entry) = pool.get("model-B") {
        println!(
            "  model-B -> {} MB, precision={}",
            entry.memory_mb, entry.precision
        );
    }
    println!();

    // ---------------------------------------------------------------
    // 6. Idle timeout eviction
    //    Wait longer than idle_timeout (1s), then call evict_idle()
    // ---------------------------------------------------------------
    println!("--- Idle timeout eviction ---");
    println!("  Sleeping 1.2s to exceed idle timeout...");
    thread::sleep(Duration::from_millis(1200));

    let idle_evicted = pool.evict_idle();
    println!("  Idle-evicted models: {:?}", idle_evicted);
    println!(
        "  Pool: {} models, {} MB total\n",
        pool.len(),
        pool.total_memory_mb()
    );
    assert!(
        pool.is_empty(),
        "all models should have been evicted after idle timeout"
    );

    // ---------------------------------------------------------------
    // 7. Memory-pressure eviction (evict_until_below)
    // ---------------------------------------------------------------
    println!("--- Memory-pressure eviction ---");
    pool.load("heavy-1", 5000, Precision::F16, RequestPriority::Normal);
    thread::sleep(Duration::from_millis(10));
    pool.load("heavy-2", 7000, Precision::F16, RequestPriority::Normal);
    println!(
        "  Loaded heavy-1 (5000 MB) and heavy-2 (7000 MB)  total={} MB",
        pool.total_memory_mb()
    );

    let pressure_evicted = pool.evict_until_below(6000);
    println!("  Evicted to stay below 6000 MB: {:?}", pressure_evicted);
    println!(
        "  Pool: {} models, {} MB total\n",
        pool.len(),
        pool.total_memory_mb()
    );
    assert!(pool.total_memory_mb() <= 6000);

    // ---------------------------------------------------------------
    // 8. Explicit unload (graceful cleanup)
    // ---------------------------------------------------------------
    println!("--- Graceful cleanup ---");
    let freed = pool.unload("heavy-2");
    println!("  Unloaded heavy-2, freed {} MB", freed);
    println!(
        "  Pool: {} models, {} MB total\n",
        pool.len(),
        pool.total_memory_mb()
    );

    println!("=== Demo completed successfully! ===");
}
