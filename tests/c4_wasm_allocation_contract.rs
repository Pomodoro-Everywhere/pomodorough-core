use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

#[test]
fn c4_release_wasm_rejects_oversized_allocations_without_trapping() {
    let target = temporary_target();
    let build = build_release_wasm(&target);
    assert!(build.success(), "C4 release WASM build failed");

    let artifact = target.join(wasm_artifact_path());
    let behavior = Command::new("node")
        .arg("tests/c4_wasm_allocation_contract.mjs")
        .arg(&artifact)
        .status()
        .expect("Node must host the C4 release WASM artifact");
    let cleanup = std::fs::remove_dir_all(&target);
    assert!(behavior.success(), "C4 WASM allocation contract failed");
    cleanup.expect("temporary C4 WASM target must be removable");
}

fn build_release_wasm(target: &Path) -> ExitStatus {
    let cargo = rustup_binary("cargo");
    Command::new(&cargo)
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--locked",
        ])
        .env("CARGO_TARGET_DIR", target)
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
        .expect("cargo must build the C4 release WASM artifact")
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
        "pomodorough-core-c4-wasm-contract-{}",
        std::process::id()
    ))
}

fn wasm_artifact_path() -> &'static Path {
    Path::new("wasm32-unknown-unknown/release/pomodorough_core.wasm")
}
