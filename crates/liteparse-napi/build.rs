extern crate napi_build;

use std::env;

fn main() {
    napi_build::setup();

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Set rpath so the .node binary finds libpdfium next to itself at runtime.
    // In the npm package, libpdfium is bundled alongside the .node file.
    match target_os.as_str() {
        "macos" => {
            // @loader_path = directory containing the .node file
            println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path");
        }
        "linux" => {
            // $ORIGIN = directory containing the .node file
            println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        }
        _ => {
            // Windows: DLLs are found via PATH or same directory automatically
        }
    }

    // Also add the build-time pdfium path so local dev builds work without
    // copying libpdfium manually.
    if let Ok(lib_path) = env::var("DEP_PDFIUM_LIB_PATH") {
        if target_os == "macos" || target_os == "linux" {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_path}");
        }
    }
}
