use crate::ipc::IpcConnection;
use std::io;
use std::time::{Duration, Instant};

pub fn run_benchmark(conn: &mut IpcConnection, iterations: u32) -> io::Result<Vec<Duration>> {
    let mut latencies = Vec::with_capacity(iterations as usize);

    // Warmup: one round-trip to prime the connection
    let _ = conn.send_command("GET_VOICE_SETTINGS", serde_json::json!({}))?;

    eprintln!("Running {iterations} iterations...\n");

    for i in 0..iterations {
        let mute = i % 2 == 0;
        let t0 = Instant::now();
        conn.send_command(
            "SET_VOICE_SETTINGS",
            serde_json::json!({ "mute": mute }),
        )?;
        let elapsed = t0.elapsed();
        latencies.push(elapsed);
    }

    Ok(latencies)
}

pub fn print_stats(latencies: &[Duration]) {
    if latencies.is_empty() {
        println!("No data.");
        return;
    }

    let mut sorted: Vec<Duration> = latencies.to_vec();
    sorted.sort();

    let n = sorted.len();
    let sum: Duration = sorted.iter().sum();
    let mean = sum / n as u32;

    let p50 = sorted[n * 50 / 100];
    let p95 = sorted[n * 95 / 100];
    let p99 = sorted[n.saturating_sub(1).min(n * 99 / 100)];

    let variance: f64 = sorted
        .iter()
        .map(|d| {
            let diff = d.as_secs_f64() - mean.as_secs_f64();
            diff * diff
        })
        .sum::<f64>()
        / n as f64;
    let stddev = Duration::from_secs_f64(variance.sqrt());

    println!("=== Discord IPC Latency Benchmark ===");
    println!("Iterations: {n}");
    println!("Min:    {:>10.3}ms", sorted[0].as_secs_f64() * 1000.0);
    println!("Max:    {:>10.3}ms", sorted[n - 1].as_secs_f64() * 1000.0);
    println!("Mean:   {:>10.3}ms", mean.as_secs_f64() * 1000.0);
    println!("Median: {:>10.3}ms", p50.as_secs_f64() * 1000.0);
    println!("p95:    {:>10.3}ms", p95.as_secs_f64() * 1000.0);
    println!("p99:    {:>10.3}ms", p99.as_secs_f64() * 1000.0);
    println!("StdDev: {:>10.3}ms", stddev.as_secs_f64() * 1000.0);
}
