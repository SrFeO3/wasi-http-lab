use wasi::http::types::{Method, Scheme, OutgoingRequest, Headers};
use wasi::http::outgoing_handler;
use wasi::io::streams::StreamError;
use wit_bindgen::generate;

use crate::mywasm::demo::kv_ops;

// WIT Bindings
generate!({
    path: "../wit",
    world: "mywasm-world",
});

struct MyGuest;

impl Guest for MyGuest {
    fn webpage_inspector(url: String) -> String {
        // 1. Parse URL (Simple parsing logic) (Expected format: scheme://authority/path)
        let (scheme_part, rest) = url.split_once("://").unwrap_or(("http", &url));
        let (authority, path_part) = rest.split_once('/').map(|(a, p)| (a, format!("/{}", p))).unwrap_or((rest, "/".to_string()));

        let scheme = match scheme_part.to_lowercase().as_str() {
            "https" => Scheme::Https,
            "http" | _ => Scheme::Http,
        };

        // 2. Build Outgoing Request
        let headers = Headers::new();
        let req = OutgoingRequest::new(headers);
        req.set_method(&Method::Get).unwrap();
        req.set_scheme(Some(&scheme)).unwrap();
        req.set_authority(Some(authority)).unwrap();
        req.set_path_with_query(Some(&path_part)).unwrap();

        // 3. Trigger Request Handler
        let future_resp = match outgoing_handler::handle(req, None) {
            Ok(f) => f,
            Err(e) => return format!("Failed to send request: {:?}", e),
        };
        
        // 4. Block on Response Future
        future_resp.subscribe().block();

        // 5. Process Response Metadata
        let resp = match future_resp.get() {
            Some(Ok(Ok(r))) => r,
            Some(Ok(Err(e))) => return format!("HTTP Error: {:?}", e),
            Some(Err(_)) => return "Response future failed".to_string(),
            None => return "Response not ready".to_string(),
        };

        // 6. Get Response Body Stream
        let body = match resp.consume() {
            Ok(b) => b,
            Err(_) => return "Failed to consume response body".to_string(),
        };
        let stream = body.stream().expect("Failed to get response stream");
        
        let mut result_bytes = Vec::new();

        // 7. Read Stream to Buffer
        loop {
            stream.subscribe().block();
            match stream.read(4096) {
                Ok(bytes) => {
                    result_bytes.extend_from_slice(&bytes);
                }
                Err(StreamError::Closed) => break,
                Err(e) => return format!("Stream error: {:?}", e),
            }
        }

        let result_str = format!("webpage-body-size is {}", result_bytes.len());
        kv_ops::kv_set("wasm-guest-exec-status", "success");
        kv_ops::kv_set("webpage-url", &url);
        kv_ops::kv_set("webpage-body-size", &result_str);

        // interacts with a dictionary resource managed by the host
        if let Some(greeting) = kv_ops::kv_get("host-greeting") {
            println!("wguest: received greeting: {greeting}");
        }

        result_str
    }
}

// Export Bindings
export!(MyGuest);