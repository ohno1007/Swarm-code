import "./style.css";
import { call, send as apiSend } from "./api.js";
import { marked } from "marked";

const MODELS = [
  { id: "deepseek-chat", label: "DeepSeek Chat" },
  { id: "deepseek-reasoner", label: "DeepSeek Reasoner（推理）" },
];
const LEVELS = [
  { label: "低", temp: 0.0 },
  { label: "中", temp: 0.4 },
  { label: "高", temp: 0.8 },
];

const SUGGESTIONS = [
  { icon: "doc", text: "解读这个项目的整体结构" },
  { icon: "bug", text: "找出代码里的潜在 Bug" },
  { icon: "test", text: "为核心模块写单元测试" },
  { icon: "code", text: "重构这段代码并解释" },
  { icon: "play", text: "运行测试并修复失败项" },
];

// ---- icons ----------------------------------------------------------------
const I = {
  spark: `<svg class="spark" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2c.3 3.2 1.4 5.1 3 6.6 1.6 1.5 3.6 2.1 6.9 2.4-3.3.3-5.3.9-6.9 2.4-1.6 1.5-2.7 3.4-3 6.6-.3-3.2-1.4-5.1-3-6.6C7.4 11.9 5.4 11.3 2 11c3.4-.3 5.4-.9 7-2.4 1.6-1.5 2.7-3.4 3-6.6Z"/></svg>`,
  plus: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>`,
  chats: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M21 12a8 8 0 0 1-11.5 7.2L4 20l1-4.5A8 8 0 1 1 21 12Z"/></svg>`,
  gear: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-2.7 1.1V21a2 2 0 1 1-4 0v-.1A1.6 1.6 0 0 0 7 19.4a1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0-1.1-2.7H1a2 2 0 1 1 0-4h.1A1.6 1.6 0 0 0 2.6 7a1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.6 1.6 0 0 0 7 2.6h.1A1.6 1.6 0 0 0 9 1.1V1a2 2 0 1 1 4 0v.1A1.6 1.6 0 0 0 15 2.6a1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8v.1a1.6 1.6 0 0 0 1.5 1H23a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1Z"/></svg>`,
  send: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>`,
  chevron: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="m6 9 6 6 6-6"/></svg>`,
  tool: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14.7 6.3a4 4 0 0 0-5.2 5l-6.1 6.1a1.5 1.5 0 0 0 2.1 2.1l6.1-6.1a4 4 0 0 0 5-5.2l-2.4 2.4-2.1-2.1 2.5-2.2Z"/></svg>`,
  doc: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>`,
  bug: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><rect x="8" y="6" width="8" height="12" rx="4"/><path d="M3 9h3M18 9h3M3 15h3M18 15h3M12 2v4"/></svg>`,
  test: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M9 3h6M10 3v6l-5 9a2 2 0 0 0 2 3h10a2 2 0 0 0 2-3l-5-9V3"/></svg>`,
  code: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="m8 9-3 3 3 3M16 9l3 3-3 3"/></svg>`,
  play: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M7 5v14l11-7z"/></svg>`,
};

function greet() {
  const h = new Date().getHours();
  if (h < 6) return "凌晨好";
  if (h < 12) return "早上好";
  if (h < 14) return "中午好";
  if (h < 18) return "下午好";
  return "晚上好";
}

const state = {
  sessionId: null,
  busy: false,
  model: "deepseek-chat",
  temp: 0.2,
  usage: [0, 24000],
  live: {},
  pendingTool: {},
  inConversation: false,
};

