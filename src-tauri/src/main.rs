#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use base64::Engine;
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

// ─────────────────────────── CODE INTERPRETER ───────────────────────────
// The agent's run_code tool: write a snippet to a temp file, execute it with a
// language-appropriate interpreter, stream stdout/stderr live to the frontend
// as `agent-shell` events (same channel shell_exec uses, so the activity pane
// mirrors it) and return exit code + captured output. Confined by nothing but
// the hard timeout — approval is enforced in the frontend before we get here.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunCodeReq {
    language: String,
    code: String,
    cwd: Option<String>,
    exec_id: Option<String>,
    timeout_secs: Option<u64>,
}

/// Map a loose language name to (interpreter, args-template, file-extension).
/// The literal "{file}" in the args is replaced with the temp script path.
fn code_runner(lang: &str) -> Result<(&'static str, Vec<&'static str>, &'static str), String> {
    match lang.trim().to_lowercase().as_str() {
        "python" | "py" | "python3" => Ok(("python", vec!["{file}"], "py")),
        "javascript" | "js" | "node" | "nodejs" => Ok(("node", vec!["{file}"], "js")),
        "typescript" | "ts" => Ok(("npx", vec!["-y", "tsx", "{file}"], "ts")),
        "powershell" | "ps" | "ps1" | "pwsh" => Ok((
            "powershell",
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "{file}",
            ],
            "ps1",
        )),
        "bash" | "sh" | "shell" => Ok(("bash", vec!["{file}"], "sh")),
        "ruby" | "rb" => Ok(("ruby", vec!["{file}"], "rb")),
        other => Err(format!(
            "unsupported language '{}' — try python, javascript, powershell, bash, typescript or ruby",
            other
        )),
    }
}

