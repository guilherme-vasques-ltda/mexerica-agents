fn main() {
    println!("cargo:rerun-if-changed=src/screen/fork_mac_xpc.m");
    #[cfg(target_os = "macos")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new().file("src/screen/fork_mac_xpc.m").flag("-fobjc-arc").compile("mexerica_screen_xpc");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=objc");
    }
}
