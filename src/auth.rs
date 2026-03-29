use crate::ipc::{IpcConnection, Opcode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

const TOKEN_CACHE_PATH: &str = "token_cache.json";
const REDIRECT_URI: &str = "http://localhost";

#[derive(Serialize, Deserialize)]
struct TokenCache {
    access_token: String,
}

pub fn authenticate(conn: &mut IpcConnection, client_id: &str, client_secret: &str) -> io::Result<()> {
    // Step 1: Handshake
    eprintln!("Sending handshake...");
    conn.send(
        Opcode::Handshake,
        &serde_json::json!({
            "v": 1,
            "client_id": client_id,
        }),
    )?;
    let (_op, ready) = conn.recv()?;
    eprintln!("Handshake complete: {}", ready["evt"].as_str().unwrap_or("?"));

    // Step 2: Try cached token
    if let Some(token) = load_cached_token() {
        eprintln!("Found cached token, attempting authentication...");
        match try_authenticate(conn, &token) {
            Ok(()) => {
                eprintln!("Authenticated with cached token.");
                return Ok(());
            }
            Err(e) => {
                eprintln!("Cached token failed ({e}), starting fresh auth flow...");
                let _ = fs::remove_file(TOKEN_CACHE_PATH);
            }
        }
    }

    // Step 3: Authorize (opens modal in Discord)
    eprintln!("Requesting authorization... (check Discord for the approval popup)");
    let auth_response = conn.send_command(
        "AUTHORIZE",
        serde_json::json!({
            "client_id": client_id,
            "scopes": ["rpc", "rpc.voice.read", "rpc.voice.write"],
        }),
    )?;

    let code = auth_response["data"]["code"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "No auth code in response"))?;
    eprintln!("Got authorization code.");

    // Step 4: Exchange code for access token
    eprintln!("Exchanging code for access token...");
    let token = exchange_token(client_id, client_secret, code)?;

    // Step 5: Cache it
    let cache = TokenCache {
        access_token: token.clone(),
    };
    let cache_json = serde_json::to_string_pretty(&cache)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(TOKEN_CACHE_PATH, cache_json)?;

    // Step 6: Authenticate
    try_authenticate(conn, &token)?;
    eprintln!("Authenticated successfully.");
    Ok(())
}

fn try_authenticate(conn: &mut IpcConnection, token: &str) -> io::Result<()> {
    conn.send_command(
        "AUTHENTICATE",
        serde_json::json!({
            "access_token": token,
        }),
    )?;
    Ok(())
}

fn load_cached_token() -> Option<String> {
    let path = Path::new(TOKEN_CACHE_PATH);
    let data = fs::read_to_string(path).ok()?;
    let cache: TokenCache = serde_json::from_str(&data).ok()?;
    Some(cache.access_token)
}

fn exchange_token(client_id: &str, client_secret: &str, code: &str) -> io::Result<String> {
    let form_body = format!(
        "grant_type=authorization_code&code={code}&client_id={client_id}&client_secret={client_secret}&redirect_uri={REDIRECT_URI}"
    );

    let response_str = ureq::post("https://discord.com/api/oauth2/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form_body.as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Token exchange failed: {e}")))?
        .body_mut()
        .read_to_string()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to read token response: {e}")))?;

    let body: serde_json::Value = serde_json::from_str(&response_str)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to parse token response: {e}")))?;

    body["access_token"]
        .as_str()
        .map(|s: &str| s.to_string())
        .ok_or_else(|| {
            let err_desc = body["error_description"]
                .as_str()
                .or(body["error"].as_str())
                .unwrap_or("unknown error");
            io::Error::new(
                io::ErrorKind::Other,
                format!("Token exchange error: {err_desc}"),
            )
        })
}
