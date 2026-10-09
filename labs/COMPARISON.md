# WASI HTTP Labs Comparison: p2-custom vs p2 vs p3

All four labs run the same experiment: the host passes a URL, the guest
does an HTTP GET and returns the body size. They differ only in WHO runs
HTTP and THROUGH WHICH API.

| | p2-custom | p2 | p3 | p3-custom |
|---|---|---|---|---|
| Goal | Contrast with a custom interface | Standard `wasi:http` (P2) | Standard `wasi:http` (P3 async model) | Custom interface with host policy (P3) |
| Guest base (all labs) | WASM component (`cdylib` for `wasm32-wasip2`), WIT bindings via wit-bindgen 0.62 | Same | Same | Same |
| Host base (all labs) | wasmtime 49.0.2 + tokio 1.53.2, edition 2024 | Same | Same, plus `component-model-async` | Same, plus `component-model-async` |
| WASI version | P2 runtime | P2 (`wasi:http` 0.2.x) | P3 (`wasi:http` 0.3.x) | P3 runtime |
| HTTP API | Custom sync `http.get` | Standard, step by step | Standard, with await | Custom async `http.get` |
| HTTP call in guest | 1 line + error mapping | ~30 lines | ~12 lines | 1 line + error mapping |
| Guest code (approx) | ~19 lines | ~65 lines | ~49 lines | ~25 lines |
| Policy | None (sample) | Host implementation hooks | Host implementation hooks | Explicit allowlist policy in host |
| Portability | Own host only | Generic runtimes | Generic runtimes (future) | Own host only |
| Status | Stable | Stable | Experimental (no semver guarantee) | Experimental (no semver guarantee) |

See each lab's `README.md` for setup. This file only covers what is the
same and what differs.

## 1. Custom vs standard (p2-custom and p3-custom vs p2/p3)

### Portability: standard wins, custom is own-host only

- p2/p3 guests use only standard `wasi:http`, so they run on other P2/P3
  hosts (built-in Wasmtime hosts, `wasi:http/proxy`-style hosts, and so on).
- The p2-custom guest depends on this lab's own WIT (`http.get`), so it runs
  only on this lab's host. Closed systems only.

### Security: custom wins in structure, not in this sample

- The custom advantage is that **policy lives in one place: the host**.
  Allowed URLs, fixed headers, timeouts, and size limits are enforced by
  the host, and the guest gets no extra capability. Auditing is easy because
  the guest is ~19 lines and the policy surface is only the host side.
- But the p2-custom host (`reqwest::get(...).unwrap()`) has no policy at
  all, so it is not secure as-is. For untrusted guests you need at minimum
  a URL allowlist (SSRF protection), redirect limits, timeouts, and no
  `unwrap()`. The p3-custom lab implements exactly that checklist, so read
  `labs/p3-custom-http/whost/src/main.rs` as the production shape of the
  same minimal interface.
- On the standard side, policy depends on hooks provided by the host
  implementation (wasmtime and friends). Guests can touch headers and
  status freely, so the policy placement needs its own design.

### Do you need full HTTP control in the guest?

- If "pass a URL, get a body" is enough, custom is the shortest option.
  Choose standard (p2/p3) when the guest itself must handle methods,
  headers, status codes, body streams, trailers, or error codes.
- The shortness of p2-custom comes from moving HTTP complexity into the
  host. It compares "calling a custom abstraction", not "using a standard
  HTTP API". Keep that distinction in mind when reading the samples.

### Host HTTP code: custom implements it, standard links it

The guest-side shortness above has its counterpart on the host side.
Custom hosts contain real HTTP code; standard hosts only register the
wasmtime implementation plus view plumbing.

```rust
// p2-custom: the whole HTTP layer in 4 lines (errors as values)
impl http::Host for HostState {
    async fn get(&mut self, url: String) -> Result<String, String> {
        let resp = reqwest::get(url).await.map_err(|e| e.to_string())?;
        resp.text().await.map_err(|e| e.to_string())
    }
}
```

```rust
// p2: no HTTP logic, only registration (same shape for p3 with `::p3`)
wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
// plus the WasiHttpView impl wiring ctx/table/hooks (about 10 lines,
// identical in p2 and p3 apart from the module path).
```

```rust
// p3-custom: the same 4 lines grown into an explicit policy (about 40
// lines, numbered steps 1-6): url-crate parsing, scheme allowlist, host
// allowlist (SSRF), shared client (timeout 10s, redirect limit 5),
// content-length pre-check, actual-byte cap (1 MiB). Every failure is a
// value; there is no `unwrap()`. See
// labs/p3-custom-http/whost/src/main.rs for the full code.
```

In short: custom keeps HTTP visible and auditable in a few dozen lines
of your own code; standard keeps it out of your tree entirely by trusting
the runtime implementation.

## 2. p2 vs p3: P3 advantages

The biggest P3 advantage is **concurrency inside a single guest call**,
but that is not the only one.

### Sequential multi-fetch works on p2 and custom too

