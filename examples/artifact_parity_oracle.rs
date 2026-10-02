//! Native envelopes for the official release artifact parity gate.
use serde::Deserialize;
use std::io::{self, BufRead, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: String,
    // A string preserves malformed JSON, duplicate keys, and numeric tokens.
    input: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let envelope = pomodorough_core::dispatch_envelope_json(&request.operation, &request.input);
        writeln!(output, "{envelope}")?;
    }
    output.flush()?;
    Ok(())
}
