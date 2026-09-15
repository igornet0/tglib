//! Locate and link `tdjson` when the `native` feature is enabled.
//!
//! Search order:
//! 1. `TDLIB_LIB_DIR` / `TDLIB_DIR`
//! 2. `pkg-config tdjson`
//! 3. Homebrew (`/opt/homebrew`, `/usr/local`) — macOS ARM64 first
//! 4. Common Unix prefixes
//!
//! Default/mock builds do not run this linker path.

fn main() {
    if std::env::var("CARGO_FEATURE_NATIVE").is_err() {
        return;
    }

    println!("cargo:rerun-if-env-changed=TDLIB_DIR");
    println!("cargo:rerun-if-env-changed=TDLIB_LIB_DIR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    match find_tdjson_lib_dir() {
        Some(lib_dir) => {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
            println!("cargo:rustc-link-lib=dylib=tdjson");
            if cfg!(target_os = "macos") {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
            } else if cfg!(target_os = "linux") {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
            }
            println!("cargo:warning=linking tdjson from {}", lib_dir.display());
        }
        None => {
            eprintln!(
                "\ntglib-tdlib: native tdjson not found.\n\n\
                 Install TDLib and rebuild with `--features native`.\n\n\
                 macOS (ARM64 / Intel):\n\
                   brew install tdlib\n\n\
                 Or build TDLib from source and set:\n\
                   export TDLIB_DIR=/path/to/tdlib/prefix\n\
                   export TDLIB_LIB_DIR=$TDLIB_DIR/lib\n\n\
                 pkg-config should then report:\n\
                   pkg-config --libs tdjson\n"
            );
            std::process::exit(1);
        }
    }
}

fn find_tdjson_lib_dir() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("TDLIB_LIB_DIR") {
        let p = std::path::PathBuf::from(dir);
        if lib_exists(&p) {
            return Some(p);
        }
    }
    if let Ok(dir) = std::env::var("TDLIB_DIR") {
        let p = std::path::PathBuf::from(dir).join("lib");
        if lib_exists(&p) {
            return Some(p);
        }
    }
    if let Some(dir) = pkg_config_libdir() {
        if lib_exists(&dir) {
            return Some(dir);
        }
    }
    const CANDIDATES: &[&str] = &[
        "/opt/homebrew/opt/tdlib/lib",
        "/opt/homebrew/lib",
        "/usr/local/opt/tdlib/lib",
        "/usr/local/lib",
        "/usr/lib",
        "/usr/lib64",
    ];
    for cand in CANDIDATES {
        let p = std::path::PathBuf::from(cand);
        if lib_exists(&p) {
            return Some(p);
        }
    }
    None
}

fn lib_exists(dir: &std::path::Path) -> bool {
    dir.join("libtdjson.dylib").exists()
        || dir.join("libtdjson.so").exists()
        || dir.join("tdjson.dll").exists()
        || dir.join("libtdjson.1.8.0.dylib").exists()
}

fn pkg_config_libdir() -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("pkg-config")
        .args(["--variable=libdir", "tdjson"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let dir = String::from_utf8(output.stdout).ok()?;
    let dir = dir.trim();
    if dir.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(dir))
    }
}