/// Execute a code snippet and return "exit N\n<output>". Mirrors agent_shell_exec's
/// live streaming + polling-kill timeout, but keyed on interpreter + temp file.
#[tauri::command]
fn agent_run_code(app: tauri::AppHandle, req: RunCodeReq) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let exec_id = req.exec_id.clone().unwrap_or_default();
    let (interp, arg_tmpl, ext) = code_runner(&req.language)?;
    let timeout = Duration::from_secs(req.timeout_secs.unwrap_or(30).clamp(1, 300));

    // Stage the snippet in a temp file. PowerShell gets a UTF-8 BOM so Windows
    // PowerShell 5.1 reads it as UTF-8 (matching agent_shell_exec).
    let n = SCRIPT_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("kite_code_{}_{}.{}", std::process::id(), n, ext));
    let mut bytes: Vec<u8> = Vec::new();
    if ext == "ps1" {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(req.code.as_bytes());
    std::fs::write(&path, &bytes).map_err(|e| format!("temp script write failed: {}", e))?;

    let path_str = path.to_string_lossy().to_string();
    let args: Vec<String> = arg_tmpl
        .iter()
        .map(|a| if *a == "{file}" { path_str.clone() } else { (*a).to_string() })
        .collect();

    // On Windows, npx (typescript) is a .cmd shim that needs cmd /C to resolve.
    let mut c = if cfg!(windows) && interp == "npx" {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("npx").args(&args);
        c
    } else {
        let mut c = Command::new(interp);
        c.args(&args);
        c
    };
    if let Some(cwd) = &req.cwd {
        if !cwd.trim().is_empty() {
            c.current_dir(cwd);
        }
    }
    c.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(format!(
                "failed to launch {} — is it installed and on PATH? ({})",
                interp, e
            ));
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

    let start = Instant::now();
    let mut timed_out = false;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {
                if start.elapsed() > timeout {
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
    let _ = std::fs::remove_file(&path);

    let mut s = acc.lock().unwrap().clone();
    if s.len() > 12_000 {
        s.truncate(12_000);
        s.push_str("\n…[truncated]");
    }
    if timed_out {
        s.push_str(&format!(
            "\n[killed: timed out after {}s]",
            timeout.as_secs()
        ));
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

// ─────────────────────────── DRAG & DROP ───────────────────────────
// Tauri captures OS file drops itself and hands the frontend absolute PATHS
// (not browser File objects). This command reads one dropped path so the app
// can route it: text → context/cat, image → vision/data-URI, dir → cd. Unlike
// the agent file tools it is deliberately NOT PathGuarded — the user chose the
// file by dragging it — but it is read-only and size-capped.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DroppedFile {
    kind: String,   // "text" | "image" | "dir" | "binary"
    name: String,
    path: String,
    dir: String,    // parent directory (for terminal cd)
    content: String, // text body, or "data:...;base64,..." for images
    size: u64,
}

#[tauri::command]
fn read_dropped_file(path: String) -> Result<DroppedFile, String> {
    let p = std::path::Path::new(&path);
    let meta = std::fs::metadata(p).map_err(|e| format!("cannot read {}: {}", path, e))?;
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let dir = p
        .parent()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    if meta.is_dir() {
        return Ok(DroppedFile {
            kind: "dir".into(),
            name,
            path: path.clone(),
            dir: path.clone(),
            content: String::new(),
            size: 0,
        });
    }

    let size = meta.len();
    let ext = p
        .extension()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    const IMAGE_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];
    if IMAGE_EXTS.contains(&ext.as_str()) {
        if size > 8_000_000 {
            return Err("image too large (8 MB max)".into());
        }
        let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
        let mime = match ext.as_str() {
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            _ => "image/jpeg",
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Ok(DroppedFile {
            kind: "image".into(),
            name,
            path,
            dir,
            content: format!("data:{};base64,{}", mime, b64),
            size,
        });
    }

    // Anything else: try to read as text. Reject obviously-binary content.
    if size > 3_000_000 {
        return Err("file too large to read as text (3 MB max)".into());
    }
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    // NUL byte in the first 8 KB → treat as binary, don't dump garbage.
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0u8) {
        return Ok(DroppedFile {
            kind: "binary".into(),
            name,
            path,
            dir,
            content: String::new(),
            size,
        });
    }
    const LIMIT: usize = 40_000;
    let truncated = bytes.len() > LIMIT;
    let slice = &bytes[..bytes.len().min(LIMIT)];
    let mut content = String::from_utf8_lossy(slice).to_string();
    if truncated {
        content.push_str("\n…[truncated]");
    }
    Ok(DroppedFile {
        kind: "text".into(),
        name,
        path,
        dir,
        content,
        size,
    })
}

// ─────────────────────────── GMAIL (IMAP + SMTP) ───────────────────────────
// Gmail with an app password speaks IMAP (read/search) and SMTP (send) — not
// HTTP — so it can't go through the fetch/http_proxy path the other Connections
// services use. These commands run the native protocols in Rust. Credentials
// come from the frontend per call (stored only in the browser's localStorage);
// nothing is persisted here.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailReq {
    address: String,
    password: String,
    #[serde(default)]
    count: u32,
    #[serde(default)]
    query: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    to: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    body: String,
}

type GmailSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

fn gmail_session(address: &str, password: &str) -> Result<GmailSession, String> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect(("imap.gmail.com", 993), "imap.gmail.com", &tls)
        .map_err(|e| format!("imap connect failed: {}", e))?;
    client.login(address, password).map_err(|(e, _)| {
        format!(
            "imap login failed: {} — use a Gmail App Password (myaccount.google.com/apppasswords), not your normal password",
            e
        )
    })
}

fn cow_str(o: &Option<&[u8]>) -> String {
    o.map(|c| String::from_utf8_lossy(c).to_string())
        .unwrap_or_default()
}

fn addr_to_string(a: &imap_proto::types::Address) -> String {
    let mbox = cow_str(&a.mailbox);
    let host = cow_str(&a.host);
    let name = cow_str(&a.name);
    let email = if host.is_empty() {
        mbox
    } else {
        format!("{}@{}", mbox, host)
    };
    if name.is_empty() {
        email
    } else {
        format!("{} <{}>", name, email)
    }
}

fn envelope_row(f: &imap::types::Fetch) -> String {
    let uid = f.uid.unwrap_or(0);
    let env = f.envelope();
    let subject = env
        .and_then(|e| e.subject.as_ref())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .unwrap_or_else(|| "(no subject)".into());
    let from = env
        .and_then(|e| e.from.as_ref())
        .and_then(|v| v.first())
        .map(addr_to_string)
        .unwrap_or_default();
    format!("[{}] {}  —  {}", uid, subject.trim(), from)
}

