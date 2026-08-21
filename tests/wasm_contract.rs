use std::fs;

#[test]
fn wasm_abi_exposes_allocation_dispatch_and_free_contract() {
    let source = fs::read_to_string("src/wasm_abi.rs").expect("WASM ABI source must exist");
    for symbol in [
        "pomodorough_alloc",
        "pomodorough_free",
        "pomodorough_dispatch",
    ] {
        assert!(source.contains(symbol), "missing export {symbol}");
    }
    assert!(source.contains("dispatch_envelope_json"));

    let workflow = fs::read_to_string(".github/workflows/ci.yml").unwrap();
    assert!(workflow.contains("rustup target add wasm32-unknown-unknown"));
    assert!(workflow.contains("cargo +1.97.1 build --release --target wasm32-unknown-unknown"));
}
