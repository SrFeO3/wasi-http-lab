# WASI HTTP Labs Comparison: p2-custom vs p2 vs p3-custom vs p3

All four labs run the same experiment: the host passes a URL, the guest
does an HTTP GET and reports the body size. They differ only in WHO runs
HTTP and THROUGH WHICH API.

| | p2-custom | p2 | p3-custom | p3 |
|---|---|---|---|---|
| Goal | Contrast with a custom interface | Standard `wasi:http` (P2) | Custom interface with host policy (P3) | Standard `wasi:http` (P3 async model) |
| Guest base (all labs) | WASM component (`cdylib` for `wasm32-wasip2`), WIT bindings via wit-bindgen 0.62 | Same | Same | Same |
| Host base (all labs) | wasmtime 49.0.2 + tokio 1.53.2, edition 2024 | Same | Same, plus `component-model-async` | Same, plus `component-model-async` |
| WASI version | P2 runtime | P2 (`wasi:http` 0.2.x) | P3 runtime | P3 (`wasi:http` 0.3.x) |
| HTTP API | Custom sync `http.get` | Standard, step by step | Custom async `http.get` | Standard, with await |
| Allow/deny control | Per-URL allow/deny in one host fn | Constrain full HTTP via hooks | Per-URL allow/deny in one host fn | Constrain full HTTP via hooks |
| HTTP call in guest | 1 line + error mapping | ~30 lines | 1 line + error mapping | ~12 lines |
| Guest code (approx) | ~19 lines | ~65 lines | ~19 lines | ~49 lines |
| Policy | None (sample) | Host implementation hooks | Explicit allowlist policy in host | Host implementation hooks |
| Portability (HTTP part) | Own host only | Generic runtimes | Own host only | Generic runtimes (future) |
| Status | Stable | Stable | Experimental (no semver guarantee) | Experimental (no semver guarantee) |

See each lab's `README.md` for setup. This file compares implementation
effort: how much guest/host code each way needs, and what knowledge it
requires.

## 1. How to read this comparison

- Two axes: **custom vs standard** (who runs HTTP) and **P2 vs P3**
  (sync vs async component model).
- Effort metric: lines of code (export-fn nonblank approx: ~19 / ~65 /
  ~19 / ~49) plus new concepts plus failure handling. Every excerpt cites
  `file:line` so counts are checkable.
- Custom guests return no value (the size lives only in the dictionary);
  standard guests return the body-size string and record it too.

## 2. Guest effort (4-way): WIT + export + HTTP call

### 2a WIT world and export definition (p2 vs p3: same except `async`)

```wit
// p2-custom (labs/p2-custom-http/wit/myinterface.wit)
interface http {
    get: func(url: string) -> result<string, string>;
}
world mycustomwasm-world {
    import dictionary-ops;
    import http;
    export webpageinspector: func(url: string);
}
// p3-custom: same, but `get: async func` and `export webpageinspector: async func`
```

```wit
// p2 (labs/p2-http/wit/mywasm.wit)
world mywasm-world {
    import kv-ops;
    export webpage-inspector: func(url: string) -> string;
}
// p3: same, but `export webpage-inspector: async func(url: string) -> string`
```

- `wasi:http` is never imported in WIT; it comes via the `wasip2` /
  `wasip3` crates. Same in all labs.
- Binding names follow the WIT spelling: custom `webpageinspector` /
  `call_webpageinspector`, standard `webpage_inspector` /
  `call_webpage_inspector`.
- Export signatures (`wguest/src/lib.rs`): p2-custom
  `fn webpageinspector(url: String)` (L13), p3-custom
  `async fn webpageinspector(url: String)` (L13), p2
  `fn webpage_inspector(url: String) -> String` (L19), p3
  `async fn webpage_inspector(url: String) -> String` (L17).

### 2b HTTP call in guest (custom vs standard): 1 line vs polling ~32 vs `await` ~11

Custom (both labs; p3 adds `.await`). See
`labs/p2-custom-http/wguest/src/lib.rs:14-22` and
`labs/p3-custom-http/wguest/src/lib.rs:14-22`:

```rust
let body = match http::get(&url) { // p3-custom: http::get(url).await
    Ok(body) => body,
    Err(err) => {
        dictionary_ops::dict_set("wasm-guest-exec-status", &format!("error: {err}"));
        return; // custom exports return nothing
    }
};
let len = body.len();
```

p2 standard: build + handle + block + 3-way match + consume + read loop.
See `labs/p2-http/wguest/src/lib.rs:33-77` (build + polling; polling
alone nonblank ~32 lines):