Calling URLs one by one in a loop works in all three labs, and that is
enough for most uses. In that case P3 has little positive reason: p2
(stable, mature implementation) is the safe choice. Choose P3 for
experiments, future readiness, or when you need one of the following.

### What P3 can do that P2 cannot

- **Concurrent GETs inside one guest call** (joining several `send()` calls).
  P2 guests can only round-trip synchronously, so parallelism must come
  from host-side fan-out (one Store per `spawn`), which all three labs support.
- Async composition in general (first-wins fetch with `select!`,
  multiplexed streaming, and so on).

### What P2 can do that P3 cannot

- Almost nothing in terms of features. The P3 weakness is operational:
  the wasmtime P3 implementation is experimental (no semver guarantee),
  and this lab also links the P2 runtime because the async ABI glue still
  imports e.g. `wasi:io/poll@0.2.x` (same setup as wasmtime's own P3 tests).

### Code size

- Guest: ~65 lines -> ~49 lines. The polling boilerplate
  (`subscribe().block()` -> `get()` -> `consume()` -> `read` loop) goes away
  and becomes `send().await` plus `collect().await`.
- Host: slightly larger (`wasm_component_model_async(true)` is mandatory,
  plus the `run_concurrent` + `Accessor` call style). The growth is the
  foundation for concurrent calls.

## 3. p2 -> p3 code differences: same/different, size, capability

Side-by-side comparison of matching parts (see `labs/p2-http` and
`labs/p3-http` for full files).

### (a) WIT world: almost the same, one word differs

```wit
// p2
export webpage-inspector: func(url: string) -> string;
// p3
export webpage-inspector: async func(url: string) -> string;
```

Only `async` is added. Not importing `wasi:http` into the WIT (leaving it
to the `wasip2` / `wasip3` crates) is the same in both.

### (b) Guest HTTP call: different, smaller, P3-only composition

```rust
// p2: polling in 4 steps (about 30 lines)
let future_resp = outgoing_handler::handle(req, None)...;
future_resp.subscribe().block();
let resp = match future_resp.get() { ... };
let stream = body.stream()...;
loop { stream.subscribe().block(); match stream.read(4096) { ... } }
```

```rust
// p3: send and collect (about 12 lines)
let response: Response = match send(request).await { ... };
let (body_stream, body_done) = Response::consume_body(response, transmit_rx);
let result_bytes = body_stream.collect().await;
```

URL parsing, kv exchange, and printing are the same in both.
New in P3 only: composing several `send()` calls with `join!` and friends
(this sample keeps a single GET so the three labs stay comparable).

### (c) Guest export definition: different (follows (a))

```rust
// p2
fn webpage_inspector(url: String) -> String { ... }
// p3
async fn webpage_inspector(url: String) -> String { ... }
```

### (d) Host engine/linker: different, slightly larger

```rust
// p2
config.wasm_component_model(true);
wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
wasmtime_wasi_http::p2::add_to_linker_async(&mut linker)?;
```

```rust
// p3
config.wasm_component_model_async(true); // mandatory for P3
wasmtime_wasi::p2::add_to_linker_async(&mut linker)?; // also linked for async ABI glue
wasmtime_wasi::p3::add_to_linker(&mut linker)?;       // sync registration (no async variant)
wasmtime_wasi_http::p3::add_to_linker(&mut linker)?;
```

Nothing newly possible here. Mandatory differences to run P3.

### (e) Export call: different, slightly larger

```rust
// p2: pass the Store directly
let result = bindings.call_webpage_inspector(&mut store, target_url).await?;
```

```rust
// p3: concurrent call through an Accessor (argument becomes String)
let result = store
    .run_concurrent(async |accessor| {
        bindings
            .call_webpage_inspector(accessor, target_url.to_string())
            .await
    })
    .await??;
```

The growth is the foundation for concurrent calls. Single-call results
are the same.

### (f) The same parts: kv exchange, URL, printing, test view

- The 3 kv keys (`wasm-guest-exec-status` / `webpage-url` /
  `webpage-body-size`) plus the `host-greeting` round trip are the same
  across all three labs.
- The host `target_url` variable plus `Host: Requesting URL` print, the
  6-step structure, and the `test.rs` split are also the same (custom only
  prints the dictionary dump because its export returns no value).

## 4. When is p3-custom best?

Best for closed systems that want the minimal guest AND an auditable
policy AND P3 async. For the same shape on stable P2, choose p2-custom.
For guests that must run on generic runtimes, choose p2/p3.

- Gains over p2-custom: async custom calls (one guest call could fire
  several with `join!`), errors as values instead of panics, and the full
  policy checklist (allowlist, scheme check, timeout, size cap, redirect
  limit) in clear numbered steps.
- Keeps from p2-custom: minimal guest, host-owned policy, closed system
  only (no portability beyond its own host).
- Costs over p2-custom: P3 instability (experimental, no semver
  guarantee). For a stable closed system today, p2-custom plus a
  backported policy is the calmer choice.
