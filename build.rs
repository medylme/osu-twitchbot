use std::env;
use winresource::WindowsResource;

fn main() {
    if let Ok(contents) = std::fs::read_to_string(".env") {
        for line in contents.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if env::var(key).is_err() {
                    println!("cargo:rustc-env={}={}", key, value);
                }
            }
        }
    }

    if env::var("GITHUB_LATEST_RELEASE_URL").is_err() {
        println!("cargo:warning=GITHUB_LATEST_RELEASE_URL not set, auto-updater will be disabled");
    }

    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let _ = WindowsResource::new().set_icon("assets/icon.ico").compile();
    }

    println!("cargo:rerun-if-changed=.env");
}
