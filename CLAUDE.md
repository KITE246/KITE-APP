# KITE — AI-Powered Terminal & Agent Platform
## CLAUDE.md — Project Brief (read this fully before touching any code)

---

## WHAT KITE IS

Kite is a Rust + Tauri 2 desktop app that combines:
1. A real daily-driver terminal (replaces Windows Terminal)
2. A parallel AI agent platform with voice control
3. A connection hub for all your tools and services

**The fox logo is at src/assets/logo.png — use it everywhere.**

---

## WHAT IS ALREADY BUILT (do not break these)

### ✅ CHAT TAB
- 10+ AI providers streaming (Groq, Gemini, Mistral, Cerebras,
  OpenRouter, NVIDIA, Cloudflare, GitHub Models, HuggingFace, OpenAI)
- Key rotation across 4 slots per provider
- Daily budget tracking and usage charts
- Markdown rendering with syntax highlighting
- localStorage persistence for chats and keys
- Model picker with all providers

### ✅ KITE VOICE CARD
- Floating draggable card (~280px) with fox logo
- Always on top of everything, snaps to edges
- Position persisted to localStorage
- Double-click minimizes to bubble
- States: idle / recording / transcribing / thinking / speaking / error
- F9 global hotkey (press to start, press to stop)
- Recording: click-to-start/click-to-stop (2 second minimum)
- STT: Groq whisper-large-v3 (cloud, uses existing key rotation)
  OR local faster-whisper server at http://127.0.0.1:5000
- Local whisper server: C:\Users\praga\whisper-app\server.py
  Python: C:\Users\praga\whisper-app\whisper-env\Scripts\python.exe
  Model: faster-whisper-large-v3-turbo-ct2 (fully offline)
- On transcript: send to AI for command parsing → execute command
- TTS: window.speechSynthesis speaks AI response back
- Animated rings around logo (teal=idle, coral=recording, pulse=speaking)
- 5 level bars animate to audio levels
- Mic button in top chrome to open/close card
- Voice commands work in all tabs (see Voice Everywhere section)
- After agent finishes: Kite announces result via TTS automatically

### ✅ TERMINAL TAB
- Real PTY via portable-pty crate (ConPTY on Windows)
- Terminal TABS bar: +new tab, close, Ctrl+Shift+T/W
- Split panes: split right | split down, max 4 panes
- Drag borders to resize panes
- xterm.js + WebGL renderer + fit + search + web-links addons
- Shell profiles auto-detected: PowerShell 7, Windows PowerShell,
  CMD, Git Bash, WSL distros
- Font: Cascadia Code/Consolas, size 14, ligatures
- 10000 line scrollback, copy-on-select, right-click paste
- Multi-line paste warning
- Clickable URLs
- OSC title updates in tab labels
- Status bar: cwd, shell name, ConPTY indicator
- Right-click context menu: Copy/Paste/Clear/Split/Close/Send to AI
- Toolbar: [+ New Tab ▾] [Split Right |] [Split Down -] [Clear]
- Kill all PTYs on app exit (no orphans)
- Layout persisted to localStorage

### ✅ AGENTS TAB
- Sessions list (left 280px): status dot, name, task, engine badge,
  model, cost, cancel button, subagents indented under parent
- New Agent form: name, task, engine, model, cwd, tools, budget
- Agent detail (right): activity stream, approval blocks, message input
- Engine options: API | Local | OpenCode | Coordinator
- Agent loop: JS ReAct loop using existing provider chain
- Built-in tools:
  * web_fetch(url) — GET request, returns text (auto)
  * file_read(path) — PathGuard scoped to cwd (auto)
  * file_write(path, content) — writes file (ask approval)
  * shell_exec(command) — runs in cwd, 45s timeout (ask approval)
    Windows: PowerShell commands → temp .ps1 file
    Simple commands → cmd /c
  * spawn_subagent(name, goal, engine?, model?) — child agent (auto)
    depth ≤ 2, max 8 concurrent
  * screenshot_url(url) — Puppeteer screenshot (ask)
  * run_code(language, code) — Python/JS/PS live execution (ask)
  * save_artifact(name, content, type) — saves output file
