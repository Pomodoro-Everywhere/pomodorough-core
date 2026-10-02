//! Native raw bootstrap bridge. Keep the returned JSON and independent decoding.
use serde::Deserialize;
use serde_json::value::RawValue;
use std::io::{self, BufRead};

#[derive(Deserialize)]
struct Request {
    operation: String,
    input: Box<RawValue>,
}

fn main() {
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line.unwrap()).unwrap();
        let input_raw = request.input.get();
        let input_decoded: serde_json::Value = serde_json::from_str(input_raw).unwrap();
        match pomodorough_core::dispatch_json(&request.operation, request.input.get()) {
            Ok(raw) => {
                let decoded: serde_json::Value = serde_json::from_str(&raw).unwrap();
                println!(
                    "{}",
                    serde_json::json!({"raw": raw, "decoded": decoded,
                    "inputRaw":input_raw, "inputDecoded":input_decoded})
                );
            }
            Err(error) => println!(
                "{}",
                serde_json::json!({"error": error.to_string(),
                "inputRaw":input_raw, "inputDecoded":input_decoded})
            ),
        }
    }
}
