//! Native bootstrap bridge for the actual Desktop decoder source-parity checks.
use std::io::{self, BufRead};

fn main() {
    for line in io::stdin().lock().lines() {
        let input = line.unwrap();
        match pomodorough_core::dispatch_json("bootstrap.plan.v1", &input) {
            Ok(raw) => {
                let decoded: serde_json::Value = serde_json::from_str(&raw).unwrap();
                println!("{}", serde_json::json!({"raw": raw, "decoded": decoded}));
            }
            Err(error) => println!("{}", serde_json::json!({"error": error.to_string()})),
        }
    }
}
