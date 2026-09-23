fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("failed to embed the app icon");
    }

    // The Supabase project URL and publishable key come from .env (never committed) and are baked
    // into the binary; src/supabase.rs reads them with option_env!. Without them, accounts are off.
    println!("cargo:rerun-if-changed=.env");
    for key in ["SUPABASE_URL", "SUPABASE_PUBLISHABLE_KEY"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let env = std::fs::read_to_string(".env").unwrap_or_default();
    for (key, value) in env.lines().filter_map(|line| line.split_once('=')) {
        let key = key.trim();
        if matches!(key, "SUPABASE_URL" | "SUPABASE_PUBLISHABLE_KEY") && std::env::var_os(key).is_none() {
            println!("cargo:rustc-env={key}={}", value.trim());
        }
    }
}
