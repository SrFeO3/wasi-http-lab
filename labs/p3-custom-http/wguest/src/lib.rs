use crate::mycustomwasm::demo::dictionary_ops;
use crate::mycustomwasm::demo::http;

// WIT Bindings
wit_bindgen::generate!({
    path: "../wit",
    world: "mycustomwasm-world",
});

struct MyGuestCustom;

impl Guest for MyGuestCustom {
    async fn webpageinspector(url: String) {
        // 1. Fetch through the host-enforced policy (see whost). Errors are values, never traps.
        let body = match http::get(url).await {
            Ok(body) => body,
            Err(err) => {
                dictionary_ops::dict_set("wasm-guest-exec-status", &format!("error: {err}"));
                return;
            }
        };
        let len = body.len();

        // 2. Record success.
        dictionary_ops::dict_set("wasm-guest-exec-status", "success");
        dictionary_ops::dict_set("webpage-body-size", &len.to_string());

        // 3. Interacts with a dictionary resource managed by the host.
        if let Some(greeting) = dictionary_ops::dict_get("host-greeting") {
            println!("wguest: received greeting: {greeting}");
        }
    }
}

export!(MyGuestCustom);
