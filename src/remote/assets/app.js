// Phone page of helm (specs/remote.md §7): agents list, terminal mirror, composer, launch sheet.
"use strict";

const HISTORY_PAGE = 200;
const RECONNECT_MS = 1500;
const LAUNCH_ROUTE = "#/new";
const FONT_MIN = 3;
const FONT_MAX = 20;
const READING_FONT_MIN = 7;
const RESIZE_DEBOUNCE_MS = 250;
const GRID_PADDING_X = 16;
const GRID_PADDING_BOTTOM = 8;
const LINE_HEIGHT = 1.25;
const PAN_GUARD_MS = 150;
const REPEAT_DELAY_MS = 400;
const REPEAT_EVERY_MS = 60;
const CHOICE_KEY = "helm.launch";
// The terminal view is hidden behind the sheet, so the launch size is estimated; the
// watch that follows claims the real one.
const CELL_WIDTH_EM = 0.6;
const DOCK_ESTIMATE_PX = 160;

const $ = (id) => document.getElementById(id);

const state = {
  socket: null,
  agents: [],
  watched: null,
  writable: false,
  // Full-screen TUI (Claude Code): it scrolls its own view, a swipe goes to it.
  appScrolls: false,
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
  // The mirror follows new output while the user has not scrolled away from the bottom.
  pinned: true,
  targets: { entries: [], agents: [] },
  // Last entry id and agent name picked on this phone.
  choice: loadChoice(),
  launching: false,
  launchLabel: null,
  // Panes this phone launched, labelled until the agents list names them.
  launchedLabels: new Map(),
};

function loadChoice() {
  try {
    return JSON.parse(localStorage.getItem(CHOICE_KEY)) || {};
  } catch (_) {
    return {};
  }
}

function saveChoice() {
  try {
    localStorage.setItem(CHOICE_KEY, JSON.stringify(state.choice));
  } catch (_) { /* private browsing: the choice is not remembered */ }
}

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
    if (state.launching) launchFailed("The connection to the Mac dropped — try again.");
    checkAccess().then((granted) => {
      if (granted) setTimeout(connect, RECONNECT_MS);
    });
  };
}

