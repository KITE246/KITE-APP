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

// ─────────────────────────── AGENT TOOLS ───────────────────────────
// Native tools the JS agent loop invokes. Every path is confined to the
// agent's working directory (PathGuard) so an agent can't read or clobber
// files elsewhere on disk. file_write / shell_exec are gated behind the
// frontend approval flow before they ever reach here.

/// Resolve `path` (relative or absolute) against `cwd` and guarantee the result
/// stays inside `cwd`. `must_exist` distinguishes reads from writes.
fn guard_path(cwd: &str, path: &str, must_exist: bool) -> Result<std::path::PathBuf, String> {
    if cwd.trim().is_empty() {
        return Err("agent has no working directory set".into());
    }
    let base = std::fs::canonicalize(cwd).map_err(|e| format!("working dir invalid: {}", e))?;
    let req = std::path::Path::new(path);
    let joined = if req.is_absolute() { req.to_path_buf() } else { base.join(req) };

    let canon = if joined.exists() {
        std::fs::canonicalize(&joined).map_err(|e| e.to_string())?
    } else {
        if must_exist {
            return Err(format!("no such file: {}", path));
        }
        let parent = joined.parent().ok_or("invalid path")?;
        let cparent = std::fs::canonicalize(parent)
            .map_err(|e| format!("parent directory invalid: {}", e))?;
        cparent.join(joined.file_name().ok_or("invalid file name")?)
    };

    if !canon.starts_with(&base) {
        return Err("path escapes the working directory".into());
    }
    Ok(canon)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileReadReq {
    cwd: String,
    path: String,
}

#[tauri::command]
fn agent_file_read(req: FileReadReq) -> Result<String, String> {
    let p = guard_path(&req.cwd, &req.path, true)?;
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    const LIMIT: usize = 200_000;
    let truncated = bytes.len() > LIMIT;
    let slice = &bytes[..bytes.len().min(LIMIT)];
    let mut text = String::from_utf8_lossy(slice).to_string();
    if truncated {
        text.push_str("\n…[truncated]");
    }
    Ok(text)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileWriteReq {
    cwd: String,
    path: String,
    content: String,
}

#[tauri::command]
fn agent_file_write(req: FileWriteReq) -> Result<String, String> {
    let p = guard_path(&req.cwd, &req.path, false)?;
    std::fs::write(&p, req.content.as_bytes()).map_err(|e| e.to_string())?;
    Ok(format!("wrote {} bytes to {}", req.content.len(), p.display()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShellExecReq {
    cwd: String,
    command: String,
    exec_id: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellChunk {
    exec_id: String,
    stream: String,
    chunk: String,
}

static SCRIPT_SEQ: AtomicU32 = AtomicU32::new(0);

/// Decide whether a command needs PowerShell (so it gets a temp `.ps1`) or can
/// run through `cmd /c`. Anything with `$`-variables, braces, backticks,
/// multiple lines, PowerShell comparison operators, verb-noun cmdlets, or a
/// PowerShell-only alias is treated as PowerShell; the rest (git, npm, python,
/// dir, echo, …) go to cmd where there is no extra parsing layer to mangle them.
fn command_needs_powershell(cmd: &str) -> bool {
    if cmd.contains('$') || cmd.contains('`') || cmd.contains('{') || cmd.contains('}')
        || cmd.contains('\n')
    {
        return true;
    }
    let lower = cmd.to_lowercase();
    for op in [
        " -eq ", " -ne ", " -lt ", " -gt ", " -le ", " -ge ", " -match ", " -notmatch ",
        " -like ", " -notlike ", " -and ", " -or ", " -not ", " -contains ", " -in ", " -notin ",
    ] {
        if lower.contains(op) {
            return true;
        }
    }
    let first = lower.split_whitespace().next().unwrap_or("");
    // verb-noun cmdlet, e.g. get-childitem / select-string
    if first.matches('-').count() == 1
        && first.split('-').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphabetic()))
    {
        return true;
    }
    const PS_ALIASES: [&str; 12] = [
        "ls", "gci", "cat", "gc", "pwd", "gm", "gl", "select", "where", "measure", "sort", "foreach",
    ];
    PS_ALIASES.contains(&first)
}

/// Run a shell command in the working directory, streaming each output line to
/// the frontend as `agent-shell` events (so it can mirror live into the agent's
/// terminal pane) while also accumulating the full output to return to the
/// agent. PowerShell code is written to a temp `.ps1` and run with `-File` so
/// inline syntax (`$i`, quotes, etc.) is never re-parsed; simple commands run
/// via `cmd /c`. Hard 45s timeout kills a runaway command.
#[tauri::command]
fn agent_shell_exec(app: tauri::AppHandle, req: ShellExecReq) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let exec_id = req.exec_id.clone().unwrap_or_default();
    let command = req.command.trim_end().to_string();

    // Build the child process. For PowerShell, stage the code in a temp .ps1 so
    // the shell reads it verbatim instead of parsing an escaped inline string.
    let mut temp_script: Option<std::path::PathBuf> = None;
    let mut c = if command_needs_powershell(&command) {
        let n = SCRIPT_SEQ.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("kite_agent_{}_{}.ps1", std::process::id(), n));
        // UTF-8 BOM so Windows PowerShell 5.1 reads the script as UTF-8.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(command.as_bytes());
        std::fs::write(&path, &bytes).map_err(|e| format!("temp script write failed: {}", e))?;
        let mut c = Command::new("powershell");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &path.to_string_lossy(),
        ]);
        temp_script = Some(path);
        c
    } else {
        let mut c = Command::new("cmd");
        c.args(["/c", &command]);
        c
    };
    if !req.cwd.trim().is_empty() {
        c.current_dir(&req.cwd);
    }
    c.stdout(Stdio::piped()).stderr(Stdio::piped());

    let spawned = c.spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            if let Some(p) = &temp_script {
                let _ = std::fs::remove_file(p);
            }
            return Err(format!("failed to run: {}", e));
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let acc = Arc::new(Mutex::new(String::new()));

    let pump = |reader: Box<dyn std::io::Read + Send>, is_err: bool| {
        let acc = acc.clone();
        let app = app.clone();
        let id = exec_id.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().flatten() {
                {
                    let mut g = acc.lock().unwrap();
                    if is_err {
                        g.push_str("[stderr] ");
                    }
                    g.push_str(&line);
                    g.push('\n');
                }
                let _ = app.emit(
                    "agent-shell",
                    ShellChunk {
                        exec_id: id.clone(),
                        stream: if is_err { "err".into() } else { "out".into() },
                        chunk: line,
                    },
                );
            }
        })
    };

    let mut handles = vec![];
    if let Some(out) = stdout {
        handles.push(pump(Box::new(out), false));
    }
    if let Some(err) = stderr {
        handles.push(pump(Box::new(err), true));
    }

    // wait with a hard timeout, polling so we can kill a runaway process
    let start = Instant::now();
    let mut timed_out = false;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(45) {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break -1;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    for h in handles {
        let _ = h.join();
    }
    if let Some(p) = &temp_script {
        let _ = std::fs::remove_file(p);
    }

    let mut s = acc.lock().unwrap().clone();
    if s.len() > 12_000 {
        s.truncate(12_000);
        s.push_str("\n…[truncated]");
    }
    if timed_out {
        s.push_str("\n[killed: timed out after 45s]");
    }
    Ok(format!("exit {}\n{}", code, s.trim()))
}

/// Native folder picker for the New Agent form.
#[tauri::command]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
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
            term_close,
            agent_file_read,
            agent_file_write,
            agent_shell_exec,
            pick_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
