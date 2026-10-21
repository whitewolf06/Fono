//! Read one bounded BenchRequest JSON from stdin; write only a timing/event trace.
use std::io::Read;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() != 1 {
        return Err(
            "wake_bench reads BenchRequest JSON on stdin; use tools/wake-bench/bench.py".into(),
        );
    }
    let mut bytes = Vec::new();
    std::io::stdin().take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err("bench request exceeds 64 KiB".into());
    }
    let request: fono_wake::bench::BenchRequest = serde_json::from_slice(&bytes)?;
    println!(
        "{}",
        serde_json::to_string(&fono_wake::bench::run(&request)?)?
    );
    Ok(())
}