```rust
let headers = Headers::new();
let req = OutgoingRequest::new(headers);
req.set_method(&Method::Get).unwrap();
req.set_scheme(Some(&scheme)).unwrap();
req.set_authority(Some(authority)).unwrap();
req.set_path_with_query(Some(&path_part)).unwrap();

let future_resp = match outgoing_handler::handle(req, None) {
    Ok(f) => f,
    Err(e) => return format!("Failed to send request: {:?}", e),
};
future_resp.subscribe().block();
let resp = match future_resp.get() {
    Some(Ok(Ok(r))) => r,
    Some(Ok(Err(e))) => return format!("HTTP Error: {:?}", e),
    Some(Err(_)) => return "Response future failed".to_string(),
    None => return "Response not ready".to_string(),
};
let body = match resp.consume() {
    Ok(b) => b,
    Err(_) => return "Failed to consume response body".to_string(),
};
let stream = body.stream().expect("Failed to get response stream");
let mut result_bytes = Vec::new();
loop {
    stream.subscribe().block();
    match stream.read(4096) {
        Ok(bytes) => result_bytes.extend_from_slice(&bytes),
        Err(StreamError::Closed) => break,
        Err(e) => return format!("Stream error: {:?}", e),
    }
}
```

p3 standard: build + `send().await` + `collect().await` (no polling loop).
See `labs/p3-http/wguest/src/lib.rs:31-57` (build + send/collect;
send/collect alone nonblank ~11 lines):

```rust
let (_trailers_tx, trailers_rx) = wit_future::new(|| Ok(None));
let (request, transmit_rx) = Request::new(Fields::new(), None, trailers_rx, None);
if request.set_method(&Method::Get).is_err()
    || request.set_scheme(Some(&scheme)).is_err()
    || request.set_authority(Some(authority)).is_err()
    || request.set_path_with_query(Some(&path_part)).is_err()
{
    return "Failed to build request".to_string();
}
let response: Response = match send(request).await {
    Ok(r) => r,
    Err(e) => return format!("HTTP Error: {e:?}"),
};
let (body_stream, body_done) = Response::consume_body(response, transmit_rx);
let result_bytes = body_stream.collect().await;
if let Err(e) = body_done.await {
    return format!("Body trailers error: {e:?}");
}
```

URL parsing, dictionary exchange, and the greeting print are the same in
purpose (see §2c). New in P3 only: composing several `send()` calls with
`join!` and friends (this sample keeps a single GET so the four labs stay
comparable).

### 2c Concepts and error handling the guest must know

| | p2-custom | p2 | p3-custom | p3 |
|---|---|---|---|---|
| Must know | WIT `result` only | `OutgoingRequest`/`Headers`/`Scheme`/`Method`, `outgoing_handler`, `subscribe().block()`/`get()`, `consume()`, `InputStream.read` loop, `StreamError` | WIT async `result` only | `Request`/`Fields`/`Method`/`Scheme`, `wit_future` trailers, `send().await`, `consume_body`/`collect`/`body_done` |
| On failure | host error string into dict, return (no value) | nested results mapped to strings (handle / get / consume / read) | same as p2-custom (async) | `send` / `collect` / trailer errors mapped to strings |
| Export fn | ~19 lines | ~65 lines | ~19 lines | ~49 lines |

- Dictionary exchange is the same in purpose across all four labs
  (status/size + `host-greeting` round trip); interfaces differ in code
  (`kv-ops` with 3 keys incl. `webpage-url` vs `dictionary-ops` with
  2 keys).
- The shortness of custom comes from moving HTTP complexity into the
  host. It compares "calling a custom abstraction", not "using a standard
  HTTP API". Keep that distinction in mind when reading the samples.

## 3. Host effort (4-way): registration + HTTP/policy + call style

### 3a Engine/linker registration (p2: 2 registrations, p3: 3 + async model)

```rust
// p2 standard (labs/p2-http/whost/src/main.rs:57-68)
config.wasm_component_model(true);
wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
```

```rust
// p3 standard (labs/p3-http/whost/src/main.rs:56-69)
config.wasm_component_model_async(true); // mandatory for P3
wasmtime_wasi::p2::add_to_linker_async(&mut linker)?; // also linked for async ABI glue
wasmtime_wasi::p3::add_to_linker(&mut linker)?;       // sync registration (no async variant)
wasmtime_wasi_http::p3::add_to_linker(&mut linker)?;
```

- Custom labs skip the `wasmtime_wasi_http` line and link their own world
  instead (`p2-custom`: `labs/p2-custom-http/whost/src/main.rs:59-60`,
  `p3-custom`: `labs/p3-custom-http/whost/src/main.rs:170-173` with the
  same P3 async-model setup as above).
- `linker.allow_shadowing(true)` is called in p2, p3, and p3-custom
  (p2-custom does not need it). Nothing newly possible here: mandatory
  differences to run P3.

### 3b HTTP implementation and policy: self-written (4 / ~60 lines) vs linked + wiring

p2-custom: the whole HTTP layer in 4 lines (errors as values). See
`labs/p2-custom-http/whost/src/main.rs:31-36`:

```rust
impl http::Host for HostState {
    async fn get(&mut self, url: String) -> Result<String, String> {
        let resp = reqwest::get(url).await.map_err(|e| e.to_string())?;
        resp.text().await.map_err(|e| e.to_string())
    }
}
```

