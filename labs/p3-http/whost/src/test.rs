//! Integration test for the P3 HTTP host.
//!
//! The minimal WASI implementation lives in `main.rs`; this module only holds
//! the test so `main.rs` stays readable.
use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn test_host_path_selection() -> Result<()> {
    println!("Running host URL selection test...");
    let target_url = "http://localhost:8080/verify-this-specific-path";

    let mut config = Config::new();
    config.wasm_component_model_async(true);
    let engine = Engine::new(&config)?;

    let mut linker = Linker::new(&engine);
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi::p3::add_to_linker(&mut linker)?;
    wasmtime_wasi_http::p3::add_to_linker(&mut linker)?;
    MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Component::from_file(&engine, component_path)?;

    let kv = HashMap::from([(
        "host-greeting".to_string(),
        "Greeting from Test!".to_string(),
    )]);

    let mut store = Store::new(
        &engine,
        HostState {
            ctx: WasiCtxBuilder::new().build(),
            http_ctx: WasiHttpCtx::new(),
            table: ResourceTable::new(),
            hooks: [],
            kv,
        },
    );
    let bindings = MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;
    let result = store
        .run_concurrent(async |accessor| {
            bindings
                .call_webpage_inspector(accessor, target_url.to_string())
                .await
        })
        .await??;

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
