//! Integration/concurrency tests for the P2 standard-HTTP host.
//!
//! The minimal WASI implementation lives in `main.rs`; this module only holds
//! the tests so `main.rs` stays readable.
use super::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn test_host_path_selection() -> Result<()> {
    println!("Running host URL selection test...");
    let target_url = "http://localhost:8080/verify-this-specific-path";

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Arc::new(Engine::new(&config)?);
    let mut linker = Linker::new(&engine);
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
    MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;
    let linker = Arc::new(linker);

    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Arc::new(Component::from_file(&engine, component_path)?);

    let kv = HashMap::from([(
        "host-greeting".to_string(),
        "Greeting from Test!".to_string(),
    )]);

    let mut store = Store::new(
        &engine,
        HostState {
            table: ResourceTable::new(),
            ctx: WasiCtxBuilder::new().build(),
            http_ctx: WasiHttpCtx::new(),
            hooks: [],
            kv,
        },
    );
    let bindings = MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;
    let result = bindings
        .call_webpage_inspector(&mut store, target_url)
        .await?;

    assert!(
        result.starts_with("webpage-body-size is"),
        "Response must return body size format"
    );

    // Verify host hashtable interaction
    let state = store.data();
    assert_eq!(
        state.kv.get("wasm-guest-exec-status").map(|s| s.as_str()),
        Some("success")
    );
    assert_eq!(
        state.kv.get("webpage-url").map(|s| s.as_str()),
        Some(target_url)
    );

    Ok(())
}

/// Helper to run Wasm guests concurrently.
async fn run_concurrent_requests(
    concurrency: usize,
    expected_min_duration: Duration,
    test_label: &str,
) -> Result<()> {
    // 1. Initialize shared resources and local test cache
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    let mut linker = Linker::new(&engine);
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
    MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Component::from_file(&engine, component_path)
        .expect("Component not found. Please build wguest first.");

    // Safe sharing across tasks
    let engine = Arc::new(engine);
    let component = Arc::new(component);
    let linker = Arc::new(linker);

    // Consistent path for both warming and task execution
    let base_url = format!(
        "http://localhost:8080/sleep/{}",
        expected_min_duration.as_secs()
    );

    let mut handles = Vec::new();
    let start = Instant::now();

    // 2. Spawn parallel tasks
    for task_id in 0..concurrency {
        let engine = Arc::clone(&engine);
        let component = Arc::clone(&component);
        let linker = Arc::clone(&linker);
        let label = test_label.to_string();
        let b_url = base_url.clone();

        let handle: tokio::task::JoinHandle<Result<Duration>> = tokio::spawn(async move {
            // Store/Ctx must be unique per task (thread)
            let table = ResourceTable::new();

            let ctx = WasiCtxBuilder::new().inherit_stdio().build();
            let http_ctx = WasiHttpCtx::new();

            let host_ctx = HostState {
                table,
                ctx,
                http_ctx,
                hooks: [],
                kv: HashMap::new(),
            };
            let mut store = Store::new(&engine, host_ctx);

            // Use /sleep/N for concurrent tests
            let url = if concurrency > 1 {
                // Unique path per task to ensure track on server
                format!(
                    "{}?id={}&total={}&test={}",
                    b_url, task_id, concurrency, label
                )
            } else {
                format!("http://localhost:8080/index.html?test={}", label)
            };

            let task_start = Instant::now();
            let bindings = MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;
            let result = bindings.call_webpage_inspector(&mut store, &url).await?;
            let task_duration = task_start.elapsed();

            assert!(
                !result.is_empty(),
                "Task {} failed. Unexpected empty response",
                task_id
            );
            Ok(task_duration)
        });
        handles.push(handle);
    }

    let mut durations = Vec::new();
    // 3. Wait for completion
    for handle in handles {
        durations.push(handle.await.unwrap()?);
    }
    let elapsed = start.elapsed();

    // 4. Output results
    println!("\n--- Concurrency Test Statistics ---");
    println!("Test Label: {}", test_label);
    println!("Concurrency level: {}", concurrency);
    println!("Parallelism model: Tokio Async Tasks (Multi-threaded)");
    println!("Total elapsed time: {:?}", elapsed);

    if !durations.is_empty() {
        let min_d = durations.iter().min().unwrap();
        let max_d = durations.iter().max().unwrap();
        let sum_d: Duration = durations.iter().sum();
        let avg_d = sum_d / durations.len() as u32;

        println!("Individual Task Duration Stats:");
        println!("  Min: {:?}", min_d);
        println!("  Max: {:?}", max_d);
        println!("  Avg: {:?}", avg_d);
    }

    if concurrency > 1 {
        let speedup =
            (expected_min_duration.as_secs_f64() * concurrency as f64) / elapsed.as_secs_f64();
        println!("Estimated speedup factor vs Serial: {:.2}x", speedup);
    }
    println!("-----------------------------------");

    if concurrency > 1 {
        // Assert: Total time < (Serial / 2) AND Total time <= (Single * 1.2)
        let serial_duration = expected_min_duration * concurrency as u32;
        let serial_threshold = serial_duration / 2;
        let parallel_efficiency_threshold = expected_min_duration.mul_f64(1.2);

        assert!(
            elapsed < serial_threshold,
            "Parallelism failed: Took {:?}, but threshold (half of serial) is {:?}",
            elapsed,
            serial_threshold
        );
        assert!(
            elapsed <= parallel_efficiency_threshold,
            "Efficiency failed: Took {:?}, exceeding 1.2x of single execution ({:?})",
            elapsed,
            parallel_efficiency_threshold
        );

        println!("Parallelism and Efficiency confirmed.");
    }

    Ok(())
}

