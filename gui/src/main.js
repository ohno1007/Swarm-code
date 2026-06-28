import "./style.css";
import "material-symbols/outlined.css";

// Material 3 web components
import "@material/web/button/filled-button.js";
import "@material/web/button/filled-tonal-button.js";
import "@material/web/button/text-button.js";
import "@material/web/iconbutton/icon-button.js";
import "@material/web/textfield/outlined-text-field.js";
import "@material/web/slider/slider.js";
import "@material/web/dialog/dialog.js";
import "@material/web/chips/chip-set.js";
import "@material/web/chips/filter-chip.js";
import "@material/web/progress/circular-progress.js";

import { call, send as apiSend } from "./api.js";
import { marked } from "marked";

const MODELS = ["deepseek-chat", "deepseek-reasoner"];

const state = {
  sessionId: null,
  busy: false,
  model: "deepseek-chat",
  temp: 0.2,
  usage: [0, 24000],
  live: {}, // agent -> bubble element
  pendingTool: {}, // agent -> tool element
};

const app = document.getElementById("app");
app.innerHTML = `
  <div class="shell">
    <div class="appbar">
      <div class="logo">S</div>
      <div class="title">Swarm-code</div>
      <span class="ws" id="ws"></span>
      <div class="spacer"></div>
      <md-icon-button id="btn-new" title="New session"><span class="material-symbols-outlined">add</span></md-icon-button>
      <md-icon-button id="btn-settings" title="Settings"><span class="material-symbols-outlined">settings</span></md-icon-button>
    </div>
    <div class="body">
      <div class="rail">
        <div class="head">Sessions</div>
        <div class="list" id="sessions"></div>
      </div>
      <div class="chat">
        <div class="transcript" id="transcript"></div>
        <div class="composer">
          <md-outlined-text-field id="input" type="textarea" rows="1" placeholder="Message Swarm-code…  (Enter to send, Shift+Enter for newline)"></md-outlined-text-field>
          <md-filled-button id="send"><span class="material-symbols-outlined" slot="icon">send</span>Send</md-filled-button>
        </div>
      </div>
      <div class="rightpanel">
        <div>
          <div class="panel-title">Context memory</div>
          <div class="ring-wrap">
            <svg class="ring" viewBox="0 0 120 120">
              <circle class="track" cx="60" cy="60" r="52"></circle>
              <circle class="value" id="ring-val" cx="60" cy="60" r="52"></circle>
              <text class="ring-label" id="ring-label" x="60" y="60" text-anchor="middle" dominant-baseline="central">0%</text>
            </svg>
          </div>
          <div class="ring-cap" id="ring-cap">0 / 24k tokens</div>
        </div>
        <div class="control">
          <div class="panel-title">Model</div>
          <md-chip-set id="models"></md-chip-set>
        </div>
        <div class="control">
          <div class="label"><span>Thinking intensity</span><span id="temp-val">0.2</span></div>
          <md-slider id="temp" min="0" max="2" step="0.1" value="0.2" labeled></md-slider>
        </div>
      </div>
    </div>
  </div>

  <md-dialog id="settings">
    <div slot="headline">Settings</div>
    <form slot="content" id="settings-form" method="dialog">
      <div class="field">
        <md-outlined-text-field id="key" label="DeepSeek API key" type="password" style="width:100%"></md-outlined-text-field>
        <div class="hint" id="key-hint"></div>
      </div>
      <div class="field">
        <md-outlined-text-field id="workspace" label="Workspace folder" style="width:100%"></md-outlined-text-field>
        <div class="hint">The project folder the agents read and edit.</div>
      </div>
    </form>
    <div slot="actions">
      <md-text-button id="settings-cancel">Cancel</md-text-button>
      <md-filled-button id="settings-save">Save</md-filled-button>
    </div>
  </md-dialog>
`;

const $ = (id) => document.getElementById(id);
const transcript = $("transcript");

// ---- helpers --------------------------------------------------------------

function fmtK(n) {
  return n >= 1000 ? (n / 1000).toFixed(1) + "k" : "" + n;
}

function atBottom() {
  return transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight < 80;
}
function scrollDown(force) {
  if (force || atBottom()) transcript.scrollTop = transcript.scrollHeight;
}

function addInfo(text) {
  const d = document.createElement("div");
  d.className = "info";
  d.textContent = text;
  transcript.appendChild(d);
  scrollDown(true);
}

function userBubble(text) {
  const wrap = document.createElement("div");
  wrap.className = "msg user";
  wrap.innerHTML = `<div class="bubble"></div>`;
  wrap.querySelector(".bubble").textContent = text;
  transcript.appendChild(wrap);
  scrollDown(true);
}

function assistantBubble(agent, depth) {
  const wrap = document.createElement("div");
  wrap.className = "msg assistant" + (depth > 0 ? " worker" : "");
  const who = depth > 0 ? agent : "";
  wrap.innerHTML = `${who ? `<div class="who">${who}</div>` : ""}<div class="bubble"></div>`;
  wrap._raw = "";
  transcript.appendChild(wrap);
  return wrap;
}

function renderRing(used, max) {
  const frac = max ? Math.min(used / max, 1) : 0;
  const r = 52, circ = 2 * Math.PI * r;
  const val = $("ring-val");
  val.style.strokeDasharray = `${circ}`;
  val.style.strokeDashoffset = `${circ * (1 - frac)}`;
  const color = frac < 0.6 ? "var(--md-sys-color-success)" : frac < 0.85 ? "#e6c34a" : "var(--md-sys-color-error)";
  val.style.stroke = color;
  $("ring-label").textContent = Math.round(frac * 100) + "%";
  $("ring-cap").textContent = `${fmtK(used)} / ${fmtK(max)} tokens`;
}

