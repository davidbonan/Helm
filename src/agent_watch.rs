//! AI agent detection in terminals (specs/agents.md): per-workspace activity
//! badge in the left sidebar.
//!
//! Two anti-false-positive layers:
//! - **Layer A — process gate**: an agent must be present in the PTY's
//!   foreground process group (`classify`). Without an agent, no output can
//!   light the badge (cargo build, vim, animated prompts…).
//! - **Layer B — activity heuristic**: sustained spontaneous output
//!   (`terminal::activity`) ⇒ it is working; prolonged silence after a real
//!   work episode ⇒ it is done (green badge until acknowledged).
//!
//! Known limitation: an agent launched **under tmux/screen** lives in another
//! session (different tty) — invisible from the PTY's pgid, no badge.

use crate::terminal::activity::ActivitySnapshot;

/// Recognized agent binaries (compared against the process `p_comm`, and
/// against the script path for agents launched via an interpreter).
pub const WATCHLIST: &[&str] = &["claude", "codex", "opencode", "gemini", "aider", "amp"];

/// Interpreters an agent may hide behind (npm / pip install): we then inspect
/// argv to find the launched script.
const INTERPRETERS: &[&str] = &["node", "bun", "deno", "python", "python3", "Python"];

/// Interactive shells: in a pane's foreground they mean an idle prompt, not a
/// named activity — excluded from tab auto-naming (terminal.md §4).
const SHELLS: &[&str] = &["zsh", "bash", "fish", "sh", "dash", "tcsh", "ksh", "nu"];

/// Silence beyond which the agent is no longer considered to be producing.
pub const SILENCE_MS: u64 = 2_500;
/// Minimum duration of a spontaneous-output run to count as work: a one-shot
/// redraw (prompt, banner) does not last a second.
pub const MIN_RUN_MS: u64 = 1_000;
/// Throughput floor: a "breathing" prompt (pulsing cursor) emits a few bytes
/// per second, a working agent emits hundreds.
pub const MIN_WORK_BYTES: u64 = 200;
/// Minimum span of the last **observed Working episode** (bounds stamped on the
/// reader's output) to arm the green badge — filters startup banners and resize
/// redraws. Measured on the episode, not the raw run: trailing end-of-turn
/// redraws (prompt returning, "esc to interrupt" bar cleared) restart a tiny
/// run and must not disarm detection.
pub const ATTENTION_MIN_WORK_MS: u64 = 2_000;
/// Silence required before arming the green — longer than `SILENCE_MS` so it
/// does not flicker during a silent tool call mid-turn.
pub const ATTENTION_SILENCE_MS: u64 = 6_000;

/// State shown by the badge, ordered by ascending priority for per-workspace
/// aggregation (`max`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentBadge {
    /// No agent in the panes: no badge.
    #[default]
    None,
    /// Agent present, idle: grey badge.
    Idle,
    /// The agent finished a work episode the user did not see: green badge,
    /// acknowledged on tab focus or keystroke.
    Done,
    /// Agent working: spinner.
    Working,
}

/// Recognizes an agent in a foreground-group process. `argv` may be empty
/// (unreadable): we then degrade to comparing `comm` alone — fail-safe, never
/// an extra false positive.
pub fn classify(comm: &str, argv: &[String]) -> Option<&'static str> {
    if let Some(name) = WATCHLIST.iter().find(|n| **n == comm) {
        return Some(name);
    }
    if INTERPRETERS.contains(&comm) {
        // npm/pip agent: `node …/node_modules/@anthropic-ai/claude-code/cli.js`.
        // We match only exact path components (`claude`, `claude-code`,
        // `gemini-cli`…) — a user project "claude-test" does not match.
        return argv
            .iter()
            .skip(1)
            .find_map(|arg| arg.split('/').find_map(agent_name));
    }
    // Versioned binary: Claude Code's native installer runs
    // `~/.local/bin/claude` → symlink `versions/2.1.162` ⇒ p_comm = "2.1.162".
    // The invoked name survives in argv[0]: only its exact basename counts —
    // never the arguments (`vim claude` does not match).
    agent_name(argv.first()?.rsplit('/').next()?)
}

