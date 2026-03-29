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

fn get_credential(env_var: &str) -> Result<String, Box<dyn std::error::Error>> {
    // 1. Runtime env var (e.g. from systemd EnvironmentFile)
    if let Ok(val) = std::env::var(env_var) {
        return Ok(val);
    }
    // 2. File path env var (e.g. DISCORD_PTT_CLIENT_ID_FILE=/run/agenix/...)
    let file_var = format!("{env_var}_FILE");
    if let Ok(path) = std::env::var(&file_var) {
        return std::fs::read_to_string(&path)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("Failed to read {file_var} ({path}): {e}").into());
    }
    // 3. Compile-time env var (from .env at build time)
    let compile_time = match env_var {
        "DISCORD_PTT_CLIENT_ID" => option_env!("DISCORD_PTT_CLIENT_ID"),
        "DISCORD_PTT_CLIENT_SECRET" => option_env!("DISCORD_PTT_CLIENT_SECRET"),
        _ => None,
    };
    if let Some(val) = compile_time {
        return Ok(val.to_string());
    }
    Err(format!("{env_var} not set. Set it via env var, {env_var}_FILE, or .env at build time.").into())
}

fn connect_and_auth() -> Result<IpcConnection, Box<dyn std::error::Error>> {
    let client_id = get_credential("DISCORD_PTT_CLIENT_ID")?;
    let client_secret = get_credential("DISCORD_PTT_CLIENT_SECRET")?;

    let mut conn = IpcConnection::connect()?;
    auth::authenticate(&mut conn, &client_id, &client_secret)?;
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