/// List the most recent `count` messages in the inbox (uid, subject, sender).
#[tauri::command]
fn gmail_list(req: GmailReq) -> Result<String, String> {
    let mut session = gmail_session(&req.address, &req.password)?;
    let mailbox = session.select("INBOX").map_err(|e| e.to_string())?;
    let total = mailbox.exists;
    if total == 0 {
        let _ = session.logout();
        return Ok("(inbox is empty)".into());
    }
    let count = if req.count == 0 { 10 } else { req.count };
    let start = if total > count { total - count + 1 } else { 1 };
    let range = format!("{}:{}", start, total);
    let messages = session
        .fetch(range, "(UID ENVELOPE)")
        .map_err(|e| e.to_string())?;
    let mut rows: Vec<(u32, String)> = messages
        .iter()
        .map(|m| (m.uid.unwrap_or(0), envelope_row(m)))
        .collect();
    let _ = session.logout();
    rows.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    Ok(rows
        .into_iter()
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// IMAP TEXT search; returns matching messages (newest first, capped at 25).
#[tauri::command]
fn gmail_search(req: GmailReq) -> Result<String, String> {
    let mut session = gmail_session(&req.address, &req.password)?;
    session.select("INBOX").map_err(|e| e.to_string())?;
    let q = req.query.replace('"', "");
    let uids = session
        .uid_search(format!("TEXT \"{}\"", q))
        .map_err(|e| e.to_string())?;
    if uids.is_empty() {
        let _ = session.logout();
        return Ok("(no matches)".into());
    }
    let mut ids: Vec<u32> = uids.into_iter().collect();
    ids.sort_by(|a, b| b.cmp(a));
    ids.truncate(25);
    let set = ids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let messages = session
        .uid_fetch(set, "(UID ENVELOPE)")
        .map_err(|e| e.to_string())?;
    let mut rows: Vec<(u32, String)> = messages
        .iter()
        .map(|m| (m.uid.unwrap_or(0), envelope_row(m)))
        .collect();
    let _ = session.logout();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(rows
        .into_iter()
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn extract_text(mail: &mailparse::ParsedMail) -> String {
    if mail.subparts.is_empty() {
        if mail.ctype.mimetype.starts_with("text/") {
            return mail.get_body().unwrap_or_default();
        }
        return String::new();
    }
    for p in &mail.subparts {
        if p.ctype.mimetype == "text/plain" {
            if let Ok(b) = p.get_body() {
                if !b.trim().is_empty() {
                    return b;
                }
            }
        }
    }
    let mut out = String::new();
    for p in &mail.subparts {
        out.push_str(&extract_text(p));
    }
    out
}

/// Read one message by uid and return sender, subject and a text body.
#[tauri::command]
fn gmail_read(req: GmailReq) -> Result<String, String> {
    let mut session = gmail_session(&req.address, &req.password)?;
    session.select("INBOX").map_err(|e| e.to_string())?;
    let messages = session
        .uid_fetch(&req.id, "(BODY[])")
        .map_err(|e| e.to_string())?;
    let msg = messages.iter().next().ok_or("email not found")?;
    let body = msg.body().ok_or("no body returned")?;
    let parsed = mailparse::parse_mail(body).map_err(|e| e.to_string())?;
    let text = extract_text(&parsed);
    let _ = session.logout();
    let hdr = |key: &str| {
        parsed
            .headers
            .iter()
            .find(|h| h.get_key().eq_ignore_ascii_case(key))
            .map(|h| h.get_value())
            .unwrap_or_default()
    };
    Ok(format!(
        "From: {}\nSubject: {}\nDate: {}\n\n{}",
        hdr("from"),
        hdr("subject"),
        hdr("date"),
        text.chars().take(8000).collect::<String>()
    ))
}

/// Send a plain-text email via Gmail SMTP.
#[tauri::command]
fn gmail_send(req: GmailReq) -> Result<String, String> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};
    let email = Message::builder()
        .from(
            req.address
                .parse()
                .map_err(|e| format!("bad from address: {}", e))?,
        )
        .to(req
            .to
            .parse()
            .map_err(|e| format!("bad to address: {}", e))?)
        .subject(req.subject.clone())
        .date_now()
        .body(req.body.clone())
        .map_err(|e| e.to_string())?;
    let creds = Credentials::new(req.address.clone(), req.password.clone());
    let mailer = SmtpTransport::relay("smtp.gmail.com")
        .map_err(|e| e.to_string())?
        .credentials(creds)
        .build();
    mailer
        .send(&email)
        .map_err(|e| format!("smtp send failed: {}", e))?;
    Ok(format!("sent to {}", req.to))
}

// ─────────────────────────── MCP (stdio JSON-RPC servers) ───────────────────────────
// MCP servers are long-lived child processes (usually `npx …`) that speak
// line-delimited JSON-RPC over stdin/stdout. Unlike the terminal these need
// clean pipes (not a PTY, which would echo and line-edit), so they get their
// own registry. stdout lines are forwarded to the frontend as `mcp-stdout`
// events; the JS client correlates them to requests by JSON-RPC id.

struct McpProc {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
}

#[derive(Default)]
struct McpState {
    procs: Mutex<HashMap<String, McpProc>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpLine {
    id: String,
    line: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpStartReq {
    id: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

/// Spawn an MCP server as a piped child process and stream its stdout/stderr
/// back as `mcp-stdout` / `mcp-stderr` events. On Windows the command runs via
/// `cmd /C` so `.cmd` shims (npx, uvx) resolve on PATH. No-op if already running.
#[tauri::command]
fn mcp_start(app: tauri::AppHandle, state: State<McpState>, req: McpStartReq) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    if state
        .procs
        .lock()
        .map_err(|e| e.to_string())?
        .contains_key(&req.id)
    {
        return Ok(());
    }

    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&req.command).args(&req.args);
        c
    } else {
        let mut c = Command::new(&req.command);
        c.args(&req.args);
        c
    };
    for (k, v) in &req.env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — no flashing console
    }

    let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {}", e))?;
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let stderr = child.stderr.take().ok_or("no stderr pipe")?;
    let stdin = child.stdin.take().ok_or("no stdin pipe")?;

    // Register before spawning readers so an instant-exit can't race the insert.
    state
        .procs
        .lock()
        .map_err(|e| e.to_string())?
        .insert(req.id.clone(), McpProc { child, stdin });

    let (ido, appo) = (req.id.clone(), app.clone());
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = appo.emit(
                        "mcp-stdout",
                        McpLine {
                            id: ido.clone(),
                            line: line.trim_end().to_string(),
                        },
                    );
                }
                Err(_) => break,
            }
        }
        // stdout closed → the server has exited; drop bookkeeping and notify.
        if let Some(st) = appo.try_state::<McpState>() {
            if let Ok(mut map) = st.procs.lock() {
                map.remove(&ido);
            }
        }
        let _ = appo.emit("mcp-exit", ido.clone());
    });

    let (ide, appe) = (req.id.clone(), app.clone());
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = appe.emit(
                        "mcp-stderr",
                        McpLine {
                            id: ide.clone(),
                            line: line.trim_end().to_string(),
                        },
                    );
                }
                Err(_) => break,
            }
        }
    });

    Ok(())
}

