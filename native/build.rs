fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") { return; }
    let library=pkg_config::Config::new().probe("gstreamer-app-1.0").unwrap();
    let mut build=cc::Build::new();build.file("src/capture-mac.m").flag("-fobjc-arc").flag("-fblocks").flag("-mmacosx-version-min=13.0");
    for path in library.include_paths {build.include(path);}
    build.compile("pulse_capture_mac");
    for name in ["ScreenCaptureKit","CoreMedia","CoreVideo","Foundation","AppKit"] {println!("cargo:rustc-link-lib=framework={name}");}
    println!("cargo:rerun-if-changed=src/capture-mac.m");
}
