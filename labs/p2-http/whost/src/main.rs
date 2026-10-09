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
    // 1. Wasmtime Engine Setup
    let mut config = Config::new();
    config.wasm_component_model(true);

    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);

    // 2. Link WASI P2 and WASI-HTTP P2 (explicitly P2, not P3:
    // `wasmtime_wasi::p2` / `wasmtime_wasi_http::p2` implement WASI 0.2.x.
    // The P3 counterparts live in `::p3` and are experimental.)
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
    MywasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    // 3. Initialize Host Context
    let ctx = WasiCtxBuilder::new().inherit_stdio().build();
    let http_ctx = WasiHttpCtx::new();
    let table = ResourceTable::new();

    let kv = HashMap::from([(
        "host-greeting".to_string(),
        "Hello from the Host!".to_string(),
    )]);

    let state = HostState {
        ctx,
        http_ctx,
        table,
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

    // 6. Execute Guest Export
    let target_url = "http://localhost:8080/test1.html";
    println!("Host: Requesting URL -> {}", target_url);

    let result = bindings
        .call_webpage_inspector(&mut store, target_url)
        .await?;

    println!("=== Response from web server ===");
    println!("{}", result);
    println!("====================================");

    Ok(())
}

#[cfg(test)]
mod test;
