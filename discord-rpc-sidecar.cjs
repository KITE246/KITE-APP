// discord-rpc-sidecar.js — zero-dependency Discord Rich Presence bridge for Kite.
//
//   node discord-rpc-sidecar.js <application_id>
//
// Speaks the Discord IPC protocol directly over the local named pipe / socket,
// so it needs NO npm packages (no discord-rpc install). Kite spawns it through
// the same Tauri MCP process plumbing (mcp_start / mcp_send) and pushes
// newline-delimited JSON events to it on stdin:
//
//   {"type":"idle"}
//   {"type":"running","name":"Research","tool":"web_search","startTs":1720000000000}
//   {"type":"approval","name":"Research","startTs":1720000000000}
//   {"type":"done","name":"Research","duration":"1m 12s","cost":"0.0031"}
//   {"type":"voice"}
//
// All diagnostics go to STDERR (stdout is kept empty so it never collides with
// the JSON-RPC correlation the MCP client does on mcp-stdout). If Discord isn't
// running it retries quietly every RECONNECT_MS.

'use strict';
const net = require('net');

const CLIENT_ID = (process.argv[2] || '').trim();
const RECONNECT_MS = 15000;
const OP = { HANDSHAKE: 0, FRAME: 1, CLOSE: 2, PING: 3, PONG: 4 };

let sock = null;         // connected socket, once handshake completes
let ready = false;       // Discord sent READY
let current = idleActivity(); // last activity we want shown
let nonceSeq = 0;
let reconnectTimer = null;
let inbuf = Buffer.alloc(0);

function err(msg){ try{ process.stderr.write('[discord-rpc] ' + msg + '\n'); }catch(_){} }
// One-line status back to Kite (ignored by the MCP client — no id/method field).
function status(s){ try{ process.stdout.write(JSON.stringify({ _kite_discord: s }) + '\n'); }catch(_){} }

if (!CLIENT_ID){
  err('no application id passed — exiting');
  process.exit(1);
}

/* ── pipe / socket path (tries discord-ipc-0 .. discord-ipc-9) ── */
function ipcPath(i){
  if (process.platform === 'win32') return '\\\\?\\pipe\\discord-ipc-' + i;
  const base = process.env.XDG_RUNTIME_DIR || process.env.TMPDIR || process.env.TMP || process.env.TEMP || '/tmp';
  return base.replace(/\/$/, '') + '/discord-ipc-' + i;
}

/* ── frame encode: [int32 op LE][int32 len LE][json body] ── */
function encode(op, data){
  const body = Buffer.from(JSON.stringify(data), 'utf8');
  const head = Buffer.alloc(8);
  head.writeInt32LE(op, 0);
  head.writeInt32LE(body.length, 4);
  return Buffer.concat([head, body]);
}

function send(op, data){
  if (!sock) return;
  try { sock.write(encode(op, data)); }
  catch(e){ err('write failed: ' + e.message); }
}

/* ── activity builders (Discord presence payloads) ── */
function baseAssets(){ return { large_image: 'kite', large_text: 'Kite' }; }
function idleActivity(){
  return { details: 'AI Terminal', state: 'Idle', assets: baseAssets(), instance: false };
}
function buildActivity(ev){
  const a = { assets: baseAssets(), instance: false };
  switch (ev.type){
    case 'running':
      a.details = 'Running agent: ' + clip(ev.name || 'agent', 118);
      a.state = clip(ev.tool ? String(ev.tool) : 'Working…', 128);
      if (ev.startTs) a.timestamps = { start: Math.floor(ev.startTs) };
      a.assets.small_image = 'running';
      a.assets.small_text = 'Running';
      break;
    case 'approval':
      a.details = 'Running agent: ' + clip(ev.name || 'agent', 118);
      a.state = 'Waiting for approval…';
      if (ev.startTs) a.timestamps = { start: Math.floor(ev.startTs) };
      a.assets.small_image = 'approval';
      a.assets.small_text = 'Waiting';
      break;
    case 'done':
      a.details = 'Agent complete: ' + clip(ev.name || 'agent', 118);
      a.state = 'Completed in ' + (ev.duration || '?') + ' — Cost: $' + (ev.cost != null ? ev.cost : '0');
      break;
    case 'voice':
      a.details = 'Kite Voice';
      a.state = 'Listening…';
      a.assets.small_image = 'running';
      a.assets.small_text = 'Listening';
      break;
    case 'idle':
    default:
      return idleActivity();
  }
  return a;
}
function clip(s, n){ s = String(s == null ? '' : s); return s.length > n ? s.slice(0, n - 1) + '…' : (s || ' '); }

/* ── push current activity to Discord ── */
function apply(){
  if (!ready || !sock) return;
  send(OP.FRAME, {
    cmd: 'SET_ACTIVITY',
    args: { pid: process.pid, activity: current },
    nonce: String(++nonceSeq)
  });
}

/* ── connection lifecycle ── */
function connect(i){
  if (i > 9){ err('no Discord IPC pipe found — is the Discord desktop app running? retrying in ' + (RECONNECT_MS/1000) + 's'); scheduleReconnect(); return; }
  if (i === 0) err('searching for Discord (app id ' + CLIENT_ID + ')…');
  const s = net.createConnection(ipcPath(i));
  let settled = false;
  s.once('connect', () => {
    settled = true;
    sock = s;
    inbuf = Buffer.alloc(0);
    send(OP.HANDSHAKE, { v: 1, client_id: CLIENT_ID });
  });
  s.once('error', () => {
    if (settled){ teardown(); return; }
    s.destroy();
    connect(i + 1); // try the next pipe index
  });
  s.on('data', onData);
  s.on('close', () => { teardown(); });
}

function teardown(){
  if (ready) status('waiting');
  ready = false;
  if (sock){ try{ sock.destroy(); }catch(_){} sock = null; }
  scheduleReconnect();
}

function scheduleReconnect(){
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => { reconnectTimer = null; connect(0); }, RECONNECT_MS);
}

/* ── parse inbound frames ── */
function onData(chunk){
  inbuf = Buffer.concat([inbuf, chunk]);
  while (inbuf.length >= 8){
    const op = inbuf.readInt32LE(0);
    const len = inbuf.readInt32LE(4);
    if (inbuf.length < 8 + len) break;
    const body = inbuf.slice(8, 8 + len);
    inbuf = inbuf.slice(8 + len);
    let msg = null;
    try { msg = JSON.parse(body.toString('utf8')); } catch(_){ msg = null; }
    handleFrame(op, msg);
  }
}
function handleFrame(op, msg){
  if (op === OP.PING){ send(OP.PONG, msg); return; }
  if (op === OP.CLOSE){ teardown(); return; }
  if (op === OP.FRAME && msg && msg.evt === 'READY'){
    ready = true;
    err('connected to Discord');
    status('connected');
    apply(); // flush whatever state Kite last set
  }
}

/* ── stdin: newline-delimited JSON events from Kite ── */
let linebuf = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => {
  linebuf += d;
  let nl;
  while ((nl = linebuf.indexOf('\n')) >= 0){
    const line = linebuf.slice(0, nl).trim();
    linebuf = linebuf.slice(nl + 1);
    if (!line) continue;
    let ev; try { ev = JSON.parse(line); } catch(_){ continue; }
    current = buildActivity(ev);
    apply();
  }
});
process.stdin.on('end', () => process.exit(0));
process.on('SIGTERM', () => process.exit(0));
process.on('SIGINT', () => process.exit(0));

connect(0);
