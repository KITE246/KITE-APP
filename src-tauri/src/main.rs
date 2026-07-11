#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::{Emitter, Manager, State};

#[derive(Deserialize)]
struct ProxyRequest {
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
}

#[derive(Serialize)]
struct ProxyResponse {
    status: u16,
    body: String,
    headers: HashMap<String, String>,
}

#[tauri::command]
async fn http_proxy(req: ProxyRequest) -> Result<ProxyResponse, String> {
    let client = reqwest::Client::new();
    let method = match req.method.as_deref().unwrap_or("GET") {
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        _ => reqwest::Method::GET,
    };

    let mut builder = client.request(method, &req.url);

    if let Some(headers) = req.headers {
        for (k, v) in headers {
            builder = builder.header(&k, &v);
        }
    }

    if let Some(body) = req.body {
        builder = builder.body(body);
    }

    let resp = builder.send().await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();

    let mut headers = HashMap::new();
    for (k, v) in resp.headers() {
        if let Ok(val) = v.to_str() {
            headers.insert(k.to_string(), val.to_string());
        }
    }

    let body = resp.text().await.map_err(|e| e.to_string())?;

    Ok(ProxyResponse {
        status,
        body,
        headers,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroqTranscribeReq {
    audio_b64: String,
    api_key: String,
    model: Option<String>,
    filename: Option<String>,
}

/// Multipart audio upload to Groq's OpenAI-compatible Whisper endpoint. Done in
/// Rust so we send real binary multipart (the JS CORS proxy stringifies bodies)
/// and avoid browser CORS. Returns Groq's raw JSON body ({"text": "..."}).
#[tauri::command]
async fn groq_transcribe(req: GroqTranscribeReq) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.audio_b64.trim())
        .map_err(|e| format!("bad audio encoding: {}", e))?;
    let model = req
        .model
        .unwrap_or_else(|| "whisper-large-v3-turbo".to_string());
    let filename = req.filename.unwrap_or_else(|| "recording.wav".to_string());

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", model)
        .text("response_format", "json")
        .text("temperature", "0");

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.groq.com/openai/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", req.api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("network error: {}", e))?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("groq http {}: {}", status.as_u16(), body));
    }
    Ok(body)
}