p2-custom has no policy: default client (no URL allowlist, no timeout,
default redirect policy, no body-size cap). Request failures are still
error values (no `unwrap()` on the request path), but any URL goes
through.

p3-custom: the same 4 lines grown into an explicit policy. See
`labs/p3-custom-http/whost/src/main.rs:88-145`, steps 1-6:

- `url::Url::parse` (no string matching), scheme allowlist, host
  allowlist (SSRF: `localhost`, `127.0.0.1` only)
- shared client (timeout 10s, redirect limit 5)
- `content-length` pre-check + actual-byte cap (1 MiB)
- every failure is a value; no `unwrap()` on the request path

Standard (p2/p3): no HTTP logic, only registration (§3a) plus view
plumbing. See `labs/p2-http/whost/src/main.rs:44-52` (same shape for p3):

```rust
impl WasiHttpView for HostState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http_ctx,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}
```

The sample hooks are empty (`hooks: [(); 0]`); policy on the standard
side depends on hooks provided by the host implementation, so it needs
its own design. In short: custom keeps HTTP visible and auditable in a
few dozen lines of your own code; standard keeps it out of your tree
entirely by trusting the runtime implementation.

### 3c Export call style: direct `Store` (p2) vs `run_concurrent` + `Accessor` (p3)

```rust
// p2 (labs/p2-http/whost/src/main.rs:102-104; p2-custom same shape)
// pass the Store directly
let result = bindings.call_webpage_inspector(&mut store, target_url).await?;
```

```rust
// p3 (labs/p3-http/whost/src/main.rs:98-104; p3-custom same shape)
// concurrent call through an Accessor (argument becomes String)
let result = store
    .run_concurrent(async |accessor| {
        bindings
            .call_webpage_inspector(accessor, target_url.to_string())
            .await
    })
    .await??;
```

The growth is the foundation for concurrent calls. Single-call results
are the same. Host-side fan-out (one `Store` per `spawn`) is possible in
all four labs and is demonstrated in the p2, p2-custom, and p3-custom
tests (p3-http currently has a single-call test only).

The host 6-step structure (`target_url` + `Host: Requesting URL` print +
instantiate + call + report) and the `test.rs` split are the same across
all four labs. Default `target_url` is `http://localhost:8080/test1.html`
everywhere. Custom hosts print the final dictionary dump (their export
returns no value); standard hosts print the returned string.

## 4. Decision matrix: which lab when

### 4a Portability (HTTP part): standard wins, custom is own-host only

- p2/p3 guests use only standard `wasi:http` for HTTP, so the HTTP part
  runs on other P2/P3 hosts (built-in Wasmtime hosts,
  `wasi:http/proxy`-style hosts, and so on).
- The p2-custom/p3-custom guests depend on this lab's own WIT
  (`http.get`), so they run only on this lab's host. Closed systems only.

### 4b Policy: custom centralizes it, standard delegates it

- Custom centralizes a minimal capability (`get(url)`) at one host choke
  point, so deny-by-default is easy and auditable; standard exposes full
  HTTP semantics to the guest, so policy must constrain a wider surface.
- But p2-custom itself has no policy (see §3b); p3-custom is
  the production shape of the same interface.
- Standard: guests can touch headers and status freely; policy depends on
  hooks provided by the host implementation and needs its own design.

### 4c Control: do you need full HTTP control in the guest?

- If "pass a URL, get a body" is enough, custom is the shortest option.
- Choose standard (p2/p3) when the guest itself must handle methods,
  headers, status codes, body streams, trailers, or error codes.

### 4d Concurrency: sequential everywhere, in-guest join only on P3

- Calling URLs one by one in a loop works in all four labs, and that is
  enough for most uses. In that case P3 has little positive reason: p2
  (stable, mature implementation) is the safe choice.
- **Concurrent GETs inside one guest call** (joining several `send()`
  calls) is P3-only. P2 guests round-trip synchronously, so parallelism
  must come from host-side fan-out, which all four labs support (§3c).
- Async composition in general (first-wins fetch with `select!`,
  multiplexed streaming, and so on) is P3-only.

### 4e Stability: P2 is stable, P3 is experimental

- Almost nothing differs in features against P2. The P3 weakness is
  operational: the wasmtime P3 implementation is experimental (no semver
  guarantee), and these labs also link the P2 runtime because the async
  ABI glue still imports e.g. `wasi:io/poll@0.2.x` (same setup as
  wasmtime's own P3 tests).

### 4f Picks

- Stable closed system, minimal guest: **p2-custom** (+ backported policy
  from p3-custom when needed).
- Closed system, auditable policy, P3 async: **p3-custom**. Gains over
  p2-custom: async custom calls plus the full policy checklist in clear
  numbered steps. Error handling is the same in both (errors as values).
- HTTP code that must run on generic runtimes: **p2** (stable) or **p3**
  (future, experimental).
- Guest code size: ~65 lines (p2) -> ~49 lines (p3); the polling
  boilerplate goes away. Host grows slightly (async model +
  `run_concurrent`/`Accessor`).
