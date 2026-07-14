use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let kicad_root = manifest_dir
        .parent()
        .expect("gerber-parse must live directly under the KiCad source tree");
    let clipper_dir = kicad_root.join("thirdparty/clipper2/Clipper2Lib");
    let version_header = clipper_dir.join("include/clipper2/clipper.version.h");

    println!("cargo:rerun-if-changed=cpp/clipper_bridge.cpp");
    println!("cargo:rerun-if-changed=cpp/clipper_bridge.h");
    println!("cargo:rerun-if-changed={}", version_header.display());
    println!(
        "cargo:rerun-if-changed={}",
        clipper_dir.join("src/clipper.engine.cpp").display()
    );

    cc::Build::new()
        .cpp(true)
        .define("USINGZ", None)
        .include(clipper_dir.join("include"))
        .include(manifest_dir.join("cpp"))
        .file(clipper_dir.join("src/clipper.engine.cpp"))
        .file(manifest_dir.join("cpp/clipper_bridge.cpp"))
        .flag_if_supported("/std:c++17")
        .flag_if_supported("-std=c++17")
        .compile("gerber_clipper2");
}
