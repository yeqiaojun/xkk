use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=XKK_PRO_VERSION");
    let raw = env::var("XKK_PRO_VERSION").unwrap_or_else(|_| "0".to_string());
    let version: i32 = raw
        .parse()
        .expect("XKK_PRO_VERSION must be a non-negative i32");
    assert!(version >= 0, "XKK_PRO_VERSION must be a non-negative i32");
    println!("cargo:rustc-env=XKK_PRO_VERSION={version}");
}