const app = document.getElementById("app");
app.innerHTML = `
  <div class="rail">
    <div class="logo">${I.spark}</div>
    <button class="icon" id="r-new" title="新建会话">${I.plus}</button>
    <button class="icon" id="r-chats" title="会话列表">${I.chats}</button>
    <div class="spacer"></div>
    <button class="icon" id="r-settings" title="设置">${I.gear}</button>
  </div>
  <div class="main">
    <div class="topbar">
      <span class="ws" id="ws" title="点击修改工作区"></span>
      <div class="spacer"></div>
      <span class="ctx-pill" id="ctx"><svg class="mini" viewBox="0 0 36 36"><circle cx="18" cy="18" r="15" fill="none" stroke="#e7e5db" stroke-width="5"/><circle id="ctx-ring" cx="18" cy="18" r="15" fill="none" stroke="#5a9c6b" stroke-width="5" stroke-linecap="round" transform="rotate(-90 18 18)"/></svg><span id="ctx-text">上下文 0%</span></span>
    </div>
    <div class="content">
      <div class="home" id="home">
        <div class="inner">
          <div class="greeting serif">${I.spark}<span id="hello">${greet()}</span></div>
          <div id="home-composer"></div>
          <div class="chips" id="chips"></div>
        </div>
      </div>
      <div class="conversation" id="conversation">
        <div class="transcript" id="transcript"><div class="thread" id="thread"></div></div>
        <div class="composer-wrap" id="conv-composer"></div>
      </div>
    </div>
    <div class="drawer" id="drawer">
      <div class="dh"><span>会话</span><button class="icon" id="d-new" title="新建">${I.plus}</button></div>
      <div class="list" id="sessions"></div>
    </div>
  </div>

  <div class="backdrop" id="backdrop">
    <div class="modal">
      <h2>设置</h2>
      <div class="field">
        <label>DeepSeek API 密钥</label>
        <input id="key" type="password" placeholder="sk-..." />
        <div class="hint" id="key-hint"></div>
      </div>
      <div class="field">
        <label>工作区文件夹</label>
        <input id="workspace" placeholder="例如 C:\\Users\\you\\project" />
        <div class="hint">智能体读取与修改的项目文件夹。</div>
      </div>
      <div class="actions">
        <button class="btn" id="s-cancel">取消</button>
        <button class="btn primary" id="s-save">保存</button>
      </div>
    </div>
  </div>
`;

const $ = (id) => document.getElementById(id);
const thread = $("thread");

// ---- composer -------------------------------------------------------------

function buildComposer() {
  const el = document.createElement("div");
  el.className = "composer";
  el.innerHTML = `
    <textarea rows="1" placeholder="今天有什么可以帮你的？（Enter 发送，Shift+Enter 换行）"></textarea>
    <div class="composer-bottom">
      <button class="iconbtn add" title="附加">${I.plus}</button>
      <div class="spacer"></div>
      <button class="menu-btn model">${modelLabel()} ${I.chevron}</button>
      <button class="menu-btn level">强度：${levelLabel()} ${I.chevron}</button>
      <button class="send" disabled>${I.send}</button>
    </div>`;
  const ta = el.querySelector("textarea");
  const sendBtn = el.querySelector(".send");
  const grow = () => {
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 220) + "px";
    sendBtn.disabled = state.busy || !ta.value.trim();
  };
  ta.addEventListener("input", grow);
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit(ta.value);
    }
  });
  sendBtn.addEventListener("click", () => submit(ta.value));
  el.querySelector(".model").addEventListener("click", (e) => openModelMenu(e.currentTarget));
  el.querySelector(".level").addEventListener("click", (e) => openLevelMenu(e.currentTarget));
  el._ta = ta;
  el._refresh = () => {
    el.querySelector(".model").innerHTML = `${modelLabel()} ${I.chevron}`;
    el.querySelector(".level").innerHTML = `强度：${levelLabel()} ${I.chevron}`;
    grow();
  };
  return el;
}

let composer = buildComposer();
$("home-composer").appendChild(composer);

function modelLabel() {
  return (MODELS.find((m) => m.id === state.model) || MODELS[0]).label;
}
function levelLabel() {
  let best = LEVELS[0];
  for (const l of LEVELS) if (Math.abs(l.temp - state.temp) < Math.abs(best.temp - state.temp)) best = l;
  return best.label;
}
function focusComposer() {
  composer._ta.focus();
}

