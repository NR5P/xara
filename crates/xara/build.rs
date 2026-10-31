use std::env;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    for c in ["pulse", "alsa", "coreaudio", "aaudio", "wasapi"] {
        println!("cargo::rustc-check-cfg=cfg({c})");
    }
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let linux = os == "linux";
    if linux && env::var_os("CARGO_FEATURE_PULSE").is_some() {
        println!("cargo::rustc-cfg=pulse");
    }
    if linux && env::var_os("CARGO_FEATURE_ALSA").is_some() {
        println!("cargo::rustc-cfg=alsa");
    }
    if os == "macos" || os == "ios" {
        println!("cargo::rustc-cfg=coreaudio");
    }
    if os == "android" {
        println!("cargo::rustc-cfg=aaudio");
    }
    if os == "windows" {
        println!("cargo::rustc-cfg=wasapi");
    }
}
