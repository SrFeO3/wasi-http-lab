# wasi-p2-http-lab

Experimenting with the **standard** WASI Preview 2 (P2) HTTP interface for Wasm components.
(Comparison target: wasi-p2-custom-http-lab)
(Three-way comparison: `labs/COMPARISON.md`)

## Overview
This project explores the behavior of the WASI Preview 2 HTTP using a host (`whost`) and a guest (`wguest`) component.
- Async WebAssembly testing utilizing WASI P2 and WASI P2 HTTP.
- The Wasm guest utilizes the host-provided WASI P2 HTTP interface for HTTP GET operations.
- The Wasm guest interacts with a dictionary resource managed by the host.
- Includes basic benchmarks for parallel execution.

## Environment & Tools
- **Language:** Rust (target: `wasm32-wasip2`)
- **Key Crates:** `wasmtime`, `wasmtime-wasi`, `wasmtime-wasi-http`
- **Specification:** WASI P2 + standard `wasi:http`

## Explicitly P2 (not P3)
Every WASI touchpoint in this lab names P2 on purpose:
- Host links `wasmtime_wasi::p2::add_to_linker_async` and
  `wasmtime_wasi_http::p2::add_to_linker_async`: the WASIp2 implementations.
  (The P3 counterparts in `::p3` are experimental per the official docs.)
- Guest depends on the [`wasip2`](https://crates.io/crates/wasip2) crate
  (WASI 0.2.x bindings), which the official `wasi` crate docs recommend
  "to explicitly indicate which version of the WASI standard you'd like to use".
- Build target is `wasm32-wasip2`.
- For the P3 equivalent, see `labs/p3-http` (uses `wasip3` + `::p3` linker).

## Setup & Execution

### 1. Start Mock Web Server
We use OpenResty (Nginx + Lua) to provide various response patterns (delays, errors).
```bash
docker run --name demo-server -d -p 8080:80 \
  -v $(pwd)/configs/nginx.conf:/usr/local/openresty/nginx/conf/nginx.conf:ro \
  openresty/openresty:alpine
```

### 2. Build Wasm Guest
Compile the guest component to the WASI P2 target.
```bash
cd wguest
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release
```

### 3. Run Wasm Host & Tests
Execute the host or run the concurrency/integration tests.
```bash
cd whost
cargo run
cargo test -- --nocapture

cargo run --example cache
cargo test --example cache -- --nocapture
```