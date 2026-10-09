use std::collections::HashMap;

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
    // 1. Engine with the async component model (required for P3).
    let mut config = Config::new();
    config.wasm_component_model_async(true);
    let engine = Engine::new(&config)?;

    // 2. Link WASI P3 and WASI-HTTP P3 (synchronous registration).
    // WASI P2 is also linked: the async ABI glue still imports
    // e.g. `wasi:io/poll@0.2.x` (same as wasmtime's own P3 tests).
    let mut linker = Linker::new(&engine);
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi::p3::add_to_linker(&mut linker)?;
    wasmtime_wasi_http::p3::add_to_linker(&mut linker)?;
    MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    // 3. Host context.
    let kv = HashMap::from([(
        "host-greeting".to_string(),
        "Hello from the Host!".to_string(),
    )]);
    let state = HostState {
        ctx: WasiCtxBuilder::new().inherit_stdio().build(),
        http_ctx: WasiHttpCtx::new(),
        table: ResourceTable::new(),
        hooks: [],
        kv,
    };
    let mut store = Store::new(&engine, state);

    // 4. load WASM component
    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Component::from_file(&engine, component_path)
        .expect("Failed to load component. Please build wguest first.");

    // 5. Instantiate Component
    let bindings = MywasmWorld::instantiate_async(&mut store, &component, &linker).await?;

    // 6. Execute Guest Export (P3 async exports are invoked concurrently
    // via `Store::run_concurrent`, which supplies the `Accessor`).
    let target_url = "http://localhost:8080/test1.html";
    println!("Host: Requesting URL -> {}", target_url);

    let result = store
        .run_concurrent(async |accessor| {
            bindings
                .call_webpage_inspector(accessor, target_url.to_string())
                .await
        })
        .await??;

    println!("=== Response from web server ===");
    println!("{}", result);
    println!("====================================");

    Ok(())
}

// Tests live in `test.rs` so this file stays the minimal implementation.
#[cfg(test)]
mod test;
