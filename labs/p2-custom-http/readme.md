# wasi-p2-custom-http-lab

Experimenting with a **custom** HTTP interface built on WASI Preview 2 (P2) for Wasm components.
(Comparison target: wasi-p2-http-lab)

## Overview
This project explores the behavior of a custom HTTP interface built on top of WASI Preview 2 using a host (`whost`) and a guest (`wguest`) component.
- Async WebAssembly testing utilizing WASI P2 with a simplified custom HTTP GET implementation.
- The Wasm guest utilizes a custom host-provided HTTP interface for GET operations.
- The Wasm guest interacts with a dictionary resource managed by the host.
- Includes basic benchmarks for parallel execution.

## Environment & Tools
- **Language:** Rust (target: `wasm32-wasip2`)
- **Key Crates:** `wasmtime`, `wasmtime-wasi`, `reqwest` (host-side)
- **Specification:** WASI P2 + **custom WIT interface**

## HTTP GET Implementation Overview

### Wasm host (`whost`)
```
#[async_trait::async_trait]
impl http::Host for HostState {
    async fn get(&mut self, url: String) -> String {
        reqwest::get(url).await.unwrap().text().await.unwrap()
    }
}
```

### Wasm guest (`wguest`)

```
let body = http::get("https://example.com");
```

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
```
