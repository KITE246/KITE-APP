Kite v0.1: Wrap existing HTML chat app in Tauri 2 + add Kite Voice card.



LOGO: src/assets/logo.png (fox logo, use everywhere)



PART 1 - WRAP (minimal changes):

\- Tauri 2 + Vite. Copy reference/AI\_CHATBOT.html to src/index.html

\- Do NOT rewrite or refactor the app logic

\- localStorage keeps working as storage

\- Fix only what breaks in webview (fonts, fetch CORS issues)

\- Window: 1280x800, min 960x600

\- App icon: src/assets/logo.png



PART 2 - KITE VOICE CARD:

\- Floating draggable card (260x160, compact horizontal glass card) as overlay on top of the app

\- Layout: fox logo left (with pulsing sonar rings) | title + state | close X;

&#x20; small circular mic button + state text; thin level fill bar across the bottom

\- States: "Hold to talk" / "Listening..." / "Transcribing..." / "Thinking..." /

&#x20; "Speaking..." / error in coral

\- Draggable anywhere on screen (pointer capture, snaps 12px from edges)

\- Position saved to localStorage

\- Double-click card = minimize to small bubble

\- Close X button, reopen via mic button in top bar

\- Hold mic button (pointerdown/pointerup) to record

\- Recording: Web Audio API (AudioContext + ScriptProcessor) captures raw 16kHz

&#x20; mono PCM, encoded to a WAV blob in the browser. (MediaRecorder webm/opus was

&#x20; dropped — WebView2's opus encoder produced near-empty frames.)

\- Minimum 1s hold so a quick tap still captures real audio

\- On release: transcribe via Groq Cloud Whisper (see below)

\- On success: insert text into focused input/textarea in the app

\- If nothing focused: show text in a toast with copy button

\- On error: show the reason in coral on the card

\- Voice intelligence: after transcription, the text is inserted into the chat

&#x20; composer and routed through a command detector — "change model to X",

&#x20; "new chat", "clear chat", "send \[message]", "change theme"; anything else is

&#x20; sent to the AI (kiteAsk) and the reply is spoken via window.speechSynthesis (TTS)

\- Card state flow: idle → recording → transcribing → thinking → speaking → idle



TRANSCRIPTION (Groq Cloud Whisper — no local server):

\- Endpoint: POST https://api.groq.com/openai/v1/audio/transcriptions

\- Model: whisper-large-v3-turbo, response\_format json → {text:"..."}

\- Auth: the Groq API key already stored in Settings → API Keys (Bearer token)

\- Runs through the Rust `groq_transcribe` command (multipart upload done server-side

&#x20; to avoid webview CORS and to send real binary — the JS CORS proxy only forwards

&#x20; string bodies). Frontend base64-encodes the WAV and passes it to the command.

\- If no Groq key: show "Add a Groq API key in Settings" in coral on the card

\- No Python, no local model, no sidecar process — transcription is fully cloud-based



STYLE: port all CSS from reference/AI\_CHATBOT.html exactly

(--bg --teal --coral variables, Inter + Space Mono fonts, dark default)

Voice card uses same dark theme, teal accents, coral for errors.



RULES:

\- Zero console errors on boot

\- Don't break any existing chat/keys feature

\- Smoke test: keys tab saves, chat streams a reply, voice card drags

