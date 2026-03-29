use serde_json::Value;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use uuid::Uuid;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Opcode {
    Handshake = 0,
    Frame = 1,
    Close = 2,
    Ping = 3,
    Pong = 4,
}

impl Opcode {
    fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Handshake),
            1 => Some(Self::Frame),
            2 => Some(Self::Close),
            3 => Some(Self::Ping),
            4 => Some(Self::Pong),
            _ => None,
        }
    }
}

pub struct IpcConnection {
    stream: UnixStream,
}

impl IpcConnection {
    pub fn connect() -> io::Result<Self> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .or_else(|_| std::env::var("TMPDIR"))
            .unwrap_or_else(|_| "/tmp".to_string());

        // Try discord-ipc-0 through discord-ipc-9, also check snap/flatpak paths
        let base_dirs = [
            PathBuf::from(&runtime_dir),
            PathBuf::from(&runtime_dir).join("app/com.discordapp.Discord"),
            PathBuf::from(&runtime_dir).join("snap.discord"),
        ];

        for base in &base_dirs {
            for i in 0..10 {
                let path = base.join(format!("discord-ipc-{i}"));
                match UnixStream::connect(&path) {
                    Ok(stream) => {
                        eprintln!("Connected to {}", path.display());
                        return Ok(Self { stream });
                    }
                    Err(_) => continue,
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Could not find Discord IPC socket. Is Discord running?",
        ))
    }

    pub fn send(&mut self, opcode: Opcode, payload: &Value) -> io::Result<()> {
        let data = serde_json::to_vec(payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut header = [0u8; 8];
        header[..4].copy_from_slice(&(opcode as u32).to_le_bytes());
        header[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
        self.stream.write_all(&header)?;
        self.stream.write_all(&data)?;
        self.stream.flush()
    }

    pub fn recv(&mut self) -> io::Result<(Opcode, Value)> {
        let mut header = [0u8; 8];
        self.stream.read_exact(&mut header)?;

        let op = u32::from_le_bytes(header[..4].try_into().unwrap());
        let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;

        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf)?;

        let opcode = Opcode::from_u32(op)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("Unknown opcode: {op}")))?;
        let value: Value = serde_json::from_slice(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok((opcode, value))
    }

    pub fn send_command(&mut self, cmd: &str, args: Value) -> io::Result<Value> {
        let nonce = Uuid::new_v4().to_string();
        let payload = serde_json::json!({
            "cmd": cmd,
            "nonce": nonce,
            "args": args,
        });
        self.send(Opcode::Frame, &payload)?;

        // Read responses until we get the one matching our nonce
        loop {
            let (opcode, response) = self.recv()?;
            if opcode == Opcode::Close {
                let msg = response["message"].as_str().unwrap_or("Connection closed by Discord");
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, msg));
            }
            // Match on nonce to skip event dispatches
            if response.get("nonce").and_then(|n| n.as_str()) == Some(&nonce) {
                // Check for RPC errors
                if let Some(evt) = response.get("evt").and_then(|e| e.as_str()) {
                    if evt == "ERROR" {
                        let msg = response["data"]["message"]
                            .as_str()
                            .unwrap_or("Unknown RPC error");
                        return Err(io::Error::new(io::ErrorKind::Other, format!("RPC error: {msg}")));
                    }
                }
                return Ok(response);
            }
        }
    }
}
