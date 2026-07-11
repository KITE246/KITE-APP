#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

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
    use base64::Engine;
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
    use base64::Engine;
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
    use base64::Engine;
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            http_proxy,
            groq_transcribe,
            tts_fetch,
            fetch_bytes,
            open_mic_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