function renderModels() {
  const set = $("models");
  set.innerHTML = "";
  for (const m of MODELS) {
    const chip = document.createElement("md-filter-chip");
    chip.label = m;
    chip.selected = m === state.model;
    chip.addEventListener("click", async () => {
      state.model = m;
      renderModels();
      if (state.sessionId) await call("set_model", { id: state.sessionId, model: m });
    });
    set.appendChild(chip);
  }
}

// ---- streaming ------------------------------------------------------------

function handleMsg(msg) {
  const { agent, depth, event } = msg;
  if (event.kind === "text") {
    let b = state.live[agent];
    if (!b) {
      b = assistantBubble(agent, depth);
      state.live[agent] = b;
    }
    b._raw += event.text;
    b.querySelector(".bubble").innerHTML = marked.parse(b._raw);
    scrollDown();
  } else if (event.kind === "tool_start") {
    delete state.live[agent];
    const card = document.createElement("div");
    card.className = "tool" + (depth > 0 ? " worker" : "");
    card.innerHTML = `
      <div class="row">
        <span class="ic">build</span>
        <span class="name"></span>
        <span class="args"></span>
      </div>`;
    card.querySelector(".name").textContent = event.name;
    card.querySelector(".args").textContent = event.args || "";
    transcript.appendChild(card);
    state.pendingTool[agent] = card;
    scrollDown();
  } else if (event.kind === "tool_end") {
    const card = state.pendingTool[agent];
    if (card) {
      const res = document.createElement("div");
      res.className = "result" + (event.ok ? "" : " err");
      res.textContent = (event.ok ? "✓ " : "✗ ") + event.preview;
      card.appendChild(res);
      delete state.pendingTool[agent];
    }
    scrollDown();
  } else if (event.kind === "compacted") {
    addInfo(`… compacted ${event.summarized} earlier messages`);
  }
}

async function sendMessage() {
  const field = $("input");
  const text = field.value.trim();
  if (!text || state.busy || !state.sessionId) return;
  field.value = "";
  userBubble(text);
  state.busy = true;
  state.live = {};
  $("send").disabled = true;

  try {
    await apiSend(state.sessionId, text, handleMsg);
  } catch (e) {
    addInfo("error: " + e);
  }
  state.busy = false;
  $("send").disabled = false;
  await refreshStatus();
}

// ---- sessions & status ----------------------------------------------------

async function refreshSessions() {
  let sessions = [];
  try {
    sessions = await call("list_sessions");
  } catch {
    return;
  }
  const list = $("sessions");
  list.innerHTML = "";
  for (const s of sessions) {
    const el = document.createElement("div");
    el.className = "session" + (s.id === state.sessionId ? " active" : "");
    el.innerHTML = `<div>${s.title}</div><div class="sub">${s.turns} msgs · ${s.id.slice(0, 8)}</div>`;
    el.addEventListener("click", () => selectSession(s.id));
    list.appendChild(el);
  }
}

async function refreshStatus() {
  if (!state.sessionId) return;
  try {
    const st = await call("session_status", { id: state.sessionId });
    state.model = st.model;
    state.temp = st.temp;
    state.usage = [st.used, st.max];
    $("temp").value = st.temp;
    $("temp-val").textContent = Number(st.temp).toFixed(1);
    renderModels();
    renderRing(st.used, st.max);
  } catch {}
  await refreshSessions();
}

function selectSession(id) {
  state.sessionId = id;
  transcript.innerHTML = "";
  addInfo("Switched session. New messages appear here.");
  refreshStatus();
}

async function newSession() {
  try {
    const id = await call("create_session", { title: "session" });
    state.sessionId = id;
    transcript.innerHTML = "";
    addInfo("New session ready. Ask me to explore or edit your project.");
    await refreshStatus();
  } catch (e) {
    addInfo("error: " + e);
  }
}

// ---- settings -------------------------------------------------------------

async function openSettings() {
  const status = await call("app_status");
  $("ws").textContent = status.workspace;
  $("workspace").value = status.workspace;
  $("key-hint").textContent = status.maskedKey
    ? `Current: ${status.maskedKey}`
    : "No key set — required to chat.";
  $("key").value = "";
  $("settings").show();
}

async function saveSettings() {
  const key = $("key").value.trim();
  const ws = $("workspace").value.trim();
  try {
    if (ws) {
      const resolved = await call("set_workspace", { path: ws });
      $("ws").textContent = resolved;
    }
    if (key) {
      await call("set_key", { key });
    }
    $("settings").close();
    if (!state.sessionId) await newSession();
    else await refreshStatus();
  } catch (e) {
    $("key-hint").textContent = "error: " + e;
  }
}

// ---- wire up --------------------------------------------------------------

$("send").addEventListener("click", sendMessage);
$("input").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendMessage();
  }
});
$("btn-new").addEventListener("click", newSession);
$("btn-settings").addEventListener("click", openSettings);
$("settings-cancel").addEventListener("click", () => $("settings").close());
$("settings-save").addEventListener("click", saveSettings);
$("temp").addEventListener("input", (e) => {
  $("temp-val").textContent = Number(e.target.value).toFixed(1);
});
$("temp").addEventListener("change", async (e) => {
  state.temp = Number(e.target.value);
  if (state.sessionId) await call("set_temp", { id: state.sessionId, temp: state.temp });
});

async function init() {
  renderModels();
  renderRing(0, 24000);
  const status = await call("app_status");
  $("ws").textContent = status.workspace;
  if (!status.hasKey) {
    addInfo("Welcome to Swarm-code. Add your DeepSeek API key in Settings to begin.");
    openSettings();
  } else {
    await newSession();
  }
}

init();
