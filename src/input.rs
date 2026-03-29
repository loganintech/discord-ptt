use evdev::{Device, EventSummary, KeyCode};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonState {
    Pressed,
    Released,
}

pub struct InputListener {
    device: Device,
    button: KeyCode,
}

impl InputListener {
    pub fn open(path: &Path, button: KeyCode) -> io::Result<Self> {
        let device = Device::open(path)
            .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, format!(
                "Failed to open {}: {e} (are you in the 'input' group?)", path.display()
            )))?;

        let name = device.name().unwrap_or("Unknown");
        eprintln!("Opened device: {name}");

        if let Some(keys) = device.supported_keys() {
            if !keys.contains(button) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Device does not support {button:?}"),
                ));
            }
        }

        Ok(Self { device, button })
    }

    /// Blocks until the target button is pressed or released.
    /// Returns the new state.
    pub fn wait_for_event(&mut self) -> io::Result<ButtonState> {
        loop {
            let events = self.device.fetch_events()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            for event in events {
                if let EventSummary::Key(_, code, value) = event.destructure() {
                    if code == self.button {
                        match value {
                            1 => return Ok(ButtonState::Pressed),
                            0 => return Ok(ButtonState::Released),
                            _ => {} // ignore repeat (value=2)
                        }
                    }
                }
            }
        }
    }
}

pub fn list_devices() {
    let devices = evdev::enumerate().collect::<Vec<_>>();
    if devices.is_empty() {
        eprintln!("No input devices found. Are you in the 'input' group?");
        return;
    }
    println!("{:<30} {}", "PATH", "NAME");
    println!("{}", "-".repeat(70));
    for (path, device) in devices {
        let name = device.name().unwrap_or("Unknown");
        println!("{:<30} {name}", path.display());
    }
}

pub struct DetectedButton {
    pub path: PathBuf,
    pub device_name: String,
    pub button: KeyCode,
}

/// Resolve an eventN path to its stable /dev/input/by-id/ symlink if one exists.
fn resolve_stable_path(event_path: &Path) -> PathBuf {
    let by_id = Path::new("/dev/input/by-id");
    if let Ok(entries) = std::fs::read_dir(by_id) {
        for entry in entries.flatten() {
            if let Ok(target) = std::fs::canonicalize(entry.path()) {
                if let Ok(canonical_event) = std::fs::canonicalize(event_path) {
                    if target == canonical_event {
                        return entry.path();
                    }
                }
            }
        }
    }
    event_path.to_path_buf()
}

/// Opens all input devices and waits for a button press on any of them.
/// Ignores common noise like BTN_LEFT, BTN_RIGHT, and regular keys.
pub fn detect_button() -> io::Result<DetectedButton> {
    let devices: Vec<(PathBuf, Device)> = evdev::enumerate().collect();
    if devices.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No input devices found. Are you in the 'input' group?",
        ));
    }

    eprintln!("Press the button you want to use for push-to-talk...");

    // Grab non-blocking handles to all devices
    let mut devices: Vec<(PathBuf, Device)> = devices
        .into_iter()
        .filter_map(|(path, dev)| {
            // Only keep devices that support key events
            if dev.supported_keys().is_some() {
                dev.set_nonblocking(true).ok()?;
                Some((path, dev))
            } else {
                None
            }
        })
        .collect();

    // Codes to ignore — these fire constantly during normal use
    let ignore = [
        KeyCode::BTN_LEFT,
        KeyCode::BTN_RIGHT,
        KeyCode::BTN_TOUCH,
        KeyCode::BTN_TOOL_FINGER,
        KeyCode::BTN_TOOL_PEN,
    ];

    loop {
        for (path, device) in &mut devices {
            let events: Vec<_> = match device.fetch_events() {
                Ok(events) => events.collect(),
                Err(_) => continue,
            };
            for event in events {
                if let EventSummary::Key(_, code, 1) = event.destructure() {
                    if ignore.contains(&code) {
                        continue;
                    }
                    let device_name = device.name().unwrap_or("Unknown").to_string();
                    let stable_path = resolve_stable_path(path);
                    return Ok(DetectedButton {
                        path: stable_path,
                        device_name,
                        button: code,
                    });
                }
            }
        }
        // Small sleep to avoid busy-spinning
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

