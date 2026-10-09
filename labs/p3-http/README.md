# wasi-p3-http-lab

Experimenting with the **WASI Preview 3 (P3) HTTP** interface for Wasm components.
(Comparison targets: `labs/p2-http`, `labs/p2-custom-http`)

## Overview
Same shape as the P2 standard lab: a host (`whost`) plus a guest (`wguest`)
component, but running on the P3 async component model.
- The guest uses the `wasip3` crate (`wasi:http/client.send`) with `async`/`await`
  instead of P2's `subscribe().block()` polling loop.
- The guest interacts with the same `kv-ops` dictionary resource managed by the host.
- Includes the same URL-selection integration test as a starting point.

## Environment & Tools
- **Language:** Rust (guest target: `wasm32-wasip2`; the `wasip3` crate is
  designed for this target; there is no stable `wasm32-wasip3` std yet)
- **Key Crates:** `wasmtime 49.0.2`, `wasmtime-wasi 49.0.2`,
  `wasmtime-wasi-http 49.0.2` (P3 is default-on since 49.0.0),
  `wit-bindgen 0.62.0`, `wasip3 0.9.0`
- **Specification:** WASI P3 (`wasi:http@0.3.0`) + custom `kv-ops` WIT interface
- **Status:** experimental: P3 in wasmtime is explicitly unstable
  (no semver guarantee) and the host additionally links WASI P2, because the
  async ABI glue still imports e.g. `wasi:io/poll@0.2.x`
  (same as wasmtime's own P3 tests).

## P2 -> P3 differences
Moved to [`labs/COMPARISON.md`](../COMPARISON.md) (sections 2-3), which compares
all three labs side by side: what is the same, what differs, whether code
shrinks or grows, and what P3 newly enables.

## Setup & Execution

### 1. Start Mock Web Server
Same mock as the P2 labs (OpenResty provides delay/error patterns;
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
