use std::collections::HashMap;

use wasmtime::component::{Component, HasSelf, Linker, bindgen};
use wasmtime::{Config, Engine, Result, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

// WIT Bindings
bindgen!({
    path: "../wit",
    world: "mycustomwasm-world",
    imports: { default: async },
    exports: { default: async },
});

struct HostState {
    dictionary: HashMap<String, String>,
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

use crate::mycustomwasm::demo::http;
impl http::Host for HostState {
    async fn get(&mut self, url: String) -> Result<String, String> {
        let resp = reqwest::get(url).await.map_err(|e| e.to_string())?;
        resp.text().await.map_err(|e| e.to_string())
    }
}

impl mycustomwasm::demo::dictionary_ops::Host for HostState {
    async fn dict_set(&mut self, key: String, value: String) {
        self.dictionary.insert(key, value);
    }

    async fn dict_get(&mut self, key: String) -> Option<String> {
        self.dictionary.get(&key).cloned()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Wasmtime Engine Setup
    let mut config = Config::new();
    config.wasm_component_model(true);

    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);

    // 2. Link WASI P2 and MycustomwasmWorld (P2 runtime; only the `http`
    // interface itself is custom, implemented with `reqwest` below).
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    MycustomwasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    // 3. Initialize Host Context
    let ctx = WasiCtxBuilder::new().inherit_stdio().build();
    let table = ResourceTable::new();

    let dictionary = HashMap::from([(
        "host-greeting".to_string(),
        "Hello from the Host!".to_string(),
    )]);

    let state = HostState {
        dictionary,
        ctx,
        table,
    };
    let mut store = Store::new(&engine, state);

    // 4. load WASM component
    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Component::from_file(&engine, component_path)
        .expect("Failed to load component. Please build wguest first.");

    // 5. Instantiate Component
    let bindings = MycustomwasmWorld::instantiate_async(&mut store, &component, &linker).await?;

    // 6. Execute Guest Export
    let target_url = "http://localhost:8080/";
    println!("Host: Requesting URL -> {}", target_url);

    bindings
        .call_webpageinspector(&mut store, target_url)
        .await?;

    println!("\n[Host] Final Dictionary State:");
    let final_dict = &store.data().dictionary;
    for (k, v) in final_dict {
        println!("  {}: {}", k, v);
    }

    Ok(())
}

// Concurrency tests live in `test.rs` so this file stays the minimal implementation.
#[cfg(test)]
mod test;
