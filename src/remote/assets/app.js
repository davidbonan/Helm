// Phone page of helm (specs/remote.md §7): agents list, terminal mirror, composer.
"use strict";

const HISTORY_PAGE = 200;
const RECONNECT_MS = 1500;
const FONT_MIN = 3;
const FONT_MAX = 20;
const READING_FONT_MIN = 7;
const RESIZE_DEBOUNCE_MS = 250;
const GRID_PADDING_X = 16;
const GRID_PADDING_BOTTOM = 8;
const LINE_HEIGHT = 1.25;

const $ = (id) => document.getElementById(id);

const state = {
  socket: null,
  agents: [],
  watched: null,
  writable: false,
  historyFirst: 0,
  historyDone: false,
  historyLoading: false,
  cols: 0,
  // The font the user reads at: it sets the phone's screen size in cells.
  fontPx: 12,
  // The font on screen: smaller while the Mac holds a wider size.
  displayPx: 12,
  viewportWidth: 0,
  resizeTimer: 0,
};

function escapeHtml(text) {
  return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

// Socket

function connect() {
  if (document.visibilityState === "hidden") return;
  if (state.socket && state.socket.readyState <= WebSocket.OPEN) return;
  const socket = new WebSocket(`ws://${location.host}/ws`);
  state.socket = socket;
  socket.onopen = () => {
    $("link").hidden = true;
    if (state.watched !== null) send({ type: "watch", id: state.watched, ...phoneSize() });
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
  state.cols = 0;
  resetHistory();
  $("screen").innerHTML = "";
  $("notice").hidden = true;
  $("agents-view").hidden = true;
  $("terminal-view").hidden = false;
  renderHeader();
  send({ type: "watch", id, ...phoneSize() });
}

function closeMirror() {
  if (state.watched !== null) send({ type: "unwatch" });
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
  if (frame.cols !== state.cols) {
    state.cols = frame.cols;
    fitDisplay();
  }
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

function applyFont(fontPx) {
  state.displayPx = Math.min(FONT_MAX, Math.max(FONT_MIN, fontPx));
  document.documentElement.style.setProperty("--term-font", `${state.displayPx}px`);
}

function charWidthEm() {
  const probe = document.createElement("span");
  probe.style.fontSize = "100px";
  probe.textContent = "0".repeat(100);
  $("screen").appendChild(probe);
  const width = probe.getBoundingClientRect().width / 10000;
  probe.remove();
  return width;
}

// The screen the phone offers at the reading font. Rows ignore the iOS keyboard:
// opening it must not resize the agent.
function phoneSize() {
  const scroller = $("scroller");
  const viewport = window.visualViewport;
  const keyboard = viewport ? Math.max(0, window.innerHeight - viewport.height) : 0;
  const width = scroller.clientWidth - GRID_PADDING_X;
  const height = scroller.clientHeight + keyboard - GRID_PADDING_BOTTOM;
  return {
    cols: Math.floor(width / (state.fontPx * charWidthEm())),
    rows: Math.floor(height / (state.fontPx * LINE_HEIGHT)),
  };
}

function requestResize() {
  clearTimeout(state.resizeTimer);
  state.resizeTimer = setTimeout(() => {
    if (state.watched !== null) send({ type: "resize", id: state.watched, ...phoneSize() });
  }, RESIZE_DEBOUNCE_MS);
}

// A frame wider than the phone (the Mac took the size back) shows fitted to the width.
function fitDisplay() {
  if (state.cols === 0) return;
  const fitted = ($("scroller").clientWidth - GRID_PADDING_X) / (state.cols * charWidthEm());
  applyFont(Math.min(state.fontPx, fitted));
}

// Zooming keeps the content point under `anchor` (scroller coordinates) in place;
// once it settles, the agent is asked for the screen that fits at the new font.
function zoomTo(fontPx, anchor) {
  const scroller = $("scroller");
  const before = state.displayPx;
  state.fontPx = Math.min(FONT_MAX, Math.max(READING_FONT_MIN, fontPx));
  applyFont(state.fontPx);
  const ratio = state.displayPx / before;
  scroller.scrollLeft = (scroller.scrollLeft + anchor.x) * ratio - anchor.x;
  scroller.scrollTop = (scroller.scrollTop + anchor.y) * ratio - anchor.y;
}

function zoomStep(step) {
  const scroller = $("scroller");
  zoomTo(Math.round(state.displayPx) + step, { x: 0, y: scroller.clientHeight });
  requestResize();
}

const pinch = { distance: 0, fontPx: 0 };

function touchDistance(touches) {
  return Math.hypot(touches[0].clientX - touches[1].clientX, touches[0].clientY - touches[1].clientY);
}

function touchMidpoint(touches) {
  const rect = $("scroller").getBoundingClientRect();
  return {
    x: (touches[0].clientX + touches[1].clientX) / 2 - rect.left,
    y: (touches[0].clientY + touches[1].clientY) / 2 - rect.top,
  };
}

function startPinch(event) {
  if (event.touches.length !== 2) return;
  pinch.distance = touchDistance(event.touches);
  pinch.fontPx = state.displayPx;
}

function movePinch(event) {
  if (event.touches.length !== 2 || pinch.distance === 0) return;
  event.preventDefault();
  zoomTo(pinch.fontPx * touchDistance(event.touches) / pinch.distance, touchMidpoint(event.touches));
}

function endPinch(event) {
  if (event.touches.length >= 2 || pinch.distance === 0) return;
  pinch.distance = 0;
  requestResize();
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

// Layout: the page is pinned to the visual viewport so the dock rides above the iOS
// keyboard — iOS pans the page by `offsetTop` when the keyboard opens.

function fitViewport() {
  const viewport = window.visualViewport;
  const scroller = $("scroller");
  const stick = isAtBottom(scroller);
  const root = document.documentElement.style;
  root.setProperty("--app-height", `${viewport ? viewport.height : window.innerHeight}px`);
  root.setProperty("--app-top", `${viewport ? viewport.offsetTop : 0}px`);
  if (stick) scroller.scrollTop = scroller.scrollHeight;
  const width = viewport ? viewport.width : window.innerWidth;
  if (width !== state.viewportWidth) {
    state.viewportWidth = width;
    if (state.watched !== null) requestResize();
  }
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
$("font-down").addEventListener("click", () => zoomStep(-1));
$("font-up").addEventListener("click", () => zoomStep(1));
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
$("scroller").addEventListener("touchstart", startPinch, { passive: true });
$("scroller").addEventListener("touchmove", movePinch, { passive: false });
$("scroller").addEventListener("touchend", endPinch);
$("scroller").addEventListener("touchcancel", endPinch);
document.addEventListener("gesturestart", (event) => event.preventDefault());

function route() {
  const match = location.hash.match(/^#\/pane\/(\d+)$/);
  if (match) openMirror(Number(match[1]));
  else closeMirror();
}
window.addEventListener("hashchange", route);

// Hidden, the phone stops driving: closing the socket gives the Mac its size back.
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") connect();
  else if (state.socket) state.socket.close();
});
if (window.visualViewport) {
  window.visualViewport.addEventListener("resize", fitViewport);
  window.visualViewport.addEventListener("scroll", fitViewport);
}
window.addEventListener("resize", fitViewport);

fitViewport();
applyFont(state.fontPx);
renderAgents();
route();
connect();
