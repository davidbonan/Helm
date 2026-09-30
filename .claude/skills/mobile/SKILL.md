---
name: mobile
description: Shows helm's phone page (specs/remote.md §7) in iPhone Safari on the Xcode iOS simulator and captures it — agents list, terminal mirror, dark/light theme, layout, keyboard opening frame by frame; replays swipes in Chrome's touch emulation. Runs the real phone server against fake agents, no Mac UI needed. Use it whenever the phone page changes or someone says « teste sur le simulateur », « à quoi ça ressemble sur iPhone », « vérifie la vue mobile ».
---

# The phone page on the iOS simulator

The simulator shares the Mac's network: it reaches the preview server on the LAN
address like a real iPhone does (plain `http://`, **not** a secure context), or on
`127.0.0.1` with `--loopback` (a secure context: APIs gated on it show up there only).

## Run

```bash
S=<scratchpad>
xcrun simctl boot "iPhone 17"; xcrun simctl bootstatus "iPhone 17"   # once; runs headless
cargo build -q --example phone_preview
./target/debug/examples/phone_preview [--light] [--loopback] > $S/preview.log 2>&1 &   # background
U=$(head -1 $S/preview.log)                  # pairing URL; then one `#/pane/<id>` per fake agent
xcrun simctl openurl booted "$U"             # pairs, lands on the agents list
xcrun simctl openurl booted "${U%%/pair*}/#/pane/1"   # terminal view of the first agent
xcrun simctl io booted screenshot $S/shot.png
sips -Z 900 $S/shot.png --out $S/shot-s.png  # small enough to read back
```

`phone_preview` serves the assets embedded at build time: rebuild it after editing
`src/remote/assets/`. Kill it (`pkill -f phone_preview`) before starting another one.
Three fake `claude` agents (two projects), a full-screen transcript, input echoed.
`--light` publishes helm's light theme; without it, helm dark. A command after `--`
runs in the first pane instead — a real full-screen Claude Code on an old session,
read only (strip this session's `CLAUDE*` variables, or it inherits them):

```bash
U=$(env | grep -o '^CLAUDE[A-Z_]*' | sed 's/^/-u /' | tr '\n' ' ')
env $U ./target/debug/examples/phone_preview -- claude --resume <old session id> --fork-session > $S/preview.log 2>&1 &
```

## Keyboard

The simulator hides the software keyboard while it believes a hardware one is
attached (AXe's HID counts as one). Once per device, then reboot it:

```bash
xcrun simctl spawn booted defaults write com.apple.keyboard.preferences AutomaticMinimizationEnabled -bool false
xcrun simctl spawn booted defaults write com.apple.keyboard.preferences HardwareKeyboardLastSeen -bool false
```

Tap the prompt with AXe (`brew install cameroncooke/axe/axe`; points, not pixels):
`axe tap -x 180 -y 747 --udid <udid>`; `axe describe-ui` lists Safari's own buttons
(the keyboard's *OK* closes it). Record and read it frame by frame:

```bash
xcrun simctl io booted recordVideo --codec h264 -f $S/kb.mp4 & R=$!   # start >2 s before the gesture
kill -INT $R
ffmpeg -i $S/kb.mp4 -vf "fps=30,select='between(n,60,80)',scale=240:-1,tile=7x3" -vsync vfr -frames:v 1 $S/kb.png
```

The recording is variable-rate (a still screen writes no frame). To read viewport
values per frame, paint them into a temporary fixed `<div>` from a `requestAnimationFrame`
loop (`visualViewport.height/offsetTop`, `scrollY`, the dock's rect) — removed before
committing. WebKit reports the keyboard's final viewport at once while it animates the
page: timings read in JS are not what the screen shows.

## Gestures

On the simulator, `axe drag --start-x 200 --start-y 250 --end-x 200 --end-y 450
--duration 0.3 --steps 30 --udid <udid>` while recording shows the feel. To count what
the page sends and receives, replay a 60 Hz swipe in Chrome's iPhone touch emulation
(the same page and server, not iOS):

```bash
node .claude/skills/mobile/swipe.mjs "$(head -1 $S/preview.log)" 1
# scroll: 23 messages, 23 lines · screen: 23 frames, gaps 50 67 33… · content moved 23 lines
```

## What the simulator tells, and what it does not

- **Reliable**: rendering by iOS WebKit (same engine as the iPhone), theme, layout,
  safe areas, Safari's floating toolbar, which web APIs a `http://` LAN page gets.
- **Reachable with AXe**: taps, drags and the software keyboard (above). WebDriver
  (`safaridriver`) finds no simulator host.
- **Not reliable**: timing — the Mac renders faster than an iPhone. Feel is still read
  on a real iPhone.
