# wasi-p2-http-lab

Experimenting with the **standard** WASI Preview 2 (P2) HTTP interface for Wasm components.
(Comparison target: wasi-p2-custom-http-lab)

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