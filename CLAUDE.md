KITE — AI Agent Platform

CLAUDE.md — Read this fully before touching any code.


WHAT KITE IS

Kite is a Rust + Tauri 2 desktop AI agent platform.
Simple, powerful, focused.

Fox logo: src/assets/logo.png — use everywhere.


CURRENT STATE (what exists right now)

TABS:


Chat
Agents
Swarm      — pseudo-tab 12, live mission canvas (kiteSwarm)
Connections
MCP

Models & Keys and Settings live in the Settings modal (gear / Ctrl+,), not as tabs.


REMOVED (do not add back):


Terminal tab
Projects view
Workflow canvas
PTY / xterm.js code


TERMINAL — REMOVED AGAIN (2026-07-16)

A real PTY terminal (portable-pty + xterm.js panes) was built and then removed
at the user's request: it made the machine lag badly. Do not rebuild it without
being asked directly. If it is ever asked for again, the things that cost time:
  - This app does NOT set withGlobalTauri, so `window.__TAURI__` does not exist.
    Use window.__TAURI_INTERNALS__.invoke / window.kiteListen, or import from
    @tauri-apps/api/core and /event inside a module script.
  - The parent must drop(pair.slave) after spawn or reads never see EOF.
  - Panes are expensive: a PTY child + an xterm renderer each. That is the lag.

Note: `window.kiteTerm` is still the OLD terminal-styled chat (kterm-*, driven by
agent_shell_exec, reachable via voice/palette on tab 8). It is not a real
terminal and has no PTY. Leave it alone.



WHAT IS BUILT AND WORKING

CHAT TAB


10+ AI providers streaming
Groq, Gemini, Mistral, Cerebras, OpenRouter, NVIDIA,
Cloudflare, GitHub Models, HuggingFace, OpenAI, Anthropic
Key rotation across 4 slots per provider
Daily budget tracking and usage charts
Markdown rendering with syntax highlighting
localStorage persistence
Model picker


KITE VOICE CARD


Floating draggable card with fox logo
Always on top, snaps to edges, position persisted
Double-click minimizes to bubble
States: idle / recording / transcribing / thinking / speaking / error
F9 global hotkey
Click to start / click to stop recording
STT: Groq whisper-large-v3 OR local faster-whisper
Local server: C:\Users\praga\whisper-app\server.py
Python: C:\Users\praga\whisper-app\whisper-env\Scripts\python.exe
TTS: Gemini 2.5 Flash TTS with 4 key rotation OR speechSynthesis fallback
Animated rings, level bars
Voice commands parsed by AI and executed
After agent finishes: TTS announces result
Wake word: "Hey Kite"


AGENTS TAB


Sessions list: status, name, task, engine, model, cost
New Agent form: name, goal, engine, model, tools, budget
Agent detail: activity stream, approvals, message input
JS ReAct loop using existing provider chain
Basic tools: web_fetch, file_read, file_write, shell_exec, spawn_subagent
Approval flow: inline Approve/Deny
Budget: max iterations, tokens, cost
Subagents indented under parent
TTS announces completion


CONNECTIONS TAB


GitHub (tested working)
Gmail, Google Drive, Notion, Cloudflare, Linear,
Vercel, Figma, Supabase, Discord, Twitter, Reddit, YouTube
Real logos via cdn.simpleicons.org
localStorage token storage
Read: auto | Write: approval


MCP TAB


Memory (running, tested working)
Sequential Thinking (running, tested working)
Filesystem, Fetch, Puppeteer, Playwright
Exa, Firecrawl, Context7, Chrome DevTools
Brave Search, Git, Shell, SQLite, PDF
Custom server add form
Claude Desktop config import



WHAT WE ARE BUILDING NOW

GOAL: Make the agent able to do ANYTHING.


1. AGENT TOOLS — FULL EXPANSION

WEB & RESEARCH:


web_fetch(url) — already exists
web_search(query) — DuckDuckGo top 10
web_screenshot(url) — Puppeteer MCP
web_scrape(url, selector?) — extract elements
web_monitor(url, interval_mins) — watch changes


FILES & SYSTEM:


file_read, file_write [approval] — already exist
file_append(path, content) [approval]
file_delete(path) [approval]
file_list(dir)
file_search(dir, pattern)
file_move(from, to) [approval]
file_zip(files[], output)
file_unzip(path, dest)


CODE EXECUTION:


run_python(code, packages[]?) — auto pip install
run_javascript(code) — Node.js
run_powershell(script) [approval]
run_command(cmd, cwd?) [approval]


DATA & ANALYSIS:


parse_csv(path)
parse_json(path)
parse_pdf(path) — via PDF MCP
parse_excel(path)
generate_chart(data, type, title) — returns PNG inline


MEMORY & KNOWLEDGE:


memory_save(key, value) — Memory MCP
memory_recall(query)
memory_list()
note_create(title, content)
note_search(query)


