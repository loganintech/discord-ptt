mod auth;
mod bench;
mod config;
mod input;
mod ipc;

use evdev::KeyCode;
use input::{ButtonState, InputListener};
use ipc::IpcConnection;
use std::path::Path;
use std::process::Command;

const SERVICE_NAME: &str = "discord-ptt";

fn usage() {
    eprintln!("Usage:");
    eprintln!("  discord-ptt                           Run PTT in foreground");
    eprintln!("  discord-ptt toggle                    Start or stop PTT service");
    eprintln!("  discord-ptt setup                     Re-run button detection setup");
    eprintln!("  discord-ptt bench [iterations]        Benchmark IPC latency");
    eprintln!("  discord-ptt list-devices              List available input devices");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str());

    match cmd {
        Some("toggle") => run_toggle(),
        Some("setup") => {
            run_setup()?;
            Ok(())
        }
        Some("bench") => run_bench(&args[2..]),
        Some("list-devices") => {
            input::list_devices();
            Ok(())
        }
        Some("help" | "--help" | "-h") => {
            usage();
            Ok(())
        }
        None => {
            let cfg = match config::load() {
                Some(cfg) => {
                    eprintln!(
                        "Using saved config: {} on {}",
                        cfg.button_name, cfg.device_path
                    );
                    cfg
                }
                None => {
                    eprintln!("No config found, starting first-time setup.\n");
                    run_setup()?
                }
            };
            run_ptt_with_config(&cfg)
        }
        Some(other) => {
            eprintln!("Unknown command: {other}\n");
            usage();
            std::process::exit(1);
        }
    }
}

fn connect_and_auth() -> Result<IpcConnection, Box<dyn std::error::Error>> {
    let client_id =
        option_env!("CLIENT_ID").ok_or("CLIENT_ID not set. Add it to .env and rebuild.")?;
    let client_secret =
        option_env!("CLIENT_SECRET").ok_or("CLIENT_SECRET not set. Add it to .env and rebuild.")?;

    let mut conn = IpcConnection::connect()?;
    auth::authenticate(&mut conn, client_id, client_secret)?;
    Ok(conn)
}

fn is_service_active() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", SERVICE_NAME])
        .status()
        .is_ok_and(|s| s.success())
}

fn run_toggle() -> Result<(), Box<dyn std::error::Error>> {
    if is_service_active() {
        let status = Command::new("systemctl")
            .args(["--user", "stop", SERVICE_NAME])
            .status()?;
        if status.success() {
            eprintln!("PTT stopped");
        } else {
            return Err("Failed to stop service".into());
        }
    } else {
        let status = Command::new("systemctl")
            .args(["--user", "start", SERVICE_NAME])
            .status()?;
        if status.success() {
            eprintln!("PTT started");
        } else {
            return Err("Failed to start service. Is the discord-ptt service installed?".into());
        }
    }
    Ok(())
}

fn run_setup() -> Result<config::Config, Box<dyn std::error::Error>> {
    let detected = input::detect_button()?;
    let button_name = format!("{:?}", detected.button);

    eprintln!(
        "\nDetected: {button_name} on \"{}\" ({})",
        detected.device_name,
        detected.path.display()
    );

    let cfg = config::Config {
        device_path: detected.path.to_string_lossy().into_owned(),
        button_code: detected.button.0,
        button_name,
    };

    config::save(&cfg)?;
    eprintln!("Config saved. Run 'discord-ptt' to start.\n");
    Ok(cfg)
}

fn run_bench(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let iterations: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(100);

    let mut conn = connect_and_auth()?;

    let original = conn.send_command("GET_VOICE_SETTINGS", serde_json::json!({}))?;
    let was_muted = original["data"]["mute"].as_bool().unwrap_or(false);

    let latencies = bench::run_benchmark(&mut conn, iterations)?;

    let _ = conn.send_command(
        "SET_VOICE_SETTINGS",
        serde_json::json!({ "mute": was_muted }),
    );

    bench::print_stats(&latencies);
    Ok(())
}

fn run_ptt_with_config(cfg: &config::Config) -> Result<(), Box<dyn std::error::Error>> {
    let button = KeyCode(cfg.button_code);

    let mut conn = connect_and_auth()?;
    let mut listener = InputListener::open(Path::new(&cfg.device_path), button)?;

    conn.send_command("SET_VOICE_SETTINGS", serde_json::json!({ "mute": true }))?;

    eprintln!(
        "PTT active — hold {} to talk, Ctrl+C to quit",
        cfg.button_name
    );

    loop {
        match listener.wait_for_event()? {
            ButtonState::Pressed => {
                conn.send_command(
                    "SET_VOICE_SETTINGS",
                    serde_json::json!({ "mute": false }),
                )?;
            }
            ButtonState::Released => {
                conn.send_command(
                    "SET_VOICE_SETTINGS",
                    serde_json::json!({ "mute": true }),
                )?;
            }
        }
    }
}