/// Write one JSON-RPC message (a single line) to a running server's stdin.
#[tauri::command]
fn mcp_send(state: State<McpState>, id: String, message: String) -> Result<(), String> {
    use std::io::Write;
    let mut map = state.procs.lock().map_err(|e| e.to_string())?;
    let p = map.get_mut(&id).ok_or("mcp server is not running")?;
    p.stdin
        .write_all(message.as_bytes())
        .map_err(|e| e.to_string())?;
    p.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
    p.stdin.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// Kill a running server and drop it from the registry.
#[tauri::command]
fn mcp_stop(state: State<McpState>, id: String) -> Result<(), String> {
    if let Some(mut p) = state.procs.lock().map_err(|e| e.to_string())?.remove(&id) {
        let _ = p.child.kill();
        let _ = p.child.wait();
    }
    Ok(())
}

/// Ids of servers currently running.
#[tauri::command]
fn mcp_running(state: State<McpState>) -> Vec<String> {
    state
        .procs
        .lock()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Read the Claude Desktop config JSON (for the MCP "Import" button).
#[tauri::command]
fn mcp_claude_config() -> Result<String, String> {
    let base = std::env::var("APPDATA").map_err(|_| "APPDATA not set".to_string())?;
    let path = std::path::Path::new(&base)
        .join("Claude")
        .join("claude_desktop_config.json");
    if !path.exists() {
        return Err(format!("no config at {}", path.display()));
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

// ─────────────────────────── AGENT FILE + DATA TOOLS ───────────────────────────
// Extra file/system tools for the expanded agent toolset. Every path is
// PathGuarded to the agent's working directory (same guarantee as
// agent_file_read/write). Mutating tools (append/delete/move/zip/unzip) are
// gated behind the frontend approval flow before they reach here.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileListReq {
    cwd: String,
    #[serde(default)]
    path: String,
}

/// List a directory's immediate children (dirs first, then files with sizes).
#[tauri::command]
fn agent_file_list(req: FileListReq) -> Result<String, String> {
    let target = if req.path.trim().is_empty() { ".".to_string() } else { req.path.clone() };
    let p = guard_path(&req.cwd, &target, true)?;
    let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        return Err(format!("{} is not a directory", target));
    }
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&p).map_err(|e| e.to_string())? {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        let name = entry.file_name().to_string_lossy().to_string();
        match entry.metadata() {
            Ok(m) if m.is_dir() => dirs.push(format!("{}/", name)),
            Ok(m) => {
                let sz = m.len();
                let human = if sz >= 1_048_576 { format!("{:.1} MB", sz as f64 / 1_048_576.0) }
                    else if sz >= 1024 { format!("{:.1} KB", sz as f64 / 1024.0) }
                    else { format!("{} B", sz) };
                files.push(format!("{}  ({})", name, human));
            }
            Err(_) => files.push(name),
        }
    }
    dirs.sort();
    files.sort();
    let mut out = dirs;
    out.extend(files);
    if out.is_empty() { return Ok("(empty directory)".into()); }
    Ok(out.join("\n"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileSearchReq {
    cwd: String,
    #[serde(default)]
    dir: String,
    pattern: String,
}

/// Simple case-insensitive filename search under `dir` (recursive, capped at
/// 200 hits). `pattern` supports `*` wildcards, e.g. "*.py" or "report*".
#[tauri::command]
fn agent_file_search(req: FileSearchReq) -> Result<String, String> {
    let base_rel = if req.dir.trim().is_empty() { ".".to_string() } else { req.dir.clone() };
    let root = guard_path(&req.cwd, &base_rel, true)?;
    let pat = req.pattern.to_lowercase();
    // Build the literal fragments a `*`-glob must contain, in order.
    let parts: Vec<String> = pat.split('*').map(|s| s.to_string()).collect();
    let matches = |name: &str| -> bool {
        let n = name.to_lowercase();
        if !pat.contains('*') { return n.contains(&pat); }
        let mut idx = 0usize;
        for (i, frag) in parts.iter().enumerate() {
            if frag.is_empty() { continue; }
            match n[idx..].find(frag.as_str()) {
                Some(pos) => {
                    // first fragment with no leading '*' must anchor at start
                    if i == 0 && pos != 0 { return false; }
                    idx += pos + frag.len();
                }
                None => return false,
            }
        }
        // trailing fragment with no '*' after it must anchor at end
        if let Some(last) = parts.last() {
            if !last.is_empty() && !n.ends_with(last.as_str()) { return false; }
        }
        true
    };
    let mut hits: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(&root).max_depth(12).into_iter().filter_map(|e| e.ok()) {
        if hits.len() >= 200 { break; }
        if !entry.file_type().is_file() { continue; }
        let name = entry.file_name().to_string_lossy().to_string();
        if matches(&name) {
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            hits.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    if hits.is_empty() { return Ok(format!("(no files matching \"{}\")", req.pattern)); }
    hits.sort();
    Ok(hits.join("\n"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileAppendReq {
    cwd: String,
    path: String,
    content: String,
}

#[tauri::command]
fn agent_file_append(req: FileAppendReq) -> Result<String, String> {
    use std::io::Write;
    let p = guard_path(&req.cwd, &req.path, false)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(|e| e.to_string())?;
    f.write_all(req.content.as_bytes()).map_err(|e| e.to_string())?;
    Ok(format!("appended {} bytes to {}", req.content.len(), p.display()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilePathReq {
    cwd: String,
    path: String,
}

#[tauri::command]
fn agent_file_delete(req: FilePathReq) -> Result<String, String> {
    let p = guard_path(&req.cwd, &req.path, true)?;
    let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
        Ok(format!("deleted directory {}", p.display()))
    } else {
        std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        Ok(format!("deleted {}", p.display()))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileMoveReq {
    cwd: String,
    from: String,
    to: String,
}

#[tauri::command]
fn agent_file_move(req: FileMoveReq) -> Result<String, String> {
    let src = guard_path(&req.cwd, &req.from, true)?;
    let dst = guard_path(&req.cwd, &req.to, false)?;
    std::fs::rename(&src, &dst).map_err(|e| e.to_string())?;
    Ok(format!("moved {} -> {}", src.display(), dst.display()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileZipReq {
    cwd: String,
    files: Vec<String>,
    output: String,
}

/// Create a deflate zip of the listed files (each PathGuarded) at `output`.
#[tauri::command]
fn agent_file_zip(req: FileZipReq) -> Result<String, String> {
    use std::io::Write;
    if req.files.is_empty() {
        return Err("no files given to zip".into());
    }
    let out = guard_path(&req.cwd, &req.output, false)?;
    let f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(f);
    let opts: zip::write::FileOptions<()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut n = 0;
    for rel in &req.files {
        let src = guard_path(&req.cwd, rel, true)?;
        let name = src
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| rel.clone());
        let bytes = std::fs::read(&src).map_err(|e| e.to_string())?;
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
        n += 1;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(format!("zipped {} file(s) into {}", n, out.display()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileUnzipReq {
    cwd: String,
    path: String,
    #[serde(default)]
    dest: String,
}

/// Extract a zip archive into `dest` (default: alongside the archive).
#[tauri::command]
fn agent_file_unzip(req: FileUnzipReq) -> Result<String, String> {
    let archive = guard_path(&req.cwd, &req.path, true)?;
    let dest_rel = if req.dest.trim().is_empty() { ".".to_string() } else { req.dest.clone() };
    let dest = guard_path(&req.cwd, &dest_rel, false)?;
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let f = std::fs::File::open(&archive).map_err(|e| e.to_string())?;
    let mut ar = zip::ZipArchive::new(f).map_err(|e| format!("not a valid zip: {}", e))?;
    let count = ar.len();
    ar.extract(&dest).map_err(|e| e.to_string())?;
    Ok(format!("extracted {} entries into {}", count, dest.display()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParseExcelReq {
    cwd: String,
    path: String,
    #[serde(default)]
    sheet: String,
}

/// Read an .xlsx/.xls/.ods sheet and return it as tab-separated rows (capped).
#[tauri::command]
fn agent_parse_excel(req: ParseExcelReq) -> Result<String, String> {
    use calamine::{open_workbook_auto, Reader};
    let p = guard_path(&req.cwd, &req.path, true)?;
    let mut wb = open_workbook_auto(&p).map_err(|e| format!("cannot open workbook: {}", e))?;
    let names = wb.sheet_names().to_vec();
    if names.is_empty() {
        return Err("workbook has no sheets".into());
    }
    let sheet = if req.sheet.trim().is_empty() { names[0].clone() } else { req.sheet.clone() };
    let range = wb
        .worksheet_range(&sheet)
        .map_err(|e| format!("sheet '{}' not found: {}", sheet, e))?;
    let mut out = String::new();
    out.push_str(&format!("Sheet: {}  ({} sheets total)\n", sheet, names.len()));
    let mut rows = 0;
    for row in range.rows() {
        if rows >= 500 {
            out.push_str("…[truncated at 500 rows]\n");
            break;
        }
        let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
        rows += 1;
    }
    Ok(out)
}

fn main() {
    tauri::Builder::default()
        .manage(McpState::default())
        .invoke_handler(tauri::generate_handler![
            http_proxy,
            groq_transcribe,
            tts_fetch,
            fetch_bytes,
            open_mic_settings,
            agent_file_read,
            agent_file_write,
            agent_shell_exec,
            agent_run_code,
            agent_file_list,
            agent_file_search,
            agent_file_append,
            agent_file_delete,
            agent_file_move,
            agent_file_zip,
            agent_file_unzip,
            agent_parse_excel,
            pick_folder,
            read_dropped_file,
            gmail_list,
            gmail_search,
            gmail_read,
            gmail_send,
            mcp_start,
            mcp_send,
            mcp_stop,
            mcp_running,
            mcp_claude_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
