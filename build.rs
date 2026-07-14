use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let clipper_dir = manifest_dir.join("vendor/clipper2/Clipper2Lib");
    let bridge_dir = manifest_dir.join("cpp");
    let version_header = clipper_dir.join("include/clipper2/clipper.version.h");

    println!(
        "cargo:rerun-if-changed={}",
        bridge_dir.join("clipper_bridge.cpp").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        bridge_dir.join("clipper_bridge.h").display()
    );
    println!("cargo:rerun-if-changed={}", version_header.display());
    println!(
        "cargo:rerun-if-changed={}",
        clipper_dir.join("src/clipper.engine.cpp").display()
    );

    cc::Build::new()
        .cpp(true)
        .define("USINGZ", None)
        .include(clipper_dir.join("include"))
        .include(&bridge_dir)
        .file(clipper_dir.join("src/clipper.engine.cpp"))
        .file(bridge_dir.join("clipper_bridge.cpp"))
        .flag_if_supported("/std:c++17")
        .flag_if_supported("-std=c++17")
        .compile("gerber_clipper2");
}
