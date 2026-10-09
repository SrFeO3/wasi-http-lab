# wasi-http-lab
WASI HTTP experiments: P2 standard vs P2 custom vs P3 vs P3 custom.

- labs/p2-http: standard wasi:http (wasmtime 49.0.2, P2)
- labs/p2-custom-http: custom WIT + reqwest host (wasmtime 49.0.2, P2)
- labs/p3-http: standard wasi:http on the P3 async component model
  (wasmtime 49.0.2, `wasip3` guest; experimental)
- labs/p3-custom-http: custom async WIT + policy-enforcing reqwest host
  (wasmtime 49.0.2, P3; experimental)

Each lab has its own `README.md` with setup & run instructions.
See [`labs/COMPARISON.md`](labs/COMPARISON.md) for the three-way comparison
(custom vs standard, P2 vs P3).