AI & GENERATION:


ai_call(prompt, model?)
ai_summarize(text, length?)
ai_translate(text, language)
ai_extract(text, schema)
ai_classify(text, categories[])
ai_image_analyze(image_path)
ai_generate_report(data, template?)


COMMUNICATION (via Connections):


email_send(to, subject, body) [approval]
email_read(count?, query?)
discord_send(channel_id, message) [approval]
discord_read(channel_id, count)
slack_send(channel, message) [approval]
slack_read(channel, count)
github_create_issue(repo, title, body) [approval]
github_create_pr(repo, title, body, branch) [approval]
notion_create_page(title, content) [approval]
linear_create_issue(title, body) [approval]
twitter_post(text) [approval]
telegram_send(chat_id, text) [approval]


PRODUCTIVITY:


calendar_read(days?)
calendar_create(title, time, duration) [approval]
sheets_read(id, range)
sheets_write(id, range, data) [approval]
airtable_list(base, table)
airtable_create(base, table, fields) [approval]
stripe_get_balance()
stripe_list_payments(count)
stripe_get_revenue(period)


CREATIVE:


image_generate(prompt, model?) — Replicate MCP, shown inline
document_create(title, content, format)



2. AGENT ACTIVITY STREAM — VISUAL OVERHAUL

No emojis. Clean text badges:

[THINKING] — dim italic
[TOOL] — teal pill, tool name + condensed args
[RESULT] — dark bg, dim text
[APPROVAL] — coral card, Approve/Deny buttons
[SUBAGENT] — indented, purple left border
[IMAGE] — inline image max 400px
[CODE] — syntax highlighted + copy button
[FILE] — file card: name, size, preview
[CHART] — inline chart
[DONE] — teal, task complete + summary
[ERROR] — coral, message + Retry button

Rich content:


Tables → HTML tables
JSON → pretty printed
Markdown → fully rendered
Images → inline
Code → language label + copy
Timestamps on every step (HH:MM:SS)
Collapsible tool call blocks
Retry button on failed steps



3. AGENT PRESETS

Horizontal scroll row top of Agents tab.
Clean cards, no emojis, service icon + name.

"Research Anything" — web_search + web_fetch + ai_summarize + file_write
"Scrape and Analyze" — web_scrape + parse_csv + run_python + generate_chart
"Email Assistant" — email_read + ai_summarize + email_send
"GitHub Assistant" — github tools + ai_call
"Daily Briefing" — web_search + email_read + calendar_read + discord_send
"Image Generator" — image_generate + file_write
"Data Analyst" — file_read + parse_csv + run_python + generate_chart
"Code Assistant" — run_python + run_javascript + file_read + file_write
"Stripe Dashboard" — stripe_get_balance + stripe_list_payments + generate_chart
"Social Monitor" — twitter + reddit + web_search

Each preset card:


Name
Short description (one line)
Tool count badge
Click: opens New Agent form pre-filled



4. NEW AGENT FORM — TOOL CARDS

Replace plain checkboxes with tool cards.

BUILT-IN TOOLS — 2 column card grid:
Each card: tool name (bold) + description + AUTO/ASK badge + toggle

CONNECTIONS section — expandable cards:


Real logo + service name + tool count
Master toggle (all tools)
Expand → individual tool toggles
Disconnected = greyed + Connect button


MCP SERVERS section — expandable cards:


Server name + tool count + status dot
Master toggle
Expand → individual tool toggles
Stopped = greyed + Start button


Style: teal left border when enabled, smooth toggles


5. AGENT SESSIONS LIST

Group by status:
RUNNING (teal header)
WAITING APPROVAL (amber header)
DONE TODAY (green header)
OLDER (dim header)

Each card:


Status dot (pulse when running)
Agent name bold
Task preview 1 line
Engine badge: API / LOCAL / AUTO
Model name dim
Live cost (updates every 5s)
Progress bar toward budget
Elapsed time
Cancel X button (running only)


Subagents:


Indented 16px
Smaller card
Left border line connecting to parent



6. NEW CONNECTIONS

Google Calendar — cdn.simpleicons.org/googlecalendar/4285F4


calendar_list_events, calendar_create_event [approval],
calendar_find_free_time, calendar_delete_event [approval]


Google Sheets — cdn.simpleicons.org/googlesheets/34A853


sheets_read, sheets_write [approval], sheets_append [approval]


Airtable — cdn.simpleicons.org/airtable/18BFFF


list_records, create_record [approval], update_record [approval]


Trello — cdn.simpleicons.org/trello/0052CC


list_boards, list_cards, create_card [approval], move_card [approval]


Telegram — cdn.simpleicons.org/telegram/26A5E4


send_message [approval], get_updates


Stripe — cdn.simpleicons.org/stripe/635BFF


get_balance, list_payments, get_revenue,
list_customers, create_payment_link [approval]


