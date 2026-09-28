// Phone page of helm (specs/remote.md §7): agents list, terminal mirror, composer.
"use strict";

const HISTORY_PAGE = 200;
const RECONNECT_MS = 1500;
const FONT_MIN = 8;
const FONT_MAX = 20;

const $ = (id) => document.getElementById(id);

const state = {
  socket: null,
  agents: [],
  watched: null,
  writable: false,
  historyFirst: 0,
  historyDone: false,
  historyLoading: false,
  fontPx: loadFontPx(),
};

function loadFontPx() {
  try {
    const stored = Number(localStorage.getItem("helm.fontPx"));
    if (stored >= FONT_MIN && stored <= FONT_MAX) return stored;
  } catch (_) { /* private mode */ }
  return 12;
}

function saveFontPx() {
  try { localStorage.setItem("helm.fontPx", String(state.fontPx)); } catch (_) { /* private mode */ }
}

function escapeHtml(text) {
  return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

// Socket

function connect() {
  if (state.socket && state.socket.readyState <= WebSocket.OPEN) return;
  const socket = new WebSocket(`ws://${location.host}/ws`);
  state.socket = socket;
  socket.onopen = () => {
    $("link").hidden = true;
    if (state.watched !== null) send({ type: "watch", id: state.watched });
  };
  socket.onmessage = (event) => receive(JSON.parse(event.data));
  socket.onclose = () => {
    if (state.socket !== socket) return;
    state.socket = null;
    $("link").hidden = false;
    checkAccess().then((granted) => {
      if (granted) setTimeout(connect, RECONNECT_MS);
    });
  };
}

async function checkAccess() {
  try {
    const response = await fetch("/", { cache: "no-store" });
    if (response.status === 401) {
      showExpired();
      return false;
    }
  } catch (_) { /* Mac unreachable: keep retrying */ }
  return true;
}

function send(message) {
  if (state.socket && state.socket.readyState === WebSocket.OPEN) {
    state.socket.send(JSON.stringify(message));
  }
}

function receive(message) {
  switch (message.type) {
    case "agents": state.agents = message.agents; renderAgents(); renderHeader(); break;
    case "screen": if (message.id === state.watched) renderScreen(message); break;
    case "history": if (message.id === state.watched) prependHistory(message); break;
    case "ended": if (message.id === state.watched) endMirror(); break;
  }
}

// Agents list

function badge(kind) {
  return `<span class="badge ${kind}" aria-label="${kind}"></span>`;
}

function subtitle(agent) {
  return [agent.branch, agent.tab].filter(Boolean).map(escapeHtml).join(" · ");
}

function renderAgents() {
  const list = $("agents");
  if (state.agents.length === 0) {
    list.innerHTML = `<div class="empty">No agent running in helm</div>`;
    return;
  }
  let html = "";
  let project = null;
  for (const agent of state.agents) {
    if (agent.project !== project) {
      project = agent.project;
      html += `<div class="group">${escapeHtml(project)}</div>`;
    }
    html += `<button class="row" type="button" data-id="${agent.id}">
      <span class="who"><div class="name">${escapeHtml(agent.agent)}</div>
      <div class="sub">${subtitle(agent)}</div></span>${badge(agent.badge)}</button>`;
  }
  list.innerHTML = html;
}

// Terminal mirror

function openMirror(id) {
  state.watched = id;
  state.writable = false;
  resetHistory();
  $("screen").innerHTML = "";
  $("notice").hidden = true;
  $("agents-view").hidden = true;
  $("terminal-view").hidden = false;
  renderHeader();
  send({ type: "watch", id });
}

function closeMirror() {
  state.watched = null;
  $("terminal-view").hidden = true;
  $("agents-view").hidden = false;
}

function endMirror() {
  state.writable = false;
  showNotice("The terminal was closed on the Mac.");
  setDockWritable(false);
}

function renderHeader() {
  if (state.watched === null) return;
  const agent = state.agents.find((a) => a.id === state.watched);
  if (!agent) {
    $("term-badge").innerHTML = "";
    return;
  }
  $("term-title").textContent = `${agent.agent} · ${agent.project}`;
  $("term-subtitle").innerHTML = subtitle(agent);
  $("term-badge").innerHTML = badge(agent.badge);
}

function lineHtml(runs, cursorCol) {
  let html = "";
  let col = 0;
  for (const run of runs) {
    const style = `color:${run.fg};background:${run.bg}` +
      (run.bold ? ";font-weight:600" : "") +
      (run.italic ? ";font-style:italic" : "") +
      (run.underline ? ";text-decoration:underline" : "");
    html += `<span style="${style}">${escapeHtml(run.t)}</span>`;
    col += [...run.t].length;
  }
  if (cursorCol !== null && cursorCol >= col) {
    html += " ".repeat(cursorCol - col) + `<span class="cursor"> </span>`;
  }
  return `<div>${html}</div>`;
}

function isAtBottom(scroller) {
  return scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 24;
}

function renderScreen(frame) {
  const scroller = $("scroller");
  const stick = isAtBottom(scroller);
  scroller.style.setProperty("--term-bg", frame.bg);
  scroller.style.setProperty("--term-fg", frame.fg);
  const cursor = frame.cursor;
  $("screen").innerHTML = frame.lines
    .map((runs, line) => lineHtml(runs, cursor && cursor[0] === line ? cursor[1] : null))
    .join("");
  if (state.writable !== frame.writable) {
    state.writable = frame.writable;
    setDockWritable(frame.writable);
    if (frame.writable) $("notice").hidden = true;
    else showNotice("The agent has exited — read only.");
  }
  if (stick) {
    scroller.scrollTop = scroller.scrollHeight;
    resetHistory();
  }
}

function resetHistory() {
  state.historyFirst = 0;
  state.historyDone = false;
  state.historyLoading = false;
  $("history").innerHTML = "";
}

function requestHistory() {
  if (state.watched === null || state.historyDone || state.historyLoading) return;
  state.historyLoading = true;
  send({ type: "history", id: state.watched, before: state.historyFirst, count: HISTORY_PAGE });
}

function prependHistory(page) {
  state.historyLoading = false;
  if (page.lines.length === 0) {
    state.historyDone = true;
    return;
  }
  const scroller = $("scroller");
  const before = scroller.scrollHeight;
  $("history").insertAdjacentHTML("afterbegin", page.lines.map((runs) => lineHtml(runs, null)).join(""));
  scroller.scrollTop += scroller.scrollHeight - before;
  state.historyFirst = page.first;
}

function showNotice(text) {
  $("notice").textContent = text;
  $("notice").hidden = false;
}

function setDockWritable(writable) {
  document.querySelector(".dock").classList.toggle("read-only", !writable);
  $("prompt").disabled = !writable;
}

function applyFont() {
  document.documentElement.style.setProperty("--term-font", `${state.fontPx}px`);
}

function zoom(step) {
  state.fontPx = Math.min(FONT_MAX, Math.max(FONT_MIN, state.fontPx + step));
  applyFont();
  saveFontPx();
}

// Composer

function autosize() {
  const prompt = $("prompt");
  prompt.style.height = "auto";
  prompt.style.height = `${prompt.scrollHeight}px`;
}

function submitPrompt(event) {
  event.preventDefault();
  if (state.watched === null || !state.writable) return;
  const prompt = $("prompt");
  send({ type: "send", id: state.watched, text: prompt.value });
  prompt.value = "";
  autosize();
  $("scroller").scrollTop = $("scroller").scrollHeight;
}

function pressKey(button) {
  if (state.watched === null || !state.writable) return;
  send({ type: "key", id: state.watched, key: button.dataset.key });
}

// Layout: the page follows the visual viewport so the dock rides above the iOS keyboard.

function fitViewport() {
  const height = window.visualViewport ? window.visualViewport.height : window.innerHeight;
  document.documentElement.style.setProperty("--app-height", `${height}px`);
}

function showExpired() {
  for (const view of document.querySelectorAll(".view")) view.hidden = true;
  $("expired-view").hidden = false;
}

// Wiring

$("agents").addEventListener("click", (event) => {
  const row = event.target.closest(".row");
  if (row) location.hash = `#/pane/${row.dataset.id}`;
});
$("back").addEventListener("click", () => history.back());
$("font-down").addEventListener("click", () => zoom(-1));
$("font-up").addEventListener("click", () => zoom(1));
$("composer").addEventListener("submit", submitPrompt);
$("prompt").addEventListener("input", autosize);
$("keys").addEventListener("mousedown", (event) => event.preventDefault());
$("keys").addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (button) pressKey(button);
});
$("scroller").addEventListener("scroll", () => {
  if ($("scroller").scrollTop < 40) requestHistory();
});

function route() {
  const match = location.hash.match(/^#\/pane\/(\d+)$/);
  if (match) openMirror(Number(match[1]));
  else closeMirror();
}
window.addEventListener("hashchange", route);

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") connect();
});
if (window.visualViewport) window.visualViewport.addEventListener("resize", fitViewport);
window.addEventListener("resize", fitViewport);

fitViewport();
applyFont();
renderAgents();
route();
connect();
