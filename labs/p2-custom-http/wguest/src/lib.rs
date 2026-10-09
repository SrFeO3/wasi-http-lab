use crate::mycustomwasm::demo::http;

wit_bindgen::generate!({
    path: "../wit/",
    world: "mycustomwasm-world",
});

struct MyGuestCustom;

impl Guest for MyGuestCustom{
    fn webpageinspector(url: String) {
        use mycustomwasm::demo::dictionary_ops;

        let body = http::get(&url);
        
        //let display = { let d = body.replace(['\n', '\r'], ""); if d.len() > 42 { format!("{}...{}", &d[..20], &d[d.len()-20..]) } else { d } };
        //println!("wguest: http getbody: {display}");

        let len = body.len();

        dictionary_ops::dict_set("wasm-guest-exec-status", "success");
        dictionary_ops::dict_set("webpage-body-size", &len.to_string());

        // interacts with a dictionary resource managed by the host
        if let Some(greeting) = dictionary_ops::dict_get("host-greeting") {
            println!("wguest: received greeting: {greeting}");
        }
    }
}

export!(MyGuestCustom);