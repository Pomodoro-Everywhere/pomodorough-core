//! Native JSON bridge for production-source completion differential probes.
use std::io::{self, BufRead};

fn main() {
    for line in io::stdin().lock().lines() {
        let request: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let result = pomodorough_core::dispatch_json(
            request["operation"].as_str().unwrap(),
            &request["input"].to_string(),
        )
        .unwrap_or_else(|error| match error {
            pomodorough_core::CoreError::InvalidInput(message) => {
                serde_json::json!({"error":message}).to_string()
            }
            other => panic!("unexpected probe error: {other:?}"),
        });
        println!("{result}");
    }
}