- Approval flow: agent pauses → inline [Approve][Deny] in stream
- Budget: max iterations(20), max tokens, max cost, wall clock
- Activity stream: markdown rendered, code highlighted, images inline
- Token/cost ticker in pane header
- Artifacts panel: files created, screenshots, code written
- Agent badge in top chrome: "● N agents live"
- Subagents auto-open new panes, tinted under parent
- After completion: TTS announces result automatically

### ✅ COORDINATOR AGENT
- Engine: Coordinator
- Step 1: Analyzes big goal → breaks into parallel subtasks
- Step 2: Spawns worker agents in parallel (join_all)
- Step 3: Monitors all workers with live dashboard
- Step 4: Synthesizes all results into final output + TTS summary
- Max workers: 2-8 (slider)
- Worker model can differ from coordinator model

### ✅ CONNECTIONS TAB
- Card grid with real logos via cdn.simpleicons.org
- Connected services: GitHub, Gmail, Google Drive, Notion,
  Cloudflare, Linear, Vercel, Figma, Supabase
- Additional: Discord, Twitter/X, Reddit, YouTube, OpenAI
- API key storage: localStorage encrypted (btoa)
- Shows last4 chars after saving
- Test button per connection
- Read tools: auto-approved
- Write tools: always ask approval
- Tool namespacing: conn__{service}__{tool}
- GitHub tools: list_repos, list_prs, list_issues, get_file,
  create_issue, search
- Gmail tools: list_emails, search, read_email, send [approval]
- Drive tools: list_files, search, read_file, create_file [approval]
- Notion tools: list_pages, search, read_page, create/update [approval]
- Cloudflare: list_zones, dns_records, analytics, purge [approval]
- Linear: list_issues, my_issues, projects, create [approval]
- Vercel: list_projects, deployments, logs, redeploy [approval]
- Figma: list_files, get_file, comments, export_node
- Supabase: list_tables, query [approval], functions, logs
- Web Search (DuckDuckGo, always available, no key)

### ✅ MCP TAB
- Built-in servers (toggle to enable):
  * Memory (@modelcontextprotocol/server-memory) — 9 tools
  * Sequential Thinking — 1 tool
  * Filesystem (@modelcontextprotocol/server-filesystem)
  * Fetch (@modelcontextprotocol/server-fetch)
  * Puppeteer (@modelcontextprotocol/server-puppeteer)
  * Playwright (@playwright/mcp)
  * Exa Search (needs EXA_API_KEY)
  * Firecrawl (needs FIRECRAWL_API_KEY)
  * Context7 (@upstash/context7-mcp)
  * Chrome DevTools
  * Brave Search (needs BRAVE_API_KEY)
  * Git MCP (@modelcontextprotocol/server-git)
  * Terminal/Shell MCP
  * Knowledge Graph MCP
  * SQLite MCP (needs db path)
  * PDF MCP
- Custom servers: add your own (stdio or HTTP)
- Import from Claude Desktop config
  Path: C:\Users\praga\AppData\Roaming\Claude\claude_desktop_config.json
- MCP client: JSON-RPC over stdio (spawn via Tauri shell)
- Tool namespacing: mcp__{server}__{tool}
- Auto-restart on crash (max 3 times)
- Always-allow toggle per server
- Tool browser per server

---

## WHAT WE ARE BUILDING IN DAY 5

### 1. HEY KITE — WAKE WORD
- Always-on background mic listener
- Detects "Hey Kite" → activates voice card automatically
- Implementation: continuous recording, chunk every 1s,
  send to Groq whisper for keyword detection
- Visual: small pulsing dot in status bar when listening
- "Hey Kite stop" / "Hey Kite cancel" → cancels current agent
- Settings: On/Off toggle, sensitivity slider

