// node swipe.mjs <pairing url> [pane id]: Chrome in iPhone touch emulation replays a 300 ms,
// 200 px downward swipe at 60 Hz on the mirror, then prints the scroll messages sent, the
// screen frames received (gaps in ms) and how far the app's content actually moved.
import { spawn } from "node:child_process";

const [pairing, pane = "1"] = process.argv.slice(2);
const origin = pairing.slice(0, pairing.indexOf("/pair"));
const chrome = spawn("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", [
  "--headless=new", "--remote-debugging-port=9333", `--user-data-dir=${process.env.TMPDIR}/helm-swipe-profile`,
  "--no-first-run", "about:blank"], { stdio: "ignore" });
process.on("exit", () => chrome.kill());
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let target;
for (let i = 0; i < 50 && !target; i++) {
  await sleep(200);
  try { target = (await (await fetch("http://127.0.0.1:9333/json")).json()).find((t) => t.type === "page"); } catch {}
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve) => ws.addEventListener("open", resolve));
let seq = 0;
const replies = new Map();
ws.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  replies.get(message.id)?.(message);
  replies.delete(message.id);
});
const cdp = (method, params = {}) => new Promise((resolve) => {
  replies.set(++seq, resolve);
  ws.send(JSON.stringify({ id: seq, method, params }));
});
const evaluate = async (expression) =>
  (await cdp("Runtime.evaluate", { expression, returnByValue: true })).result.result.value;

await cdp("Emulation.setDeviceMetricsOverride", { width: 402, height: 874, deviceScaleFactor: 3, mobile: true });
await cdp("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
await cdp("Page.navigate", { url: pairing });
await sleep(2500);
await cdp("Page.navigate", { url: `${origin}/#/pane/${pane}` });
await sleep(4000);
await evaluate(`(() => {
  window.__log = [];
  const socketSend = WebSocket.prototype.send;
  WebSocket.prototype.send = function (data) {
    const message = JSON.parse(data);
    if (message.type === "scroll") __log.push(["scroll", performance.now(), message.lines]);
    return socketSend.call(this, data);
  };
  const received = receive;
  receive = (message) => { if (message.type === "screen") __log.push(["screen", performance.now()]); received(message); };
})()`);
const screenLines = `JSON.stringify([...document.querySelectorAll("#screen > div")].map((line) => line.textContent))`;
const before = JSON.parse(await evaluate(screenLines));

const touch = (y) => [{ x: 200, y, id: 1 }];
await cdp("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: touch(250) });
for (let step = 1; step <= 18; step++) {
  await sleep(16);
  await cdp("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: touch(250 + (200 * step) / 18) });
}
await cdp("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
await sleep(2500);

const after = JSON.parse(await evaluate(screenLines));
let moved = [0, 0];
for (let shift = -80; shift <= 80; shift++) {
  const matches = before.filter((line, i) => line.trim().length > 8 && after[i + shift] === line).length;
  if (matches > moved[1]) moved = [shift, matches];
}
const log = JSON.parse(await evaluate("JSON.stringify(__log)"));
const scrolls = log.filter((entry) => entry[0] === "scroll");
const screens = log.filter((entry) => entry[0] === "screen");
console.log(`scroll: ${scrolls.length} messages, ${scrolls.reduce((sum, entry) => sum + entry[2], 0)} lines`);
console.log(`screen: ${screens.length} frames, gaps ${screens.slice(1).map((entry, i) => Math.round(entry[1] - screens[i][1])).join(" ")}`);
console.log(`content moved ${moved[0]} lines (${moved[1]} matching rows)`);
ws.close();
process.exit(0);