async function checkAccess() {
  try {
    const response = await fetch("/", { cache: "no-store" });
    if (response.status === 401) {
      showUnpaired();
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
    case "theme": applyTheme(message); break;
    case "agents": state.agents = message.agents; renderAgents(); renderHeader(); break;
    case "screen": if (message.id === state.watched) renderScreen(message); break;
    case "history": if (message.id === state.watched) prependHistory(message); break;
    case "ended": if (message.id === state.watched) endMirror(); break;
    case "targets": receiveTargets(message); break;
    case "launched": launched(message.id); break;
    case "launch_failed": launchFailed(message.message); break;
  }
}

// Theme: helm's own, sent on connect and whenever the Mac switches it.

function applyTheme(theme) {
  const root = document.documentElement;
  root.dataset.theme = theme.dark ? "dark" : "light";
  for (const [name, color] of Object.entries(theme.tokens)) root.style.setProperty(`--${name}`, color);
  $("theme-color").content = theme.tokens.sidebar;
}

// Agents list

const STATE_LABELS = { working: "Working", done: "Done", idle: "Idle" };

function icon(name) {
  return `<svg class="icon"><use href="#i-${name}"/></svg>`;
}

function statePill(kind) {
  return `<span class="state ${kind}"><span class="badge ${kind}"></span>${STATE_LABELS[kind]}</span>`;
}

const SEPARATOR = `<span class="sep">·</span>`;

function subtitle(agent) {
  const branch = agent.branch
    ? `${icon("branch")}<span class="branch">${escapeHtml(agent.branch)}</span>${SEPARATOR}`
    : "";
  return `${branch}<span>${escapeHtml(agent.tab)}</span>`;
}

function rowLines(agent) {
  if (!agent.branch) return `<div class="name">${escapeHtml(agent.tab)}</div>`;
  return `<div class="name">${escapeHtml(agent.branch)}</div><div class="sub"><span>${escapeHtml(agent.tab)}</span></div>`;
}

function byProject(items) {
  const groups = new Map();
  for (const item of items) {
    if (!groups.has(item.project)) groups.set(item.project, []);
    groups.get(item.project).push(item);
  }
  return groups;
}

function renderAgents() {
  const list = $("agents");
  if (state.agents.length === 0) {
    list.innerHTML = `<div class="empty">${icon("terminal")}<p class="empty-title">No agent running</p>
      <p>Start Claude Code or Codex in a helm terminal: it shows up here.</p></div>`;
    return;
  }
  const groups = byProject(state.agents);
  let html = "";
  for (const [project, agents] of groups) {
    html += `<section class="group"><h2 class="group-name">${escapeHtml(project)}</h2><div class="card">`;
    for (const agent of agents) {
      html += `<button class="row" type="button" data-id="${agent.id}">
        <span class="who">${rowLines(agent)}</span>
        ${statePill(agent.badge)}${icon("chevron")}</button>`;
    }
    html += `</div></section>`;
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
  const launchedHere = state.launchedLabels.get(id);
  if (launchedHere && !state.agents.some((a) => a.id === id)) showNotice(`Starting ${launchedHere.tab}…`);
  send({ type: "watch", id, ...phoneSize() });
}

function closeMirror() {
  if (state.watched !== null) send({ type: "unwatch" });
  state.watched = null;
  toggleCommands(false);
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
  const agent = state.agents.find((a) => a.id === state.watched)
    || state.launchedLabels.get(state.watched);
  if (!agent) {
    $("term-badge").innerHTML = "";
    return;
  }
  $("term-title").textContent = agent.project;
  $("term-subtitle").innerHTML = subtitle(agent);
  $("term-badge").innerHTML = agent.badge ? statePill(agent.badge) : "";
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
  state.appScrolls = frame.app_scrolls;
  scroller.classList.toggle("app-scrolls", frame.app_scrolls);
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

// A full-screen TUI takes every swipe as the wheel: its screen has nothing to scroll
// locally. Released while moving, that scroll glides on and slows down like a native
// one. Elsewhere, a swipe past the top requests the history above the screen.
const GLIDE_DECAY_PER_MS = 0.998;
const GLIDE_MIN_SPEED = 0.03;
const GLIDE_MAX_PAUSE_MS = 80;
const swipe = { id: null, y: 0, time: 0, pending: 0, speed: 0, cell: null, toApp: false, glide: 0 };

function startSwipe(event) {
  if (event.touches.length !== 1) return;
  cancelAnimationFrame(swipe.glide);
  swipe.id = event.touches[0].identifier;
  swipe.y = event.touches[0].clientY;
  swipe.time = event.timeStamp;
  swipe.pending = 0;
  swipe.speed = 0;
  swipe.toApp = false;
}

function scrollApp(dy) {
  const lineHeight = state.displayPx * LINE_HEIGHT;
  swipe.pending += dy;
  const lines = Math.trunc(swipe.pending / lineHeight);
  if (lines === 0) return;
  swipe.pending -= lines * lineHeight;
  send({ type: "scroll", id: state.watched, lines, ...swipe.cell });
}

function glide(event) {
  if (event.touches.length !== 0 || !swipe.toApp) return;
  swipe.toApp = false;
  if (event.timeStamp - swipe.time > GLIDE_MAX_PAUSE_MS) return;
  let last = performance.now();
  const step = (now) => {
    const elapsed = now - last;
    last = now;
    swipe.speed *= GLIDE_DECAY_PER_MS ** elapsed;
    if (Math.abs(swipe.speed) < GLIDE_MIN_SPEED || state.watched === null || !state.writable) return;
    scrollApp(swipe.speed * elapsed);
    swipe.glide = requestAnimationFrame(step);
  };
  swipe.glide = requestAnimationFrame(step);
}

function screenCell(touch) {
  const rect = $("screen").getBoundingClientRect();
  return {
    line: Math.max(0, Math.floor((touch.clientY - rect.top) / (state.displayPx * LINE_HEIGHT))),
    col: Math.max(0, Math.floor((touch.clientX - rect.left - GRID_PADDING_X / 2) / (state.displayPx * charWidthEm()))),
  };
}

function moveSwipe(event) {
  if (event.touches.length !== 1 || pinch.distance !== 0) return;
  const touch = event.touches[0];
  if (touch.identifier !== swipe.id) return startSwipe(event);
  const dy = touch.clientY - swipe.y;
  const elapsed = event.timeStamp - swipe.time;
  swipe.y = touch.clientY;
  swipe.time = event.timeStamp;
  if (!state.appScrolls) {
    if (dy > 0 && $("scroller").scrollTop <= 0) requestHistory();
    return;
  }
  event.preventDefault();
  if (dy === 0 || state.watched === null || !state.writable) return;
  if (elapsed > 0) swipe.speed = 0.7 * (dy / elapsed) + 0.3 * swipe.speed;
  swipe.toApp = true;
  swipe.cell = screenCell(touch);
  scrollApp(dy);
}

// Launch sheet (specs/remote.md §7.2): an entry and an agent of the Mac's own lists.

function receiveTargets(targets) {
  state.targets = targets;
  $("new-agent").hidden = targets.entries.length === 0 || targets.agents.length === 0;
  if (!$("launch").hidden) renderLaunch();
}

function chosenEntry() {
  const { entries } = state.targets;
  return entries.find((entry) => entry.id === state.choice.entry) || entries[0];
}

function chosenAgent() {
  const { agents } = state.targets;
  return agents.find((agent) => agent.name === state.choice.agent) || agents[0];
}

function entryRow(entry, chosen) {
  const lines = entry.worktree
    ? `<div class="name">${escapeHtml(entry.branch || "Worktree")}</div><div class="sub"><span>Worktree</span></div>`
    : `<div class="name">${escapeHtml(entry.project)}</div>` +
      (entry.branch ? `<div class="sub">${icon("branch")}<span class="branch">${escapeHtml(entry.branch)}</span></div>` : "");
  return `<button class="row choice${entry.worktree ? " nested" : ""}" type="button" role="radio"
    aria-checked="${chosen}" data-entry="${entry.id}"><span class="who">${lines}</span>${chosen ? icon("check") : ""}</button>`;
}

function renderLaunch() {
  const entry = chosenEntry();
  const agent = chosenAgent();
  let html = "";
  for (const entries of byProject(state.targets.entries).values()) {
    html += `<div class="card">${entries.map((e) => entryRow(e, e === entry)).join("")}</div>`;
  }
  $("launch-entries").innerHTML = html;
  $("launch-agents").innerHTML = state.targets.agents
    .map((a) => `<button class="chip" type="button" role="radio" aria-checked="${a === agent}"
      data-agent="${a.id}">${escapeHtml(a.name)}</button>`)
    .join("");
  $("launch-start").disabled = state.launching || !entry || !agent;
  $("launch-start").textContent = state.launching ? "Starting…" : "Start";
}

function openLaunch() {
  $("launch-error").hidden = true;
  renderLaunch();
  $("launch").hidden = false;
}

function closeLaunch() {
  state.launching = false;
  $("launch").hidden = true;
}

function launchSize() {
  return {
    cols: Math.floor((window.innerWidth - GRID_PADDING_X) / (state.fontPx * CELL_WIDTH_EM)),
    rows: Math.floor((window.innerHeight - DOCK_ESTIMATE_PX) / (state.fontPx * LINE_HEIGHT)),
  };
}

function startLaunch() {
  const entry = chosenEntry();
  const agent = chosenAgent();
  if (!entry || !agent || state.launching) return;
  state.choice = { entry: entry.id, agent: agent.name };
  saveChoice();
  state.launching = true;
  state.launchLabel = { project: entry.project, branch: entry.branch, tab: agent.name };
  $("launch-error").hidden = true;
  renderLaunch();
  send({ type: "launch", entry: entry.id, agent: agent.id, ...launchSize() });
}

function launched(id) {
  state.launchedLabels.set(id, state.launchLabel);
  if (location.hash === LAUNCH_ROUTE) location.replace(`#/pane/${id}`);
}

function launchFailed(message) {
  state.launching = false;
  renderLaunch();
  $("launch-error").textContent = message;
  $("launch-error").hidden = false;
}

// Composer

function autosize() {
  const prompt = $("prompt");
  prompt.style.height = "auto";
  prompt.style.height = `${prompt.scrollHeight}px`;
}

function sendText(text) {
  send({ type: "send", id: state.watched, text });
  $("scroller").scrollTop = $("scroller").scrollHeight;
}

function submitPrompt(event) {
  event.preventDefault();
  if (state.watched === null || !state.writable) return;
  const prompt = $("prompt");
  sendText(prompt.value);
  prompt.value = "";
  autosize();
}

function toggleCommands(open) {
  $("commands-menu").hidden = !open;
  $("commands-toggle").setAttribute("aria-expanded", String(open));
}

function runCommand(button) {
  toggleCommands(false);
  if (state.watched === null || !state.writable) return;
  sendText(button.dataset.command);
}

function pressKey(button) {
  if (state.watched === null || !state.writable) return;
  send({ type: "key", id: state.watched, key: button.dataset.key });
}

const hold = { timer: 0, repeated: false };

function startRepeat(event) {
  const button = event.target.closest("button[data-repeat]");
  if (!button) return;
  stopRepeat();
  hold.repeated = false;
  hold.timer = setTimeout(function repeat() {
    hold.repeated = true;
    pressKey(button);
    hold.timer = setTimeout(repeat, REPEAT_EVERY_MS);
  }, REPEAT_DELAY_MS);
}

function stopRepeat() {
  clearTimeout(hold.timer);
}

// Layout: the page is pinned to the visual viewport so the dock rides above the iOS
// keyboard.

// iOS skips its reveal pan for a field still transparent as the keyboard rises; that pan raced the viewport shrink.
function focusWithoutPan() {
  const prompt = $("prompt");
  prompt.style.opacity = "0";
  setTimeout(() => { prompt.style.opacity = ""; }, PAN_GUARD_MS);
}

// iOS reports the keyboard leaving only once it is gone: the page grows back with it instead.
function followKeyboardOut() {
  document.documentElement.style.setProperty("--app-height", `${document.documentElement.clientHeight}px`);
}

function fitViewport() {
  const viewport = window.visualViewport;
  const root = document.documentElement.style;
  root.setProperty("--app-height", `${viewport ? viewport.height : window.innerHeight}px`);
  root.setProperty("--app-top", `${viewport ? viewport.offsetTop : 0}px`);
  const width = viewport ? viewport.width : window.innerWidth;
  if (width !== state.viewportWidth) {
    state.viewportWidth = width;
    if (state.watched !== null) requestResize();
  }
}

function showUnpaired() {
  for (const view of document.querySelectorAll(".view")) view.hidden = true;
  $("unpaired-view").hidden = false;
}

// Wiring

$("agents").addEventListener("click", (event) => {
  const row = event.target.closest(".row");
  if (row) location.hash = `#/pane/${row.dataset.id}`;
});
$("back").addEventListener("click", () => history.back());
$("new-agent").addEventListener("click", () => { location.hash = LAUNCH_ROUTE; });
$("launch-cancel").addEventListener("click", () => history.back());
$("launch").addEventListener("click", (event) => {
  if (event.target === $("launch")) history.back();
});
$("launch-entries").addEventListener("click", (event) => {
  const row = event.target.closest(".row");
  if (!row || state.launching) return;
  state.choice = { ...state.choice, entry: Number(row.dataset.entry) };
  renderLaunch();
});
$("launch-agents").addEventListener("click", (event) => {
  const chip = event.target.closest(".chip");
  if (!chip || state.launching) return;
  const agent = state.targets.agents.find((a) => a.id === Number(chip.dataset.agent));
  state.choice = { ...state.choice, agent: agent.name };
  renderLaunch();
});
$("launch-start").addEventListener("click", startLaunch);
$("font-down").addEventListener("click", () => zoomStep(-1));
$("font-up").addEventListener("click", () => zoomStep(1));
$("composer").addEventListener("submit", submitPrompt);
$("prompt").addEventListener("input", autosize);
$("prompt").addEventListener("focus", focusWithoutPan);
$("prompt").addEventListener("blur", followKeyboardOut);
$("keys").addEventListener("mousedown", (event) => event.preventDefault());
$("keys").addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;
  if (button.hasAttribute("data-repeat") && hold.repeated) return;
  pressKey(button);
});
$("keys").addEventListener("pointerdown", startRepeat);
$("keys").addEventListener("pointerup", stopRepeat);
$("keys").addEventListener("pointercancel", stopRepeat);
$("commands-toggle").addEventListener("mousedown", (event) => event.preventDefault());
$("commands-toggle").addEventListener("click", () => toggleCommands($("commands-menu").hidden));
$("commands-menu").addEventListener("mousedown", (event) => event.preventDefault());
$("commands-menu").addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (button) runCommand(button);
});
document.addEventListener("click", (event) => {
  if (!event.target.closest(".commands")) toggleCommands(false);
});
$("scroller").addEventListener("scroll", () => {
  state.pinned = isAtBottom($("scroller"));
  if ($("scroller").scrollTop < 40) requestHistory();
});
new ResizeObserver(() => {
  if (state.pinned) $("scroller").scrollTop = $("scroller").scrollHeight;
}).observe($("scroller"));
$("scroller").addEventListener("touchstart", (event) => {
  startPinch(event);
  startSwipe(event);
}, { passive: true });
$("scroller").addEventListener("touchmove", (event) => {
  movePinch(event);
  moveSwipe(event);
}, { passive: false });
$("scroller").addEventListener("touchend", (event) => {
  endPinch(event);
  glide(event);
});
$("scroller").addEventListener("touchcancel", endPinch);
document.addEventListener("gesturestart", (event) => event.preventDefault());

// The sheet is a history entry: Safari's back-swipe snapshot of the list is then taken without it.
function route() {
  const match = location.hash.match(/^#\/pane\/(\d+)$/);
  if (location.hash !== LAUNCH_ROUTE) closeLaunch();
  if (match) openMirror(Number(match[1]));
  else closeMirror();
  if (location.hash === LAUNCH_ROUTE) openLaunch();
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