Spotify — cdn.simpleicons.org/spotify/1DB954


now_playing, recently_played, search, play [approval]


PostHog — cdn.simpleicons.org/posthog/F54E00


get_events, get_insights, list_feature_flags


Railway — cdn.simpleicons.org/railway/0B0D0E


list_projects, list_deployments, get_logs, redeploy [approval]


WhatsApp Business — cdn.simpleicons.org/whatsapp/25D366


send_message [approval]


Connections UI additions:


Category filter: All | Productivity | Communication | Development | Analytics | Media
Search bar
"N of M connected" pill
Sort: connected first



7. NEW MCP SERVERS

Perplexity Search — needs PERPLEXITY_API_KEY
command: npx -y @modelcontextprotocol/server-perplexity
Tools: search, search_with_sources

Docker — no key
command: npx -y @modelcontextprotocol/server-docker
Tools: list_containers, start [approval], stop [approval], get_logs, list_images

Obsidian — needs vault path
command: npx -y @modelcontextprotocol/server-obsidian
Tools: read_note, create_note, search_notes, list_notes

Image Generation (Replicate) — needs REPLICATE_API_KEY
command: npx -y @modelcontextprotocol/server-replicate
Tools: generate_image, list_models

Maps — needs GOOGLE_MAPS_KEY
command: npx -y @modelcontextprotocol/server-maps
Tools: geocode, reverse_geocode, directions, places_search

Weather — needs OPENWEATHER_API_KEY (free)
command: npx -y @modelcontextprotocol/server-weather
Tools: current_weather, forecast, weather_alerts

Crypto — no key
command: npx -y @modelcontextprotocol/server-crypto
Tools: get_price, get_trending, get_news

AWS — needs ACCESS_KEY + SECRET_KEY
command: npx -y @modelcontextprotocol/server-aws
Tools: list_s3_buckets, list_ec2_instances, get_cloudwatch_metrics

Excel/Office — no key
command: npx -y @modelcontextprotocol/server-office
Tools: read_excel, write_excel, read_word, create_word, create_powerpoint


8. DISCORD RICH PRESENCE

Show Kite activity on Discord profile.

States:


Idle: Details "Kite AI" / State "Ready"
Running: Details "Running: [name]" / State "[tool] — [elapsed]"
Approval: State "Waiting for approval..."
Done (30s): Details "Completed: [name]" / State "Done in [time] — $[cost]"
Voice: State "Listening..."


Implementation:
Node.js sidecar (discord-rpc-sidecar.js):


npm package: discord-rpc
Reads JSON events from stdin
Updates Discord presence
Silent fail if Discord closed
Retry every 30s


Tauri spawns on startup, sends events via shell stdin.
Event: {type, name, tool, elapsed, cost}

Settings section "Discord":


Rich Presence On/Off toggle
Application ID input
(discord.com/developers/applications → New Application → copy ID)
Test button
Setup instructions inline



9. VOICE + AGENT PRESETS

"Hey Kite research [topic]" → Research preset
"Hey Kite scrape [URL]" → Scrape preset
"Hey Kite check my emails" → Email Assistant
"Hey Kite stripe revenue today" → Stripe Dashboard
"Hey Kite generate image of [description]" → Image Generator
"Hey Kite run daily briefing" → Daily Briefing
"Hey Kite cancel [name]" → cancel agent
"Hey Kite agent status" → speak running agents

After every agent done:
→ TTS: "[Name] finished. [One sentence result]"
→ Discord presence: completion state


ENGINEERING RULES

NEVER BREAK:


Chat tab streaming
Voice card appearance and behavior
API key storage and rotation
GitHub connection (tested working)
Memory + Sequential Thinking MCP (tested working)


ARCHITECTURE:


Tauri 2 + Vite vanilla ES modules
Rust: tokio, thiserror, tracing
JS: marked + dompurify + highlight.js
Storage: localStorage
Keys: localStorage btoa encoded


STYLE:


NO emojis anywhere
Clean text badges for all status
CSS vars: --bg --surface --teal --coral --text --dim
Fonts: Inter (UI) + Space Mono (code/mono)
Teal (#00d4aa) = active/success/running
Coral (#ff6b6b) = error/warning/approval
Logos: cdn.simpleicons.org/{name}/{color}
8px border radius, smooth transitions


NEW CODE:


Rust commands: append before main()
JS: own script module
CSS: own block with comment header
Never log API keys
Approval required for all write/execute tools


MACHINE:


Windows 11, username: praga
App: C:\Users\praga\KITE-APP
Whisper: C:\Users\praga\whisper-app\server.py



BUILD ORDER


Add all new agent tools (web, files, code, data, AI, comms)
Activity stream visual overhaul
Agent presets row
New agent form tool cards
Sessions list visual improvements
New connections
New MCP servers
Discord Rich Presence sidecar
Voice + preset integration
Full end-to-end test


Tell me after each step and wait before continuing.