// --- Concurrency Tests [No Cache] ---
// These tests ensure the WASM host can handle multiple heavy executions in parallel.

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_no_cache_01_single() -> Result<()> {
    run_concurrent_requests(1, Duration::from_secs(3), "no_cache_single").await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_no_cache_10_parallel() -> Result<()> {
    run_concurrent_requests(10, Duration::from_secs(3), "no_cache_10").await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_no_cache_50_parallel() -> Result<()> {
    run_concurrent_requests(50, Duration::from_secs(3), "no_cache_50").await
}

/// Helper to run Wasm guests sequentially in a loop (No tokio::spawn).
/// This provides a baseline for serial execution time.
async fn run_sequential_requests(
    concurrency: usize,
    expected_min_duration: Duration,
    test_label: &str,
) -> Result<()> {
    // 1. Initialize resources
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    let mut linker = Linker::new(&engine);
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
    MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Component::from_file(&engine, component_path)
        .expect("Component not found. Please build wguest first.");

    let mut durations = Vec::new();
    let start = Instant::now();

    // 2. Sequential execution loop
    for task_id in 0..concurrency {
        let mut store = Store::new(
            &engine,
            HostState {
                table: ResourceTable::new(),
                ctx: WasiCtxBuilder::new().inherit_stdio().build(),
                http_ctx: WasiHttpCtx::new(),
                hooks: [],
                kv: HashMap::new(),
            },
        );

        let bindings = MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;
        let url = format!(
            "http://localhost:8080/sleep/{}?id={}&total={}&test={}",
            expected_min_duration.as_secs(),
            task_id,
            concurrency,
            test_label
        );

        let task_start = Instant::now();
        let result = bindings.call_webpage_inspector(&mut store, &url).await?;
        durations.push(task_start.elapsed());

        assert!(!result.is_empty(), "Task {} failed", task_id);
    }
    let elapsed = start.elapsed();

    // 3. Output results
    println!("\n--- Serial Execution Statistics ---");
    println!("Test Label: {}", test_label);
    println!("Mode: Simple Loop (Sequential)");
    println!("Total elapsed time: {:?}", elapsed);

    if !durations.is_empty() {
        let min_d = durations.iter().min().unwrap();
        let max_d = durations.iter().max().unwrap();
        let avg_d = durations.iter().sum::<Duration>() / durations.len() as u32;

        println!("Individual Task Duration Stats:");
        println!("  Min: {:?}", min_d);
        println!("  Max: {:?}", max_d);
        println!("  Avg: {:?}", avg_d);
    }
    println!("-----------------------------------");

    Ok(())
}

// --- Concurrency Tests [Serial Baseline] ---

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_serial_loop_10() -> Result<()> {
    run_sequential_requests(10, Duration::from_secs(3), "serial_loop_10").await
}
