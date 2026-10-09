# wasi-p3-custom-http-lab

Experimenting with a **custom** HTTP interface on the WASI Preview 3 (P3)
runtime for Wasm components.
(Comparison targets: `labs/p2-custom-http`, `labs/p2-http`, `labs/p3-http`)
(Three-way comparison: `labs/COMPARISON.md`)

## Overview
Same experiment as the P2 custom lab: a host (`whost`) plus a guest
(`wguest`) component, and the guest fetches a URL through a hand-written
`http.get` interface, but the interface is `async` and the host enforces
an explicit security policy.
- Async WebAssembly with a minimal custom HTTP GET on the P3 runtime.
- The guest uses one `http.get` call; errors are values, never traps.
- The guest interacts with a dictionary resource managed by the host.
- The host policy (allowlist, timeout, size cap, redirect limit) is the
  point of this lab: p2-custom shows the minimal shape, this lab shows
  the production shape.

## Environment & Tools
- **Language:** Rust (guest target: `wasm32-wasip2`)
- **Key Crates:** `wasmtime 49.0.2`, `wasmtime-wasi 49.0.2`,
  `reqwest 0.13.5`, `url 2.5.8` (host-side)
- **Specification:** P3 runtime + **custom async WIT interface**
- **Status:** experimental: P3 in wasmtime is explicitly unstable
  (no semver guarantee) and the host additionally links WASI P2, because the
  async ABI glue still imports e.g. `wasi:io/poll@0.2.x`
  (same as wasmtime's own P3 tests).

## Security policy (host)

The p2-custom host (`reqwest::get(...).unwrap()`) has no policy: any URL
goes through, failures panic the task. This lab fixes that explicitly:

1. Parse with the `url` crate, never with string matching.
2. Allowlist the scheme (`http`, `https`).
3. Allowlist the host (SSRF protection: `localhost`, `127.0.0.1` only).
4. Fetch with a shared client enforcing timeout (10s) and redirect limit (5).
5. Reject oversized bodies early via `content-length`, then enforce the cap
   (1 MiB) on the actual bytes.
6. Every failure becomes a `Result::Err` string for the guest. No `unwrap()`.

## Setup & Execution

### 1. Start Mock Web Server
Same mock as the other labs (OpenResty provides delay/error patterns;
any static server works for the smoke test).
```bash
docker run --name demo-server -d -p 8080:80 \
  -v $(pwd)/configs/nginx.conf:/usr/local/openresty/nginx/conf/nginx.conf:ro \
  openresty/openresty:alpine
```

### 2. Build Wasm Guest
```bash
cd wguest
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release
```

### 3. Run Wasm Host & Tests
```bash
cd whost
cargo run
cargo test -- --nocapture
```

The policy tests (`test_blocked_*`) need no server: rejection happens
before any network access. The concurrency tests need the OpenResty mock
with `/sleep/N` delays, like the other labs.
