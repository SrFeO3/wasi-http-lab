// A simple caching HTTP GET proxy for the Wasm host.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wasmtime::component::{Component, HasSelf, Linker, bindgen};
use wasmtime::{Config, Engine, Result, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

// WIT Bindings
bindgen!({
    path: "../wit",
    world: "mywasm-world",
    require_store_data_send: true,
    imports: { default: async },
    exports: { default: async },
});

struct HostState {
    ctx: WasiCtx,
    http_ctx: WasiHttpCtx,
    table: ResourceTable,
    hooks: [(); 0],
    kv: HashMap<String, String>,
}

impl mywasm::demo::kv_ops::Host for HostState {
    async fn kv_set(&mut self, key: String, value: String) {
        self.kv.insert(key, value);
    }

    async fn kv_get(&mut self, key: String) -> Option<String> {
        self.kv.get(&key).cloned()
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for HostState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http_ctx,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Thread-safe cache for HTTP responses
    let cache: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

    // 1. Wasmtime Engine Setup
    let mut config = Config::new();
    config.wasm_component_model(true);

    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);

    // 2. Link WASI P2 and WASI-HTTP
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
    MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    // 3. Initialize Host Context
    let ctx = WasiCtxBuilder::new().inherit_stdio().build();
    let http_ctx = WasiHttpCtx::new();
    let table = ResourceTable::new();

    let state = HostState {
        ctx,
        http_ctx,
        table,
        hooks: [],
        kv: HashMap::new(),
    };
    let mut store = Store::new(&engine, state);

    // 4. load WASM component
    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Component::from_file(&engine, component_path)
        .expect("Failed to load component. Please build wguest first.");

    // 5. Instantiate Component
    let bindings = MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;

    // 6. Execute Guest Export (URL based)
    let target_url = "http://localhost:8080/test1.html";
    println!("Host: Requesting URL -> {}", target_url);

    // 7. Cache Logic
    let cached_result = {
        let cache_lock = cache.lock().unwrap();
        cache_lock.get(target_url).cloned()
    };

    let result = if let Some(data) = cached_result {
        println!("Host: [Cache Hit] Returning data for {}", target_url);
        data
    } else {
        println!("Host: [Cache Miss] Invoking guest for {}", target_url);
        // Dynamic URL specification
        let res = bindings
            .call_webpage_inspector(&mut store, target_url)
            .await?;

        // Store result in cache
        let mut cache_lock = cache.lock().expect("lock failed");
        cache_lock.insert(target_url.to_string(), res.clone());
        res
    };

    println!("=== Response from localhost:8080 ===");
    println!("{}", result);
    println!("====================================");

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[tokio::test(flavor = "multi_thread")]
    async fn test_host_cache_integration() -> Result<()> {
        println!("Running cache integration test...");
        // Shared cache for this test instance
        let cache: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let target_url = "http://localhost:8080/cache-test.html?test=cache-dummy";

        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);
        linker.allow_shadowing(true);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
        MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;
        let linker = Arc::new(linker);

        let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
        let component = Arc::new(Component::from_file(&engine, component_path)?);

        // Initial KV state
        let kv = HashMap::from([(
            "host-greeting".to_string(),
            "Greeting from Cache Test!".to_string(),
        )]);

        // 1. First Call: Cache Miss
        let mut store1 = Store::new(
            &engine,
            HostState {
                table: ResourceTable::new(),
                ctx: WasiCtxBuilder::new().build(),
                http_ctx: WasiHttpCtx::new(),
                hooks: [],
                kv: kv.clone(),
            },
        );
        let bindings1 = MywasmWorld::instantiate_async(&mut store1, &component, &linker).await?;

        let result1 = {
            let cached = cache.lock().unwrap().get(target_url).cloned();
            if let Some(data) = cached {
                data
            } else {
                let res = bindings1
                    .call_webpage_inspector(&mut store1, target_url)
                    .await?;
                cache
                    .lock()
                    .unwrap()
                    .insert(target_url.to_string(), res.clone());
                res
            }
        };

        assert!(
            result1.starts_with("webpage-body-size is"),
            "Response must return body size format"
        );

        // Verify host hashtable interaction (Wasm guest should have written these)
        let state1 = store1.data();
        assert_eq!(
            state1.kv.get("wasm-guest-exec-status").map(|s| s.as_str()),
            Some("success")
        );
        assert_eq!(
            state1.kv.get("webpage-url").map(|s| s.as_str()),
            Some(target_url)
        );

        // 2. Second Call: Cache Hit
        let cached_val = cache.lock().unwrap().get(target_url).cloned();
        assert!(
            cached_val.is_some(),
            "Entry must exist in cache after first run"
        );
        assert_eq!(
            cached_val.unwrap(),
            result1,
            "Cached content must match first execution"
        );

        Ok(())
    }

    /// Helper to run Wasm guests concurrently.
    /// - If use_cache is false: Every request triggers a Wasm execution.
    /// - If use_cache is true: Subsequent requests hit the host-side cache.
    async fn run_concurrent_requests(
        concurrency: usize,
        expected_min_duration: Duration,
        use_cache: bool,
        test_label: &str,
    ) -> Result<()> {
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

        let test_cache: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

        let engine = Arc::new(engine);
        let component = Arc::new(component);
        let linker = Arc::new(linker);

        let base_url = format!(
            "http://localhost:8080/sleep/{}",
            expected_min_duration.as_secs()
        );
        let shared_url = format!("{}?test=cache-dummy", base_url);

        // Warm the cache if enabled
        if use_cache {
            let mut store = Store::new(
                &engine,
                HostState {
                    table: ResourceTable::new(),
                    ctx: WasiCtxBuilder::new().build(),
                    http_ctx: WasiHttpCtx::new(),
                    hooks: [],
                    kv: HashMap::new(),
                },
            );
            let bindings = MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;
            let result = bindings
                .call_webpage_inspector(&mut store, &shared_url)
                .await?;
            test_cache
                .lock()
                .unwrap()
                .insert(shared_url.clone(), result);
        }

        let mut handles = Vec::new();
        let start = Instant::now();

        for task_id in 0..concurrency {
            let engine = Arc::clone(&engine);
            let component = Arc::clone(&component);
            let linker = Arc::clone(&linker);
            let test_cache = Arc::clone(&test_cache);
            let label = test_label.to_string();
            let b_url = base_url.clone();
            let s_url_full = shared_url.clone();

            let handle: tokio::task::JoinHandle<Result<Duration>> = tokio::spawn(async move {
                let table = ResourceTable::new();
                let ctx = WasiCtxBuilder::new().build();
                let http_ctx = WasiHttpCtx::new();

                let host_ctx = HostState {
                    table,
                    ctx,
                    http_ctx,
                    hooks: [],
                    kv: HashMap::new(),
                };
                let mut store = Store::new(&engine, host_ctx);

                let url = if use_cache {
                    s_url_full
                } else if concurrency > 1 {
                    format!(
                        "{}?id={}&total={}&test={}",
                        b_url, task_id, concurrency, label
                    )
                } else {
                    format!("http://localhost:8080/index.html?test={}", label)
                };

                // Host-side cache check
                if use_cache {
                    let hit = { test_cache.lock().unwrap().get(&url).cloned() };
                    if hit.is_some() {
                        return Ok(Duration::from_nanos(0));
                    }
                }

                let task_start = Instant::now();
                let bindings =
                    MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;
                let result = bindings.call_webpage_inspector(&mut store, &url).await?;
                let task_duration = task_start.elapsed();

                if use_cache {
                    test_cache.lock().unwrap().insert(url, result.clone());
                }

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
        for handle in handles {
            durations.push(handle.await.unwrap()?);
        }
        let elapsed = start.elapsed();

        println!("\n--- Concurrency Test Statistics ---");
        println!("Test Label: {}", test_label);
        println!(
            "Cache mode: {}",
            if use_cache {
                "Enabled (Warmed)"
            } else {
                "Disabled"
            }
        );
        println!("Concurrency level: {}", concurrency);
        println!("Total elapsed time: {:?}", elapsed);

        if !durations.is_empty() {
            let min_d = durations.iter().min().unwrap();
            let max_d = durations.iter().max().unwrap();
            let sum_d: Duration = durations.iter().sum();
            let avg_d = sum_d / durations.len() as u32;
            println!("Individual Task Duration Stats:");
            println!("  Min: {:?}, Max: {:?}, Avg: {:?}", min_d, max_d, avg_d);
        }

        if concurrency > 1 {
            let serial_duration = expected_min_duration * concurrency as u32;
            let serial_threshold = serial_duration / 2;
            let parallel_efficiency_threshold = expected_min_duration.mul_f64(1.5); // Allow more overhead for tasks

            if !use_cache {
                assert!(
                    elapsed < serial_threshold,
                    "Parallelism failed: Took {:?}",
                    elapsed
                );
                assert!(
                    elapsed <= parallel_efficiency_threshold,
                    "Efficiency failed: Took {:?}",
                    elapsed
                );
            } else {
                assert!(
                    elapsed < expected_min_duration,
                    "Cache Hit should be faster than one network request"
                );
            }
        }

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrency_cache_01_single() -> Result<()> {
        run_concurrent_requests(1, Duration::from_secs(2), true, "cache_single").await
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrency_cache_10_parallel() -> Result<()> {
        run_concurrent_requests(10, Duration::from_secs(2), true, "cache_10").await
    }
}