### 2. VOICE EVERYWHERE
Full voice command system across all tabs:

CHAT: send message, new chat, clear, switch model, summarize
AGENTS: new agent [task], cancel [name], status, use [mcp] and [task],
        run coordinator on [task], approve, deny
TERMINAL: run [command], explain this, fix this error, new terminal,
          cd to [folder]
MCP: enable [server], use [tool]
NAVIGATION: go to [tab], open settings, show projects
KITE SPEAKS BACK: confirms every command, announces completions,
                  suggests corrections

Voice parser: send transcript to AI → get JSON command → execute
System: {action, target, params, context}

Example: "Hey Kite use firecrawl mcp and scrape nvidia website"
→ parser returns: {action: "new_agent", params: {task: "scrape nvidia 
  website", tools: ["mcp__firecrawl__scrape"]}}
→ creates + starts agent automatically

### 3. AGENT SCREENSHOTS
- screenshot_url(url) via Puppeteer MCP
- Renders screenshot inline in activity stream
- Vision analysis if model supports it (Claude/GPT-4V/Gemini)
- page_click, page_extract, page_fill [approval], page_wait_and_extract

### 4. CODE INTERPRETER
- run_code(language, code) tool
- Languages: Python, JavaScript/Node, PowerShell, Shell
- Write to temp file → execute → capture output → stream live
- Code blocks with syntax highlighting in activity stream
- 30 second timeout, configurable

### 5. DRAG AND DROP FILES
- Global drag-drop handler across entire app
- Drop on CHAT → reads content, adds as context
  (PDF via PDF MCP, images as vision input)
- Drop on AGENT → adds to working context
- Drop on TERMINAL → cat/run file
- Drop on CONNECTIONS → import tokens JSON
- Teal dashed border on valid drop targets
- "Drop to add to [context]" tooltip while hovering

### 6. PROJECTS VIEW
Left sidebar has two sections: PROJECTS (top) + RECENT (bottom)
Project = { name, color, icon, chats[], agents[], terminals[],
            files[], notes (markdown), created_at }
- Create project from current session
- Inside project: sidebar (chats, agents, terminals, files) +
  main content + project notes panel
- Templates: Web Scraping, Code Review, Research, Custom
- Export project as markdown
- Archive completed projects

### 7. AGENT MEMORY
- Before each run: query Memory MCP for relevant context
- Inject memories into system prompt
- After completion: save key facts to Memory MCP
- Memory types: Facts, Preferences, Patterns, Projects
- Memory browser in MCP tab

### 8. SMART TERMINAL
- AI autocomplete: ghost text suggestions as you type
  (Groq llama3 for speed, Tab to accept)
- Fix error button: coral "⚡ Fix this?" on failed commands
- Explain output: right click → "Explain this" popup
- AI history search: Ctrl+R → describe command in English

### 9. ENHANCED ACTIVITY STREAM
- Markdown rendered (headers, bold, lists, tables)
- Code blocks with syntax highlighting + copy button
- Images inline (screenshots)
- Artifacts panel: files, screenshots, code
- Agent timeline (right side): visual step tracker
- Performance metrics: tokens/sec, cost breakdown, timing

---

## UPCOMING (DAY 6)

### MODELS TAB
- Provider management dashboard
- All cloud providers + local Ollama/LM Studio
- Usage bars, cost tracking, model lists
- Global model picker used everywhere

### SETTINGS PAGE
- General, Voice, Agents, Terminal, Connections, Storage, About

### SYSTEM TRAY
- Fox logo in tray
- Right-click menu: Show/New Chat/New Agent/New Terminal/Quit
- Badge when agents running

### COMMAND PALETTE (Ctrl+K)
- Fuzzy search all commands
- Keyboard navigable

### PACKAGING
- Windows NSIS installer (.exe)
- App icon from fox logo
- Version 1.0.0

---

## ENGINEERING RULES (never break these)

### ARCHITECTURE
- Tauri 2 + Vite vanilla ES modules (no framework)
- Rust backend: tokio async, thiserror, tracing
- Frontend: ES modules, marked+dompurify, highlight.js, xterm.js
- Storage: localStorage for all data (no SQLite yet)
- Keys: localStorage btoa encoded (keychain in v2)

### THE UNTOUCHABLE RULE
Chat tab, Voice card, Terminal tab, Agents tab, Connections tab,
MCP tab — ALL existing features stay 100% untouched.
New features are self-contained: own CSS block, own script section,
own Rust commands appended before main().

### CODE STYLE
- New Rust commands: append before main(), register in handler
- New JS: own <script> module, can access shared globals
- New CSS: own block with clear comment header
- API keys never logged or exposed
- PathGuard: file tools always scoped to working directory

### PATHS (Windows, user is "praga")
- App: C:\Users\praga\KITE-APP
- Whisper: C:\Users\praga\whisper-app\server.py
- Whisper Python: C:\Users\praga\whisper-app\whisper-env\Scripts\python.exe
- Whisper model: faster-whisper-large-v3-turbo-ct2

### PROVIDERS AVAILABLE
Groq (key saved), Gemini, Mistral, Cerebras, OpenRouter,
NVIDIA NIM, Cloudflare Workers AI, GitHub Models, HuggingFace,
OpenAI, Anthropic, Ollama (local), LM Studio (local)

### VOICE PIPELINE
1. Wake word OR F9/mic button
2. Record (click start → click stop, min 2 seconds)
3. POST to http://127.0.0.1:5000/transcribe (local whisper first)
   OR Groq /openai/v1/audio/transcriptions (fallback)
4. Send transcript to AI voice parser → get JSON command
5. Execute command in current context
6. Speak confirmation via window.speechSynthesis
7. After agent done: announce result automatically

### MCP SERVERS
- Spawn via Tauri shell as sidecar processes
- JSON-RPC over stdio (initialize → tools/list → tools/call)
- Auto-restart on crash max 3 times
- Kill all on app exit

### AGENT LOOP (JS)
- ReAct pattern: think → tool call → observe → repeat
- Uses existing kiteAsk() / provider chain
- Parallel tool calls within one turn (Promise.all)
- Budget: check before every iteration
- Events: emit to activity stream in real time
- Approval: pause → show UI → resolve → continue

### STYLING
- CSS vars: --bg, --surface, --teal, --coral, --text, --dim
- Fonts: Inter (UI), Space Mono (code/terminal)
- Dark default, light mode toggle
- Teal (#00d4aa) for active/success states
- Coral (#ff6b6b) for errors/warnings/recording
- All logos: cdn.simpleicons.org/{name}/{color}
- Consistent: 8px border radius, subtle shadows, smooth transitions

---

## TESTING CHECKLIST (verify after each feature)

- [ ] Chat still streams replies
- [ ] API keys tab saves/loads keys
- [ ] Terminal opens PowerShell, commands work
- [ ] Agent runs and completes a web fetch task
- [ ] Voice card appears, records, transcribes
- [ ] No console errors on boot
- [ ] All tabs switch correctly
- [ ] Voice command "Hey Kite" activates card
- [ ] Coordinator spawns worker agents in parallel
- [ ] Screenshot tool works in agent
- [ ] Drag + drop file into chat adds context
- [ ] Projects view shows/creates projects

---

## CURRENT STATUS

Day 1 ✅ — Tauri wrap + Kite Voice card
Day 2 ✅ — Real terminal (PTY, tabs, splits)
Day 3 ✅ — Agents (parallel, subagents, tools, voice)
Day 4 ✅ — Connections (GitHub working) + MCP (Memory + SeqThinking)
Day 5 🔄 — Voice everywhere, Coordinator, Screenshots, Code interpreter,
            Drag-drop, Projects, Memory, Smart terminal
Day 6 📋 — Models tab, Settings, Tray, Command palette, Package
