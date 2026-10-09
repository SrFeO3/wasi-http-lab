//! Policy and concurrency tests for the custom P3 host.
//!
//! The minimal implementation lives in `main.rs`; this module only holds
//! the tests so `main.rs` stays readable.
use super::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Build the engine/linker pair shared by the tests below.
fn test_setup() -> anyhow::Result<(Engine, Linker<HostState>)> {
    let mut config = Config::new();
    config.wasm_component_model_async(true);
    let engine = Engine::new(&config)?;

    let mut linker = Linker::new(&engine);
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi::p3::add_to_linker(&mut linker)?;
    MycustomwasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |s| s)?;

    Ok((engine, linker))
}

fn test_component(engine: &Engine) -> anyhow::Result<Component> {
    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    Ok(Component::from_file(engine, component_path)?)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blocked_host_is_rejected() -> anyhow::Result<()> {
    // No mock server needed: the allowlist rejects before any network access.
    println!("Running policy rejection test...");
    let (engine, linker) = test_setup()?;
    let component = test_component(&engine)?;

    let mut store = Store::new(&engine, HostState::new("Greeting from Test!"));
    let bindings = MycustomwasmWorld::instantiate_async(&mut store, &component, &linker).await?;
    store
        .run_concurrent(async |accessor| {
            bindings
                .call_webpageinspector(accessor, "http://169.254.169.254/".to_string())
                .await
        })
        .await??;

    let status = store
        .data()
        .dictionary
        .get("wasm-guest-exec-status")
        .cloned()
        .unwrap_or_default();
    assert!(
        status.contains("blocked host"),
        "blocked host must fail with a policy error, got: {status}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blocked_scheme_is_rejected() -> anyhow::Result<()> {
    // No mock server needed: the scheme check rejects before any network access.
    println!("Running scheme rejection test...");
    let (engine, linker) = test_setup()?;
    let component = test_component(&engine)?;

    let mut store = Store::new(&engine, HostState::new("Greeting from Test!"));
    let bindings = MycustomwasmWorld::instantiate_async(&mut store, &component, &linker).await?;
    store
        .run_concurrent(async |accessor| {
            bindings
                .call_webpageinspector(accessor, "ftp://localhost/file".to_string())
                .await
        })
        .await??;

    let status = store
        .data()
        .dictionary
        .get("wasm-guest-exec-status")
        .cloned()
        .unwrap_or_default();
    assert!(
        status.contains("blocked scheme"),
        "blocked scheme must fail with a policy error, got: {status}"
    );
    Ok(())
}

/// Helper to run Wasm guests concurrently.
async fn run_concurrent_requests(
    concurrency: usize,
    expected_min_duration: Duration,
    test_label: &str,
) -> anyhow::Result<()> {
    // 1. Initialize shared resources
    let (engine, linker) = test_setup()?;
    let component = test_component(&engine)?;

    // Safe sharing across tasks
    let engine = Arc::new(engine);
    let component = Arc::new(component);
    let linker = Arc::new(linker);

    let base_url = format!(
        "http://localhost:8080/sleep/{}",
        expected_min_duration.as_secs()
    );

    let mut handles = Vec::new();
    let start = Instant::now();

    // 2. Spawn parallel tasks
    for task_id in 1..=concurrency {
        let engine = Arc::clone(&engine);
        let component = Arc::clone(&component);
        let linker = Arc::clone(&linker);
        let label = test_label.to_string();
        let b_url = base_url.clone();

        let handle: tokio::task::JoinHandle<anyhow::Result<Duration>> = tokio::spawn(async move {
            let url = if concurrency > 1 {
                format!(
                    "{}?id={}&total={}&test={}",
                    b_url, task_id, concurrency, label
                )
            } else {
                format!("http://localhost:8080/index.html?test={}", label)
            };

            let start_req = Instant::now();
            let state = HostState::new(&format!("{}-Guest-{}", label, task_id));
            let mut store = Store::new(&engine, state);
            let bindings =
                MycustomwasmWorld::instantiate_async(&mut store, &component, &linker).await?;
            store
                .run_concurrent(async |accessor| {
                    bindings.call_webpageinspector(accessor, url).await
                })
                .await??;
            Ok(start_req.elapsed())
        });
        handles.push(handle);
    }

    let mut durations = Vec::new();
    // 3. Wait for completion
    for handle in handles {
        durations.push(handle.await??);
    }
    let elapsed = start.elapsed();

    // 4. Report statistics
    println!("\n--- Concurrency Test Statistics ---");
    println!("Test Label: {}", test_label);
    println!("Concurrency level: {}", concurrency);
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

        let parallel_efficiency_threshold = expected_min_duration.mul_f64(1.5);
        assert!(
            elapsed < expected_min_duration * concurrency as u32 / 2,
            "Parallelism failed: Wall time too long"
        );
        assert!(
            elapsed <= parallel_efficiency_threshold,
            "Efficiency failed: Took {:?}, exceeding threshold",
            elapsed
        );
        println!("Parallelism and Efficiency confirmed.");
    }
    println!("-----------------------------------");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_10_parallel() -> anyhow::Result<()> {
    run_concurrent_requests(10, Duration::from_secs(3), "parallel-10").await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_50_parallel() -> anyhow::Result<()> {
    run_concurrent_requests(50, Duration::from_secs(3), "parallel-50").await
}