/// GET a URL and return its bytes as base64 — used for cloud TTS audio (the
/// http_proxy command returns text, which corrupts binary MP3 data).
#[tauri::command]
async fn tts_fetch(url: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("network error: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("tts http {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchBytesReq {
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
}

/// Generic request that returns the response body as "content-type\n<base64>".
/// Used for authenticated binary responses (HF image generation) that can't go
/// through http_proxy (which stringifies) or native fetch (CORS/auth).
#[tauri::command]
async fn fetch_bytes(req: FetchBytesReq) -> Result<String, String> {
    let client = reqwest::Client::new();
    let method = match req.method.as_deref().unwrap_or("GET") {
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        _ => reqwest::Method::GET,
    };
    let mut builder = client.request(method, &req.url).timeout(std::time::Duration::from_secs(120));
    if let Some(headers) = req.headers {
        for (k, v) in headers {
            builder = builder.header(&k, &v);
        }
    }
    if let Some(body) = req.body {
        builder = builder.body(body);
    }
    let resp = builder.send().await.map_err(|e| format!("network error: {}", e))?;
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let txt: String = String::from_utf8_lossy(&bytes).chars().take(200).collect();
        return Err(format!("http {}: {}", status.as_u16(), txt));
    }
    Ok(format!(
        "{}\n{}",
        ct,
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

#[tauri::command]
fn open_mic_settings() -> Result<(), String> {
    Command::new("cmd")
        .args(["/C", "start", "ms-settings:privacy-microphone"])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ─────────────────────────── TERMINAL (ConPTY) ───────────────────────────
// Real shell sessions running inside pseudo terminals. Multiple sessions are
// kept in a registry keyed by id so the frontend can drive several tabs and
// split panes at once. Output streams to xterm.js as base64 `term-output`
// events; keystrokes come back via `term_write`.

/// One live PTY session. `master` is kept for resize; `writer` feeds stdin;
/// `child` is held so we can kill it on close.
struct TermSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
struct TermState {
    sessions: Mutex<HashMap<u32, TermSession>>,
    next_id: AtomicU32,
}

#[derive(Clone, Serialize)]
struct TermOutput {
    id: u32,
    data: String,
}

#[derive(Serialize)]
struct ShellInfo {
    id: String,
    label: String,
    cmd: String,
    args: Vec<String>,
}

/// Enumerate the shells available on this machine for the quick-launch menu.
/// PowerShell and CMD are always present on Windows; Git Bash and WSL are
/// included only when found on disk.
#[tauri::command]
fn term_shells() -> Vec<ShellInfo> {
    let mut shells = vec![
        ShellInfo {
            id: "powershell".into(),
            label: "PowerShell".into(),
            cmd: "powershell.exe".into(),
            args: vec![],
        },
        ShellInfo {
            id: "cmd".into(),
            label: "CMD".into(),
            cmd: "cmd.exe".into(),
            args: vec![],
        },
    ];

    let mut git_candidates: Vec<String> = vec![
        "C:\\Program Files\\Git\\bin\\bash.exe".into(),
        "C:\\Program Files (x86)\\Git\\bin\\bash.exe".into(),
    ];
    if let Ok(lad) = std::env::var("LOCALAPPDATA") {
        git_candidates.push(format!("{}\\Programs\\Git\\bin\\bash.exe", lad));
    }
    for p in git_candidates {
        if std::path::Path::new(&p).exists() {
            shells.push(ShellInfo {
                id: "gitbash".into(),
                label: "Git Bash".into(),
                cmd: p,
                args: vec!["-i".into(), "-l".into()],
            });
            break;
        }
    }

    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    if std::path::Path::new(&format!("{}\\System32\\wsl.exe", windir)).exists() {
        shells.push(ShellInfo {
            id: "wsl".into(),
            label: "WSL".into(),
            cmd: "wsl.exe".into(),
            args: vec![],
        });
    }

    shells
}

/// Spawn a shell inside a new PTY sized `rows`x`cols` and return its session
/// id. A background thread pumps PTY output to the `term-output` event and
/// announces death via `term-exit`.
#[tauri::command]
fn term_open(
    app: tauri::AppHandle,
    state: State<TermState>,
    rows: u16,
    cols: u16,
    shell: Option<String>,
    args: Option<Vec<String>>,
) -> Result<u32, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let sh = shell.unwrap_or_else(|| {
        if cfg!(windows) {
            "powershell.exe".into()
        } else {
            "bash".into()
        }
    });
    let mut cmd = CommandBuilder::new(sh);
    if let Some(a) = args {
        cmd.args(a);
    }
    if let Ok(home) = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        cmd.cwd(home);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| e.to_string())?;
    // Drop the slave so the reader sees EOF when the child exits.
    drop(pair.slave);

    let id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    let app2 = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    if app2
                        .emit("term-output", TermOutput { id, data: b64 })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        // Session is gone — drop our bookkeeping and tell the UI.
        if let Some(st) = app2.try_state::<TermState>() {
            if let Ok(mut map) = st.sessions.lock() {
                map.remove(&id);
            }
        }
        let _ = app2.emit("term-exit", id);
    });

    state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .insert(
            id,
            TermSession {
                master: pair.master,
                writer,
                child,
            },
        );
    Ok(id)
}

/// Write user input (keystrokes / paste) to a session's stdin.
#[tauri::command]
fn term_write(state: State<TermState>, id: u32, data: String) -> Result<(), String> {
    let mut map = state.sessions.lock().map_err(|e| e.to_string())?;
    if let Some(s) = map.get_mut(&id) {
        s.writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        s.writer.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Resize a session's PTY when the xterm fit-addon reflows.
#[tauri::command]
fn term_resize(state: State<TermState>, id: u32, rows: u16, cols: u16) -> Result<(), String> {
    let map = state.sessions.lock().map_err(|e| e.to_string())?;
    if let Some(s) = map.get(&id) {
        s.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Kill one session and remove it from the registry.
#[tauri::command]
fn term_close(state: State<TermState>, id: u32) -> Result<(), String> {
    let sess = state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&id);
    if let Some(mut s) = sess {
        let _ = s.child.kill();
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(TermState::default())
        .invoke_handler(tauri::generate_handler![
            http_proxy,
            groq_transcribe,
            tts_fetch,
            fetch_bytes,
            open_mic_settings,
            term_shells,
            term_open,
            term_write,
            term_resize,
            term_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
