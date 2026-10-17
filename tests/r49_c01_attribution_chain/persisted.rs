use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

fn next_path(kind: &str) -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = ROOT.get_or_init(|| {
        std::env::var_os("R49_C01_CHAIN_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                std::env::temp_dir().join(format!("r49-c01-chain-{}-{nanos}", std::process::id()))
            })
    });
    let thread = std::thread::current();
    let directory = root.join(thread.name().unwrap_or("unnamed"));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join(format!(
        "{:05}-{kind}.json",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(super) fn reopen(value: &Value) -> Value {
    let path = next_path("state");
    let raw = serde_json::to_string_pretty(value).unwrap();
    std::fs::write(&path, &raw).unwrap();
    let stored = std::fs::read_to_string(&path).unwrap();
    assert_eq!(stored, raw, "raw persisted production return");
    let decoded: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(&decoded, value, "complete persisted production return");
    decoded
}

pub(super) fn dispatch(operation: &str, input: &Value) -> Value {
    let path = next_path("request");
    let raw = serde_json::to_string_pretty(input).unwrap();
    std::fs::write(&path, &raw).unwrap();
    let stored = std::fs::read_to_string(&path).unwrap();
    assert_eq!(stored, raw, "raw persisted request");
    assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), *input);
    let output = invoke(operation, &stored);
    assert_eq!(
        invoke(operation, &stored),
        output,
        "complete raw repeated return"
    );
    let response_path = path.with_extension("response.json");
    std::fs::write(&response_path, &output).unwrap();
    let response = std::fs::read_to_string(&response_path).unwrap();
    assert_eq!(response, output, "raw persisted production response");
    std::fs::write(
        path.with_extension("receipt.json"),
        json!({"operation": operation, "input": stored, "response": response}).to_string(),
    )
    .unwrap();
    let envelope: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(envelope["ok"], true, "{envelope}");
    envelope["value"].clone()
}

fn invoke(operation: &str, raw: &str) -> String {
    let Ok(oracle) = std::env::var("R49_C01_CHAIN_BASELINE") else {
        return dispatch_envelope_json(operation, raw);
    };
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(oracle)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        json!({"operation": operation, "input": raw})
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}
