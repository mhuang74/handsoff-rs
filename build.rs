fn main() {
    // Link to CoreGraphics and ApplicationServices frameworks on macOS
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }
}