/// True for an interactive shell process name: a pane whose foreground is just
/// its shell is idle, not running a named activity (tab auto-naming).
pub fn is_shell(comm: &str) -> bool {
    SHELLS.contains(&comm)
}

/// Matches an exact component against the watchlist (`claude`, `claude-code`,
/// `gemini-cli`…).
fn agent_name(component: &str) -> Option<&'static str> {
    WATCHLIST.iter().copied().find(|name| {
        component == *name
            || component == format!("{name}-code")
            || component == format!("{name}-cli")
    })
}

/// Aggregates the badges of a workspace's panes: the highest-priority state
/// wins (Working > Done > Idle > None).
pub fn aggregate(badges: impl IntoIterator<Item = AgentBadge>) -> AgentBadge {
    badges.into_iter().max().unwrap_or_default()
}

/// Rising edge of a completion: a pane the user has not yet acknowledged just
/// entered `Done`. Drives the one-shot completion notification (specs/agents.md)
/// — fired once per episode, never re-fired while the badge stays green.
pub fn newly_completed(prev: AgentBadge, now: AgentBadge) -> bool {
    now == AgentBadge::Done && prev != AgentBadge::Done
}

/// Human-facing form of an agent's name (a lowercase binary name, `claude` →
/// `Claude`), for the completion banner and the dashboard.
pub fn display_name(agent: &str) -> String {
    let mut chars = agent.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A pane's state machine, ticked at a fixed cadence (1 s). Pure: time and
/// probes are injected, testable without a PTY or real clock.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PaneAgentState {
    pending_attention: bool,
    /// Last activity already "seen" by the user (focus or reply): the green
    /// does not re-arm on an already-acknowledged episode.
    acked_spont_ms: u64,
    /// Bounds of the last Working episode **observed by the ticks**, stamped on
    /// the reader's output timestamps — not on the tick clock: a real ~2-3 s
    /// turn is seen by only 1-2 ticks, and measured by tick the episode would
    /// miss `ATTENTION_MIN_WORK_MS` depending on phase. Two episodes less than
    /// `ATTENTION_SILENCE_MS` apart merge (silent tool call mid-turn) — except
    /// an already-acknowledged episode: a trailing redraw (resize) does not
    /// re-arm an already-seen green.
    episode_start_ms: u64,
    episode_end_ms: u64,
    /// Consecutive ticks without an agent: the first is tolerated when an
    /// episode is in play (transiently blind probe), the second wipes.
    absent_ticks: u8,
}

impl PaneAgentState {
    /// `agent_present`: layer A (process). `focused`: the pane is in the active
    /// workspace's active tab and the window is focused — seeing = acknowledging.
    pub fn tick(
        &mut self,
        agent_present: bool,
        s: &ActivitySnapshot,
        focused: bool,
        now_ms: u64,
    ) -> AgentBadge {
        if !agent_present {
            // A lone absent tick looks more like a blind probe (transient
            // failure of tcgetpgrp / proc_listpgrppids) than a real departure:
            // wiping immediately would auto-acknowledge a pending green, lost
            // forever. Tolerated once when an episode is in play; an agent that
            // really left confirms it at the next tick.
            if self.episode_end_ms != 0 && self.absent_ticks == 0 {
                self.absent_ticks = 1;
                return if self.pending_attention {
                    AgentBadge::Done
                } else {
                    AgentBadge::Idle
                };
            }
            *self = Self {
                acked_spont_ms: s.last_spont_output_ms,
                ..Self::default()
            };
            return AgentBadge::None;
        }
        self.absent_ticks = 0;

        let silence = match s.last_spont_output_ms {
            0 => u64::MAX,
            t => now_ms.saturating_sub(t),
        };
        let run_len = s.last_spont_output_ms.saturating_sub(s.spont_run_start_ms);
        let working = silence < SILENCE_MS
            && run_len >= MIN_RUN_MS
            && s.recent_spont_bytes(now_ms) >= MIN_WORK_BYTES;
        if working {
            // New episode if the gap since the previous one exceeds the
            // tool-call merge window, or if the previous one was already
            // acknowledged (a trailing redraw after ack has nothing new to
            // report).
            if self.episode_end_ms == 0
                || s.spont_run_start_ms.saturating_sub(self.episode_end_ms) > ATTENTION_SILENCE_MS
                || self.episode_end_ms <= self.acked_spont_ms
            {
                self.episode_start_ms = s.spont_run_start_ms;
            }
            self.episode_end_ms = s.last_spont_output_ms;
            return AgentBadge::Working;
        }

        // Acknowledgements: seeing the tab, or having replied after the agent's output.
        if focused || s.last_input_ms > s.last_spont_output_ms {
            self.pending_attention = false;
            self.acked_spont_ms = s.last_spont_output_ms.max(self.episode_end_ms);
        }

        let episode_len = self.episode_end_ms.saturating_sub(self.episode_start_ms);
        if !self.pending_attention
            && silence >= ATTENTION_SILENCE_MS
            && episode_len >= ATTENTION_MIN_WORK_MS
            && self.episode_end_ms > self.acked_spont_ms
        {
            self.pending_attention = true;
        }

        if self.pending_attention {
            AgentBadge::Done
        } else {
            AgentBadge::Idle
        }
    }
}

