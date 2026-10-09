use std::collections::HashMap;
use std::time::Duration;

use wasmtime::component::{Accessor, Component, HasSelf, Linker, bindgen};
use wasmtime::{Config, Engine, Result, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

// WIT Bindings
bindgen!({
    path: "../wit",
    world: "mycustomwasm-world",
    imports: { default: async },
    exports: { default: async },
});

/// Security policy enforced by the host. Every limit is explicit here so
/// the policy is auditable in one place. The guest cannot change any of it.
#[derive(Clone)]
struct Policy {
    /// Only these hosts may be fetched (SSRF protection).
    allowed_hosts: Vec<String>,
    /// Only these schemes may be fetched.
    allowed_schemes: Vec<String>,
    /// Per-request timeout, applied to the whole request including the body.
    timeout: Duration,
    /// Maximum accepted body size, in bytes.
    max_body_bytes: usize,
    /// Maximum followed redirects.
    max_redirects: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            allowed_hosts: vec!["localhost".to_string(), "127.0.0.1".to_string()],
            allowed_schemes: vec!["http".to_string(), "https".to_string()],
            timeout: Duration::from_secs(10),
            max_body_bytes: 1024 * 1024,
            max_redirects: 5,
        }
    }
}

struct HostState {
    dictionary: HashMap<String, String>,
    ctx: WasiCtx,
    table: ResourceTable,
    policy: Policy,
    client: reqwest::Client,
}

impl HostState {
    fn new(greeting: &str) -> Self {
        let policy = Policy::default();
        // The builder only fails on programming errors (e.g. an invalid
        // timeout), never on request input, so `expect` is safe here.
        // All request-time failures below are returned as values.
        let client = reqwest::Client::builder()
            .timeout(policy.timeout)
            .redirect(reqwest::redirect::Policy::limited(policy.max_redirects))
            .build()
            .expect("failed to build HTTP client");
        Self {
            dictionary: HashMap::from([("host-greeting".to_string(), greeting.to_string())]),
            ctx: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::new(),
            policy,
            client,
        }
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

use crate::mycustomwasm::demo::http;
// WIT `async` imports are implemented through `HostWithStore`: the method
// takes an `Accessor` instead of `&mut self`. `Host` itself is just a marker.
impl http::Host for HostState {}

impl http::HostWithStore<HostState> for HasSelf<HostState> {
    async fn get(store: &Accessor<HostState, Self>, url: String) -> Result<String, String> {
        // Policy and client live in shared state. `with` runs a sync
        // closure, so copy out what is needed before any await.
        let (policy, client) = store.with(|mut access| {
            let state: &mut HostState = access.get();
            (state.policy.clone(), state.client.clone())
        });

        // 1. Parse the URL. Never trust string matching for security checks.
        let parsed = url::Url::parse(&url).map_err(|e| format!("invalid url {url:?}: {e}"))?;

        // 2. Allowlist the scheme.
        if !policy.allowed_schemes.iter().any(|s| s == parsed.scheme()) {
            return Err(format!("blocked scheme {:?} for {url:?}", parsed.scheme()));
        }

        // 3. Allowlist the host (SSRF protection: no cloud metadata
        // endpoints, no intranet hosts unless listed above).
        let host = parsed
            .host_str()
            .ok_or_else(|| format!("url has no host: {url:?}"))?;
        if !policy.allowed_hosts.iter().any(|h| h == host) {
            return Err(format!("blocked host {host:?} for {url:?}"));
        }

        // 4. Fetch. The client enforces the timeout and redirect limit.
        // No `unwrap`: every failure becomes an error value for the guest.
        let resp = client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| format!("request failed for {url:?}: {e}"))?;

        // 5. Reject oversized bodies early when the server declares the size.
        if let Some(len) = resp.content_length()
            && len > policy.max_body_bytes as u64
        {
            return Err(format!(
                "body too large (declared {len} bytes, limit is {}): {url:?}",
                policy.max_body_bytes
            ));
        }

        // 6. Read the body and enforce the cap on the actual bytes.
        // `from_utf8_lossy` never fails; binary bodies become replacement text.
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("failed to read body for {url:?}: {e}"))?;
        if bytes.len() > policy.max_body_bytes {
            return Err(format!(
                "body too large ({} bytes, limit is {}): {url:?}",
                bytes.len(),
                policy.max_body_bytes
            ));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
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
    // 1. Wasmtime Engine Setup with the async component model (P3 runtime).
    let mut config = Config::new();
    config.wasm_component_model_async(true);

    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);

    // 2. Link WASI P3 and MycustomwasmWorld (synchronous P3 registration).
    // WASI P2 is also linked: the async ABI glue still imports
    // e.g. `wasi:io/poll@0.2.x` (same as wasmtime's own P3 tests).
    linker.allow_shadowing(true);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi::p3::add_to_linker(&mut linker)?;
    MycustomwasmWorld::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)?;

    // 3. Initialize Host Context (policy + client included).
    let state = HostState::new("Hello from the Host!");
    let mut store = Store::new(&engine, state);

    // 4. load WASM component
    let component_path = "../wguest/target/wasm32-wasip2/release/wguest.wasm";
    let component = Component::from_file(&engine, component_path)
        .expect("Failed to load component. Please build wguest first.");

    // 5. Instantiate Component
    let bindings = MycustomwasmWorld::instantiate_async(&mut store, &component, &linker).await?;

    // 6. Execute Guest Export through `run_concurrent` (async export).
    let target_url = "http://localhost:8080/";
    println!("Host: Requesting URL -> {}", target_url);

    store
        .run_concurrent(async |accessor| {
            bindings
                .call_webpageinspector(accessor, target_url.to_string())
                .await
        })
        .await??;

    println!("\n[Host] Final Dictionary State:");
    let final_dict = &store.data().dictionary;
    for (k, v) in final_dict {
        println!("  {}: {}", k, v);
    }

    Ok(())
}

// Concurrency and policy tests live in `test.rs` so this file stays minimal.
#[cfg(test)]
mod test;