// ---- popup menu -----------------------------------------------------------

function openMenu(anchor, items, onPick) {
  closeMenu();
  const r = anchor.getBoundingClientRect();
  const pop = document.createElement("div");
  pop.className = "popup";
  pop.innerHTML = items
    .map((it, i) => `<div class="item ${it.sel ? "sel" : ""}" data-i="${i}"><span>${it.label}</span>${it.sub ? `<small>${it.sub}</small>` : ""}</div>`)
    .join("");
  document.body.appendChild(pop);
  pop.style.left = Math.min(r.left, window.innerWidth - pop.offsetWidth - 12) + "px";
  pop.style.top = r.top - pop.offsetHeight - 6 + "px";
  pop.querySelectorAll(".item").forEach((node) =>
    node.addEventListener("click", () => {
      onPick(Number(node.dataset.i));
      closeMenu();
    })
  );
  window._menu = pop;
}
function closeMenu() {
  if (window._menu) {
    window._menu.remove();
    window._menu = null;
  }
}
document.addEventListener("click", (e) => {
  if (window._menu && !window._menu.contains(e.target) && !e.target.closest(".menu-btn")) closeMenu();
});

function openModelMenu(anchor) {
  openMenu(
    anchor,
    MODELS.map((m) => ({ label: m.label, sel: m.id === state.model })),
    async (i) => {
      state.model = MODELS[i].id;
      composer._refresh();
      if (state.sessionId) await call("set_model", { id: state.sessionId, model: state.model });
    }
  );
}
function openLevelMenu(anchor) {
  openMenu(
    anchor,
    LEVELS.map((l) => ({ label: l.label, sub: "t=" + l.temp.toFixed(1), sel: l.label === levelLabel() })),
    async (i) => {
      state.temp = LEVELS[i].temp;
      composer._refresh();
      if (state.sessionId) await call("set_temp", { id: state.sessionId, temp: state.temp });
    }
  );
}

// ---- rendering ------------------------------------------------------------

function atBottom() {
  const t = $("transcript");
  return t.scrollHeight - t.scrollTop - t.clientHeight < 100;
}
function scrollDown(force) {
  const t = $("transcript");
  if (force || atBottom()) t.scrollTop = t.scrollHeight;
}
function addInfo(text) {
  const d = document.createElement("div");
  d.className = "info";
  d.textContent = text;
  thread.appendChild(d);
  scrollDown(true);
}
function userBubble(text) {
  const w = document.createElement("div");
  w.className = "msg user";
  w.innerHTML = `<div class="bubble"></div>`;
  w.querySelector(".bubble").textContent = text;
  thread.appendChild(w);
  scrollDown(true);
}
function assistantBubble(agent, depth) {
  const w = document.createElement("div");
  w.className = "msg assistant" + (depth > 0 ? " worker" : "");
  w.innerHTML = `${depth > 0 ? `<div class="who">${agent}</div>` : `<div class="who">Swarm-code</div>`}<div class="bubble"></div>`;
  w._raw = "";
  thread.appendChild(w);
  return w;
}

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
    card.innerHTML = `<div class="row">${I.tool}<span class="name"></span><span class="args"></span></div>`;
    card.querySelector(".name").textContent = event.name;
    card.querySelector(".args").textContent = event.args || "";
    thread.appendChild(card);
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
    addInfo(`… 已压缩 ${event.summarized} 条较早的消息`);
  }
}

function renderCtx(used, max) {
  const frac = max ? Math.min(used / max, 1) : 0;
  const r = 15, circ = 2 * Math.PI * r;
  const ring = $("ctx-ring");
  ring.style.strokeDasharray = `${circ}`;
  ring.style.strokeDashoffset = `${circ * (1 - frac)}`;
  ring.style.stroke = frac < 0.6 ? "#5a9c6b" : frac < 0.85 ? "#d6a531" : "#c0573f";
  $("ctx-text").textContent = `上下文 ${Math.round(frac * 100)}%`;
}