/// macOS process probe: foreground-group members + argv. Confined here; the
/// rest of the module is portable and pure.
#[cfg(target_os = "macos")]
pub mod probe {
    use super::classify;

    /// `true` if a watchlist agent is a member of group `pgid`.
    pub fn agent_in_group(pgid: i32) -> bool {
        foreground_agent(pgid).is_some()
    }

    /// Watchlist name of the agent running in group `pgid` (the first member that
    /// classifies), for the completion notification and the cross-repo dashboard
    /// (specs/agents.md). `None` when no agent is present.
    pub fn foreground_agent(pgid: i32) -> Option<&'static str> {
        group_comms(pgid).iter().find_map(|(pid, comm)| {
            // argv disambiguates interpreters (launched script) and versioned
            // binaries (invoked name); unnecessary if the comm matches alone.
            let argv = if super::WATCHLIST.contains(&comm.as_str()) {
                Vec::new()
            } else {
                argv(*pid)
            };
            classify(comm, &argv)
        })
    }

    /// Name of the activity in the foreground of group `pgid`, for tab
    /// auto-naming (terminal.md §4): a recognized agent (`classify`), otherwise
    /// the first non-shell command in the group. `None` at an idle shell prompt.
    pub fn foreground_name(pgid: i32) -> Option<String> {
        let comms = group_comms(pgid);
        for (pid, comm) in &comms {
            // argv only needed to disambiguate interpreters / versioned binaries.
            let argv = if super::WATCHLIST.contains(&comm.as_str()) {
                Vec::new()
            } else {
                argv(*pid)
            };
            if let Some(name) = classify(comm, &argv) {
                return Some(name.to_string());
            }
        }
        comms
            .iter()
            .map(|(_, comm)| comm.as_str())
            .find(|comm| !super::is_shell(comm))
            .map(str::to_owned)
    }

    /// `(pid, name)` of the members of group `pgid` (libproc `proc_listpgrppids`
    /// + `proc_name`, exposed by the `libc` crate).
    pub fn group_comms(pgid: i32) -> Vec<(i32, String)> {
        let pid_size = std::mem::size_of::<libc::pid_t>();
        // Empty call: upper bound on the **number** of pids (not bytes).
        let needed = unsafe { libc::proc_listpgrppids(pgid, std::ptr::null_mut(), 0) };
        if needed <= 0 {
            return Vec::new();
        }
        // Margin: processes may appear between the two calls.
        let mut pids = vec![0 as libc::pid_t; needed as usize + 8];
        let filled = unsafe {
            libc::proc_listpgrppids(
                pgid,
                pids.as_mut_ptr().cast(),
                (pids.len() * pid_size) as libc::c_int,
            )
        };
        if filled <= 0 {
            return Vec::new();
        }
        pids.truncate(filled as usize);
        pids.into_iter()
            .filter(|pid| *pid > 0)
            .filter_map(|pid| {
                let mut buf = [0u8; 64];
                let len =
                    unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
                (len > 0).then(|| {
                    let name = String::from_utf8_lossy(&buf[..len as usize]).into_owned();
                    (pid, name)
                })
            })
            .collect()
    }

    /// A process's argv (sysctl `KERN_PROCARGS2`). Empty if unreadable (another
    /// user's process, race with its death) — comm-only degradation.
    pub fn argv(pid: i32) -> Vec<String> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut size: libc::size_t = 0;
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as libc::c_uint,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size < 4 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size];
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as libc::c_uint,
                buf.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size < 4 {
            return Vec::new();
        }
        buf.truncate(size);
        parse_procargs2(&buf)
    }

    /// KERN_PROCARGS2 format: argc (i32) · NUL-terminated exec path · NUL
    /// padding · argv[0..argc] NUL-separated.
    fn parse_procargs2(buf: &[u8]) -> Vec<String> {
        let argc = i32::from_ne_bytes(buf[..4].try_into().unwrap_or_default()).max(0) as usize;
        let rest = &buf[4..];
        // Skip the executable path then the padding.
        let exec_end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        let mut i = exec_end;
        while i < rest.len() && rest[i] == 0 {
            i += 1;
        }
        rest[i..]
            .split(|b| *b == 0)
            .take(argc)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::activity::{PaneActivity, RUN_GAP_MS};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn classify_matches_known_agents_by_comm() {
        for name in WATCHLIST {
            assert_eq!(classify(name, &[]), Some(*name));
        }
    }

    #[test]
    fn classify_ignores_shells_and_build_tools() {
        for comm in ["zsh", "bash", "cargo", "rustc", "vim", "htop", "cat"] {
            assert_eq!(classify(comm, &[]), None);
        }
    }

    #[test]
    fn classify_escalates_interpreters_to_the_script_path() {
        let argv = args(&[
            "node",
            "/usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js",
        ]);
        assert_eq!(classify("node", &argv), Some("claude"));

        let argv = args(&["python3", "/opt/venv/bin/aider", "--model", "x"]);
        assert_eq!(classify("python3", &argv), Some("aider"));

        let argv = args(&["node", "/x/node_modules/@google/gemini-cli/dist/index.js"]);
        assert_eq!(classify("node", &argv), Some("gemini"));
    }

    #[test]
    fn classify_rejects_interpreters_running_something_else() {
        let argv = args(&["node", "/x/node_modules/.bin/eslint", "src/"]);
        assert_eq!(classify("node", &argv), None);
        // A user folder containing an agent name does not match.
        let argv = args(&["node", "/Users/me/dev/claude-test/index.js"]);
        assert_eq!(classify("node", &argv), None);
        // Without readable argv, an interpreter alone is not enough.
        assert_eq!(classify("node", &[]), None);
    }

    #[test]
    fn classify_matches_a_versioned_binary_through_argv0() {
        // Claude Code native installer: `~/.local/bin/claude` is a symlink to
        // `versions/2.1.162` ⇒ p_comm = "2.1.162"; the invoked name survives
        // only in argv[0].
        let argv = args(&["claude", "--continue"]);
        assert_eq!(classify("2.1.162", &argv), Some("claude"));
        let argv = args(&["/Users/me/.local/bin/claude"]);
        assert_eq!(classify("2.1.162", &argv), Some("claude"));
    }

    #[test]
    fn classify_argv0_only_matches_the_invoked_name() {
        // An argument named claude is not the invoked program.
        assert_eq!(classify("vim", &args(&["vim", "claude"])), None);
        // Only the exact basename of argv[0] counts.
        let argv = args(&["/Users/me/dev/claude-test/run"]);
        assert_eq!(classify("2.1.162", &argv), None);
        assert_eq!(classify("2.1.162", &args(&["claude-notes"])), None);
        // Without readable argv, an unknown comm does not match.
        assert_eq!(classify("2.1.162", &[]), None);
    }

    #[test]
    fn is_shell_matches_shells_only() {
        for shell in ["zsh", "bash", "fish", "sh", "dash"] {
            assert!(is_shell(shell));
        }
        for activity in ["vim", "cargo", "claude", "node", "ssh", "htop"] {
            assert!(!is_shell(activity), "{activity} is a named activity");
        }
    }

    #[test]
    fn aggregate_prioritises_working_over_done_over_idle() {
        use AgentBadge::*;
        assert_eq!(aggregate([Idle, Working, Done]), Working);
        assert_eq!(aggregate([Idle, Done]), Done);
        assert_eq!(aggregate([None, Idle]), Idle);
        assert_eq!(aggregate([]), None);
    }

    #[test]
    fn newly_completed_fires_once_on_the_rising_edge() {
        use AgentBadge::*;
        assert!(newly_completed(Working, Done));
        assert!(newly_completed(Idle, Done));
        assert!(newly_completed(None, Done));
        // No re-fire while the green persists, and never for the other states.
        assert!(!newly_completed(Done, Done));
        assert!(!newly_completed(Done, Idle));
        assert!(!newly_completed(Working, Working));
    }

    /// Spontaneous run: `len` ms of regular, copious chunks ending at `end`.
    fn busy_run(a: &PaneActivity, start: u64, len: u64) {
        let mut t = start;
        while t <= start + len {
            a.stamp_output(t, 256);
            t += 200;
        }
    }

    /// Drives a realistic episode: copious chunks every 200 ms and a state
    /// machine tick every second (the app's cadence), over time — the Working
    /// episode must be **observed** to arm the green.
    fn drive_run(
        st: &mut PaneAgentState,
        a: &PaneActivity,
        start: u64,
        len: u64,
        focused: bool,
    ) -> AgentBadge {
        let mut badge = AgentBadge::None;
        let mut next_tick = start + 1_000;
        let mut t = start;
        while t <= start + len {
            a.stamp_output(t, 256);
            if t >= next_tick {
                badge = st.tick(true, &a.snapshot(), focused, t);
                next_tick += 1_000;
            }
            t += 200;
        }
        badge
    }

    #[test]
    fn no_agent_means_no_badge_even_with_output() {
        let a = PaneActivity::default();
        busy_run(&a, 1_000, 10_000);
        let mut st = PaneAgentState::default();
        assert_eq!(
            st.tick(false, &a.snapshot(), false, 11_500),
            AgentBadge::None
        );
    }

    #[test]
    fn sustained_spontaneous_output_is_working() {
        let a = PaneActivity::default();
        busy_run(&a, 1_000, 3_000);
        let mut st = PaneAgentState::default();
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 4_200),
            AgentBadge::Working
        );
    }

    #[test]
    fn a_short_burst_never_reaches_working() {
        let a = PaneActivity::default();
        // Banner: a single copious chunk.
        a.stamp_output(1_000, 2_000);
        let mut st = PaneAgentState::default();
        assert_eq!(st.tick(true, &a.snapshot(), false, 1_500), AgentBadge::Idle);
    }

    #[test]
    fn a_low_rate_animated_prompt_is_not_working() {
        let a = PaneActivity::default();
        // Pulsing cursor: regular but tiny chunks.
        let mut t = 1_000;
        while t <= 11_000 {
            a.stamp_output(t, 6);
            t += 500;
        }
        let mut st = PaneAgentState::default();
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 11_200),
            AgentBadge::Idle,
            "low-rate output stays under the byte floor"
        );
    }

    #[test]
    fn silence_after_real_work_arms_the_green_badge() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        assert_eq!(
            drive_run(&mut st, &a, 1_000, 6_000, false),
            AgentBadge::Working
        );
        let end = 7_000;
        // Short silence: not green yet (tool-call anti-flap).
        assert_eq!(
            st.tick(true, &a.snapshot(), false, end + SILENCE_MS + 500),
            AgentBadge::Idle
        );
        assert_eq!(
            st.tick(true, &a.snapshot(), false, end + ATTENTION_SILENCE_MS + 1),
            AgentBadge::Done
        );
    }

    #[test]
    fn a_trailing_redraw_does_not_disarm_the_done_badge() {
        // Real end of turn: after the last output, the agent's TUI still emits
        // a small isolated redraw (the "esc to interrupt" bar clears, the
        // prompt returns) that restarts a tiny raw run. The observed episode
        // stays acquired: the green must arm.
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 6_000, false);
        let redraw = 7_000 + RUN_GAP_MS + 500;
        a.stamp_output(redraw, 80);
        assert_eq!(
            st.tick(true, &a.snapshot(), false, redraw + 1_000),
            AgentBadge::Idle
        );
        assert_eq!(
            st.tick(
                true,
                &a.snapshot(),
                false,
                redraw + ATTENTION_SILENCE_MS + 1
            ),
            AgentBadge::Done,
            "a tiny trailing redraw must not reset the finished episode"
        );
    }

    #[test]
    fn a_startup_banner_never_arms_done() {
        let a = PaneActivity::default();
        // Banner: 600 ms of output, then silence — Working never observed.
        busy_run(&a, 1_000, 600);
        let mut st = PaneAgentState::default();
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 1_600 + ATTENTION_SILENCE_MS + 1),
            AgentBadge::Idle,
            "no observed working episode: nothing to report"
        );
    }

    #[test]
    fn focus_clears_green_and_prevents_rearming() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 10_000, false);
        let after = 11_000 + ATTENTION_SILENCE_MS + 1;
        assert_eq!(st.tick(true, &a.snapshot(), false, after), AgentBadge::Done);

        assert_eq!(
            st.tick(true, &a.snapshot(), true, after + 1_000),
            AgentBadge::Idle,
            "focusing the tab acknowledges the badge"
        );
        assert_eq!(
            st.tick(true, &a.snapshot(), false, after + 60_000),
            AgentBadge::Idle,
            "an acknowledged episode never re-arms green"
        );
    }

    #[test]
    fn replying_to_the_agent_clears_green() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 10_000, false);
        let after = 11_000 + ATTENTION_SILENCE_MS + 1;
        assert_eq!(st.tick(true, &a.snapshot(), false, after), AgentBadge::Done);

        a.stamp_input(after + 500);
        assert_eq!(
            st.tick(true, &a.snapshot(), false, after + 1_000),
            AgentBadge::Idle,
            "typing into the pane acknowledges the badge"
        );
    }

    #[test]
    fn green_does_not_arm_while_the_tab_is_focused() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 10_000, true);
        let after = 11_000 + ATTENTION_SILENCE_MS + 1;
        assert_eq!(
            st.tick(true, &a.snapshot(), true, after),
            AgentBadge::Idle,
            "the user is already looking at the pane"
        );
        assert_eq!(
            st.tick(true, &a.snapshot(), false, after + 10_000),
            AgentBadge::Idle,
            "switching away after seeing the end must not arm green"
        );
    }

    #[test]
    fn agent_leaving_drops_the_badge_after_a_grace_tick() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 10_000, false);
        let after = 11_000 + ATTENTION_SILENCE_MS + 1;
        assert_eq!(st.tick(true, &a.snapshot(), false, after), AgentBadge::Done);
        // First absent tick: tolerated (possibly blind probe), the state holds.
        assert_eq!(
            st.tick(false, &a.snapshot(), false, after + 1_000),
            AgentBadge::Done
        );
        assert_eq!(
            st.tick(false, &a.snapshot(), false, after + 2_000),
            AgentBadge::None,
            "the agent really exited: no badge at all"
        );
    }

    #[test]
    fn a_single_blind_probe_tick_preserves_a_pending_green() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 10_000, false);
        let after = 11_000 + ATTENTION_SILENCE_MS + 1;
        assert_eq!(st.tick(true, &a.snapshot(), false, after), AgentBadge::Done);

        // Transient probe failure (a single tick without an agent): the pending
        // green must not be auto-acknowledged by the wipe.
        st.tick(false, &a.snapshot(), false, after + 1_000);
        assert_eq!(
            st.tick(true, &a.snapshot(), false, after + 2_000),
            AgentBadge::Done,
            "a one-tick probe failure must not lose the pending green"
        );
    }

    #[test]
    fn a_short_real_turn_arms_green_regardless_of_tick_phase() {
        // Real 2.5 s turn seen by only two offset ticks (unfavorable phase):
        // the bounds stamped on the reader's output give the real span —
        // measured by the tick clock, the episode would be 1 s and miss the
        // floor.
        let a = PaneActivity::default();
        busy_run(&a, 1_000, 2_500);
        let mut st = PaneAgentState::default();
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 2_400),
            AgentBadge::Working
        );
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 3_400),
            AgentBadge::Working
        );
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 3_500 + ATTENTION_SILENCE_MS + 1),
            AgentBadge::Done,
            "a real 2.5s turn must arm green whatever the tick phase"
        );
    }

    #[test]
    fn a_burst_just_under_the_floor_never_arms_green() {
        // 1.9 s of sustained output (large banner): below the
        // ATTENTION_MIN_WORK_MS floor, never green — the lower bound of the
        // banner/short-turn trade-off (D agent-badge, revised).
        let a = PaneActivity::default();
        busy_run(&a, 1_000, 1_900);
        let mut st = PaneAgentState::default();
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 2_000),
            AgentBadge::Working
        );
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 2_900 + ATTENTION_SILENCE_MS + 1),
            AgentBadge::Idle,
            "a sub-2s burst stays under the green floor"
        );
    }

    #[test]
    fn a_heavy_redraw_after_an_acked_turn_does_not_rearm_green() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 6_000, false);
        // The user sees the end of the turn: episode acknowledged.
        assert_eq!(
            st.tick(true, &a.snapshot(), true, 7_000 + SILENCE_MS + 100),
            AgentBadge::Idle
        );

        // Heavy redraw < 6 s after the ack (resize: ≥ 1 s, copious) — it must
        // not merge into the acknowledged episode nor re-arm the green.
        busy_run(&a, 10_000, 1_500);
        assert_eq!(
            st.tick(true, &a.snapshot(), false, 11_500),
            AgentBadge::Working
        );
        assert_eq!(
            st.tick(
                true,
                &a.snapshot(),
                false,
                11_500 + ATTENTION_SILENCE_MS + 1
            ),
            AgentBadge::Idle,
            "a post-ack resize redraw must not re-arm a seen completion"
        );
    }

    #[test]
    fn a_resize_repaint_does_not_re_arm_working_after_done() {
        // Agent finished (green). Opening the agents dashboard then returning to
        // the worktree resizes the PTY each way; the agent's TUI repaints on
        // SIGWINCH — a burst helm caused, not new work. Stamped as a resize, the
        // burst is suppressed and the badge stays Done.
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 10_000, false);
        let after = 11_000 + ATTENTION_SILENCE_MS + 1;
        assert_eq!(st.tick(true, &a.snapshot(), false, after), AgentBadge::Done);

        a.stamp_resize(after + 1_000);
        busy_run(&a, after + 1_050, 600);
        assert_eq!(
            st.tick(true, &a.snapshot(), false, after + 2_000),
            AgentBadge::Done,
            "a resize repaint must not flip a finished agent back to Working"
        );
    }

    #[test]
    fn working_resumes_from_done_when_output_restarts() {
        let a = PaneActivity::default();
        let mut st = PaneAgentState::default();
        drive_run(&mut st, &a, 1_000, 10_000, false);
        let after = 11_000 + ATTENTION_SILENCE_MS + 1;
        assert_eq!(st.tick(true, &a.snapshot(), false, after), AgentBadge::Done);

        assert_eq!(
            drive_run(&mut st, &a, after + 1_000, 3_000, false),
            AgentBadge::Working
        );
    }
}
