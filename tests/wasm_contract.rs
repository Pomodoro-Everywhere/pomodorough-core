use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host() {
    let target = temporary_target();
    let cargo = rustup_binary("cargo");
    let build = Command::new(&cargo)
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--locked",
        ])
        .env("CARGO_TARGET_DIR", &target)
        // The nested WASM artifact is executed by Node, not merged into the host
        // coverage profile. Select the rustup-managed toolchain that owns the WASM
        // target and do not leak host instrumentation or compiler overrides.
        .env_remove("CARGO")
        .env("RUSTC", cargo.with_file_name("rustc"))
        .env("RUSTDOC", cargo.with_file_name("rustdoc"))
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .env_remove("LLVM_COV")
        .env_remove("LLVM_PROFDATA")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("__CARGO_LLVM_COV_RUSTC_WRAPPER")
        .env_remove("__CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS")
        .env_remove("__CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES")
        .env_remove("CARGO_LLVM_COV")
        .env_remove("CARGO_LLVM_COV_SHOW_ENV")
        .env_remove("CARGO_LLVM_COV_TARGET_DIR")
        .env_remove("CARGO_LLVM_COV_BUILD_DIR")
        .status()
        .expect("cargo must build the WASM contract artifact");
    assert!(build.success(), "WASM contract artifact build failed");

    let artifact = target.join(wasm_artifact_path());
    let behavior = Command::new("node")
        .arg("tests/wasm_abi_host.mjs")
        .arg(&artifact)
        .status()
        .expect("Node must host the WASM contract artifact");
    let cleanup = std::fs::remove_dir_all(&target);
    assert!(behavior.success(), "WASM ABI host behavior failed");
    cleanup.expect("temporary WASM target must be removable");
}

fn rustup_binary(name: &str) -> PathBuf {
    let output = Command::new("rustup")
        .args(["which", "--toolchain", "1.97.1", name])
        .output()
        .expect("rustup must locate the stable toolchain");
    assert!(output.status.success(), "rustup tool lookup failed");
    let path = String::from_utf8(output.stdout).expect("rustup path must be UTF-8");
    PathBuf::from(path.trim())
}

fn temporary_target() -> PathBuf {
    std::env::temp_dir().join(format!(
        "pomodorough-core-wasm-contract-{}",
        std::process::id()
    ))
}

fn wasm_artifact_path() -> &'static Path {
    Path::new("wasm32-unknown-unknown/release/pomodorough_core.wasm")
}
