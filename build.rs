use std::env;
use std::path::{Path, PathBuf};

fn find_rawlib_include(target: &str) -> Option<PathBuf> {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".cargo")))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))?;
    let registry_sources = cargo_home.join("registry").join("src");
    let layout = if target.contains("msvc") {
        "msvc"
    } else {
        "gnu"
    };
    for registry in std::fs::read_dir(registry_sources).ok()?.flatten() {
        let source = registry
            .path()
            .join("rawlib-0.7.1")
            .join("libraw")
            .join(layout);
        if source.join("libraw").join("libraw.h").is_file() {
            return Some(source);
        }
    }
    None
}

fn build_raw_mosaic_shim() {
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("apple-darwin") {
        return;
    }
    let include = find_rawlib_include(&target).unwrap_or_else(|| {
        panic!("rawlib 0.7.1 LibRaw headers were not found in the Cargo registry")
    });
    cc::Build::new()
        .cpp(true)
        .file(Path::new("src").join("raw_mosaic_shim.cpp"))
        .include(include)
        .warnings(true)
        .compile("nexfilm_raw_mosaic_shim");
    println!("cargo:rerun-if-changed=src/raw_mosaic_shim.cpp");
}

fn main() {
    build_raw_mosaic_shim();
    tauri_build::build()
}