function renderChips() {
  $("chips").innerHTML = "";
  for (const s of SUGGESTIONS) {
    const c = document.createElement("button");
    c.className = "chip";
    c.innerHTML = `${I[s.icon]}<span>${s.text}</span>`;
    c.addEventListener("click", () => {
      composer._ta.value = s.text;
      composer._refresh();
      focusComposer();
    });
    $("chips").appendChild(c);
  }
}

// ---- conversation flow ----------------------------------------------------

function enterConversation() {
  if (state.inConversation) return;
  state.inConversation = true;
  $("home").classList.add("hide");
  $("conversation").classList.add("show");
  $("ctx").classList.add("show");
  $("conv-composer").appendChild(composer); // move the same composer down
  composer._refresh();
}
function enterHome() {
  state.inConversation = false;
  $("home").classList.remove("hide");
  $("conversation").classList.remove("show");
  $("ctx").classList.remove("show");
  thread.innerHTML = "";
  $("home-composer").appendChild(composer);
  composer._refresh();
  focusComposer();
}

async function submit(raw) {
  const text = (raw || "").trim();
  if (!text || state.busy || !state.sessionId) return;
  composer._ta.value = "";
  enterConversation();
  composer._refresh();
  userBubble(text);
  state.busy = true;
  state.live = {};
  try {
    await apiSend(state.sessionId, text, handleMsg);
  } catch (e) {
    addInfo("出错：" + e);
  }
  state.busy = false;
  composer._refresh();
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
    el.innerHTML = `<div>${s.title}</div><div class="sub">${s.turns} 条消息 · ${s.id.slice(0, 8)}</div>`;
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
    renderCtx(st.used, st.max);
    composer._refresh();
  } catch {}
  await refreshSessions();
}

function selectSession(id) {
  state.sessionId = id;
  $("drawer").classList.remove("open");
  enterConversation();
  thread.innerHTML = "";
  addInfo("已切换到该会话，新消息将显示在这里。");
  refreshStatus();
}

async function newSession() {
  try {
    const id = await call("create_session", { title: "会话" });
    state.sessionId = id;
    $("drawer").classList.remove("open");
    enterHome();
    await refreshStatus();
  } catch (e) {
    addInfo("出错：" + e);
  }
}

// ---- settings -------------------------------------------------------------

async function openSettings() {
  const st = await call("app_status");
  $("ws").textContent = st.workspace;
  $("workspace").value = st.workspace;
  $("key-hint").textContent = st.maskedKey ? `当前：${st.maskedKey}` : "尚未设置密钥 — 聊天前必须填写。";
  $("key").value = "";
  $("backdrop").classList.add("show");
}
async function saveSettings() {
  const key = $("key").value.trim();
  const ws = $("workspace").value.trim();
  try {
    if (ws) $("ws").textContent = (await call("set_workspace", { path: ws })).workspace;
    if (key) await call("set_key", { key });
    $("backdrop").classList.remove("show");
    if (!state.sessionId) await newSession();
    else await refreshStatus();
  } catch (e) {
    $("key-hint").textContent = "出错：" + e;
  }
}

// ---- wire up --------------------------------------------------------------

$("r-new").addEventListener("click", newSession);
$("d-new").addEventListener("click", newSession);
$("r-chats").addEventListener("click", () => {
  $("drawer").classList.toggle("open");
  refreshSessions();
});
$("r-settings").addEventListener("click", openSettings);
$("ws").addEventListener("click", openSettings);
$("s-cancel").addEventListener("click", () => $("backdrop").classList.remove("show"));
$("s-save").addEventListener("click", saveSettings);
$("backdrop").addEventListener("click", (e) => {
  if (e.target === $("backdrop")) $("backdrop").classList.remove("show");
});

async function init() {
  renderChips();
  renderCtx(0, 24000);
  const st = await call("app_status");
  $("ws").textContent = st.workspace;
  if (!st.hasKey) {
    openSettings();
  } else {
    await newSession();
    focusComposer();
  }
}

init();
