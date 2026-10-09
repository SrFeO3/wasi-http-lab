use wasip3::http::client::send;
use wasip3::http::types::{Fields, Method, Request, Response, Scheme};
use wasip3::{wit_bindgen, wit_future};

use crate::mywasm::demo::kv_ops;

// WIT Bindings. The WIT `async func` annotation drives the async bindings,
// so no extra `async:` option is needed here.
wit_bindgen::generate!({
    path: "../wit",
    world: "mywasm-world",
});

struct MyGuest;

impl Guest for MyGuest {
    async fn webpage_inspector(url: String) -> String {
        // 1. Parse URL (same simple logic as the P2 guest).
        // Expected format: scheme://authority/path
        let (scheme_part, rest) = url.split_once("://").unwrap_or(("http", &url));
        let (authority, path_part) = rest
            .split_once('/')
            .map(|(a, p)| (a, format!("/{p}")))
            .unwrap_or((rest, "/".to_string()));

        let scheme = match scheme_part.to_lowercase().as_str() {
            "https" => Scheme::Https,
            _ => Scheme::Http,
        };

        // 2. Build an empty-body GET request.
        // The trailers sender is dropped immediately, so the trailers future
        // resolves to its default (`Ok(None)`), same as the `http-proxy.rs`
        // example in the `wasip3` crate.
        let (_trailers_tx, trailers_rx) = wit_future::new(|| Ok(None));
        let (request, transmit_rx) = Request::new(Fields::new(), None, trailers_rx, None);

        if request.set_method(&Method::Get).is_err()
            || request.set_scheme(Some(&scheme)).is_err()
            || request.set_authority(Some(authority)).is_err()
            || request.set_path_with_query(Some(&path_part)).is_err()
        {
            return "Failed to build request".to_string();
        }

        // 3. Send and await the response.
        let response: Response = match send(request).await {
            Ok(r) => r,
            Err(e) => return format!("HTTP Error: {e:?}"),
        };

        // 4. Read the full body, then confirm the trailers future.
        let (body_stream, body_done) = Response::consume_body(response, transmit_rx);
        let result_bytes = body_stream.collect().await;
        if let Err(e) = body_done.await {
            return format!("Body trailers error: {e:?}");
        }

        let result_str = format!("webpage-body-size is {}", result_bytes.len());
        kv_ops::kv_set("wasm-guest-exec-status", "success");
        kv_ops::kv_set("webpage-url", &url);
        kv_ops::kv_set("webpage-body-size", &result_str);

        // Interacts with a dictionary resource managed by the host.
        if let Some(greeting) = kv_ops::kv_get("host-greeting") {
            println!("wguest: received greeting: {greeting}");
        }

        result_str
    }
}

// Export Bindings
export!(MyGuest);
