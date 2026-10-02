//! Native JSON bridge for read-model production-source probes.
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
        let result = pomodorough_core::dispatch_json(&request.operation, request.input.get());
        match result {
            Ok(value) => println!("{value}"),
            Err(error) => println!("{}", serde_json::json!({"error": error.to_string()})),
        }
    }
}
