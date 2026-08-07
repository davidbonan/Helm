//! Screenshot generator for the README (run on demand, not part of the gate):
//!   cargo test --features headless-verify --test shots_gen -- --nocapture
//! Renders the app's surfaces with authentic terminal content fed through the real
//! emulator, and saves PNGs under verify-artifacts/shots/ — plus the frame sequence
//! the agents-wall GIF is encoded from. Deterministic — no PTY, no timing.
#![cfg(feature = "headless-verify")]

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::agent_watch::AgentBadge;
use helm::ai::AiProvider;
use helm::git::commit_detail::{CommitDetail, CommitFile, CommitMeta};
use helm::git::conflict::{ConflictFile, ConflictKind, LineEnding, Region};
use helm::git::diff::{DiffLine, FileDiff, Hunk, LineOrigin};
use helm::git::graph::{Graph, GraphCommit, GraphRef, LaneCache, RefKind};
use helm::git::rebase::RebaseCommit;
use helm::git::status::{ChangeKind, FileEntry, OpSummary, RepoStatus};
use helm::git::sync::PullDefault;
use helm::git::worktree::{WorktreeSource, WorktreeSourceKind};
use helm::keybindings::Keymap;
use helm::pull_requests::runner::SourceStatus;
use helm::terminal::emu::{self, SharedTerm, DEFAULT_FONT_SIZE};
use helm::terminal::layout::{Layout, Orient, PaneId};
use helm::terminal::links::Editor;
use helm::terminal::palette::TermPalette;
use helm::theme::Palette;
use helm::ui::agents_view::{agents_page, AgentRow, WallView};
use helm::ui::ai_rebase_modal::{ai_rebase_modal, AiRebasePage};
use helm::ui::conflict_view::{conflict_view, ConflictEditorState};
use helm::ui::diff_view::{diff_view, DiffSurface, DiffViewState};
use helm::ui::file_list::FileMenuOutput;
use helm::ui::git_panel::{GitIntent, GitPanelState};
use helm::ui::graph_toolbar::{graph_toolbar, ToolbarState};
use helm::ui::graph_view::{graph_view, BranchEditor, GraphSearch, GraphViewState};
use helm::ui::preferences::{
    preferences_page, KeyboardState, PrSourcesView, PreferencesSection, UpdatesView,
};
use helm::ui::repo_sidebar::{
    create_worktree_modal, repo_sidebar, CreateSelection, CreateWorktreeModalAction,
    CreateWorktreePrompt, CreateWorktreeState, ProjectHeader, ProjectVisibility, RepoRow,
    SidebarAction, SidebarItem,
};
use helm::ui::tab_bar::{tab_bar, TabBarAction};
use helm::ui::terminal_view::{terminal_tree, terminal_view};
use helm::ui::{central_switch, root_layout};
use helm::update::UpdateState;
use helm::workspace_launcher::WorkspaceOpener;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct Spec {
    repo: &'static str,
    branch: &'static str,
    tab: &'static str,
    agent: &'static str,
    badge: AgentBadge,
    detail: &'static str,
    /// Project color index — the worktrees of one project share it, so their chips and
    /// wall bands carry the same hue.
    lane: usize,
    body: &'static [&'static str],
    /// Grid row (0-based) the cursor is parked on after the body is fed — the
    /// preview anchors its chrome cut on it, so it must sit in the composer.
    /// `None` leaves the cursor where the body ended.
    cursor_row: Option<usize>,
}

fn term(rows: u16, cols: u16, lines: &[&str]) -> SharedTerm {
    let t = emu::shared_term(rows, cols);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            emu::feed(&t, b"\r\n");
        }
        emu::feed(&t, line.as_bytes());
    }
    t
}

fn specs() -> Vec<Spec> {
    vec![
        Spec {
            repo: "helm",
            branch: "main",
            tab: "Tab 1",
            agent: "claude",
            badge: AgentBadge::Working,
            detail: "Working…",
            lane: 0,
            body: &[
                "\x1b[2mhelm  ~/dev/helm  main\x1b[0m",
                "$ claude",
                "\x1b[38;5;208mClaude Code\x1b[0m  \x1b[2mv2.1.162 · opus\x1b[0m",
                "",
                "\x1b[1m>\x1b[0m reuse freed lanes in the git graph allocator",
                "",
                "\x1b[32m⏺\x1b[0m Read \x1b[36msrc/git/graph.rs\x1b[0m \x1b[2m(412 lines)\x1b[0m",
                "\x1b[32m⏺\x1b[0m Update \x1b[36msrc/git/graph.rs\x1b[0m \x1b[2m(+46 −18)\x1b[0m",
                "\x1b[32m⏺\x1b[0m Bash \x1b[2mcargo test graph\x1b[0m",
                "  \x1b[2mtest result: ok. 12 passed; 0 failed\x1b[0m",
                "",
                "\x1b[36m✻\x1b[0m \x1b[1mWorking…\x1b[0m \x1b[2m(esc to interrupt · 41s · ↓ 2.3k tokens)\x1b[0m",
            ],
            cursor_row: None,
        },
        // A second agent on the same worktree: its column holds two, so the header
        // carries the count and this one stays a collapsed preview under the
        // expanded Working card above it.
        Spec {
            repo: "helm",
            branch: "main",
            tab: "Tab 2",
            agent: "aider",
            badge: AgentBadge::Idle,
            detail: "Idle",
            lane: 0,
            body: &[
                "\x1b[2mhelm  ~/dev/helm  main\x1b[0m",
                "$ aider --model sonnet",
                "\x1b[2maider v0.86.1 · git repo · 214 files\x1b[0m",
                "",
                "\x1b[1m>\x1b[0m separate two stacked agent cards with a hairline",
                "",
                "\x1b[36msrc/ui/agents_view.rs\x1b[0m",
                "\x1b[31m-    ui.add_space(AGENT_CARD_GAP);\x1b[0m",
                "\x1b[32m+    hairline(ui, palette);\x1b[0m",
                "\x1b[2mApplied edit to src/ui/agents_view.rs\x1b[0m",
                "\x1b[2mcargo test ui_agents_view … 21 passed\x1b[0m",
                "",
                "\x1b[1m>\x1b[0m ",
            ],
            cursor_row: Some(12),
        },
        Spec {
            repo: "helm",
            branch: "agents-dashboard",
            tab: "Tab 1",
            agent: "codex",
            badge: AgentBadge::Done,
            detail: "Finished 3m ago",
            lane: 0,
            // Codex leaves its startup chrome on screen: a top "update available"
            // banner box and a boxed session-info banner, both box-framed. Its
            // composer is a *bare* prompt line (no box) with a status line under it.
            // The condensed preview must drop the top banners and the bottom
            // composer / status — anchoring on the cursor, not on a box — and show
            // only the conversation between them.
            body: &[
                "\x1b[2mhelm  ~/dev/helm.worktrees/agents  agents-dashboard\x1b[0m",
                "╭──────────────────────────────────────────────────────────────╮",
                "│ \x1b[36m✦ Update available!\x1b[0m 0.139.0 → 0.141.0  ·  npm i -g @openai/codex │",
                "╰──────────────────────────────────────────────────────────────╯",
                "",
                "╭──────────────────────────────────────────────────────────────╮",
                "│ \x1b[1m>_ OpenAI Codex\x1b[0m \x1b[2m(v0.139.0)\x1b[0m · gpt-5.5 xhigh · ~/dev/helm-studio │",
                "╰──────────────────────────────────────────────────────────────╯",
                "",
                "  \x1b[2mTip: use /fast to enable our fastest inference\x1b[0m",
                "",
                "\x1b[34m•\x1b[0m explain the worktree grouping in the sidebar",
                "  \x1b[2mhelm groups a root repo with its linked worktrees under one\x1b[0m",
                "  \x1b[2msidebar header, so their branches read together.\x1b[0m",
                "\x1b[34m•\x1b[0m now add an uncommitted ratio bar to each worktree row",
                "  \x1b[2mDone — a green/red proportion bar mirroring the sidebar.\x1b[0m",
                "  \x1b[2mcargo test ui_agents_view … ok\x1b[0m",
                "",
                "\x1b[1m›\x1b[0m summarize recent commits",
                "  \x1b[2mgpt-5.5 xhigh · ~/dev/helm-studio\x1b[0m",
            ],
            cursor_row: Some(18),
        },
        Spec {
            repo: "api",
            branch: "main",
            tab: "Tab 2",
            agent: "claude",
            badge: AgentBadge::Working,
            detail: "Working…",
            lane: 1,
            // A full-screen TUI at the pane's real 110-col width, ending in Claude
            // Code's bottom chrome block — a multi-row boxed composer plus mode /
            // hint / status lines under it: the condensed preview must detect and
            // drop that whole block and show only the conversation above it.
            body: &[
                "\x1b[2mapi  ~/dev/api  main\x1b[0m",
                "\x1b[38;5;208mClaude Code\x1b[0m  \x1b[2mv2.1.162 · opus\x1b[0m",
                "",
                "\x1b[1m>\x1b[0m run the full test suite, fix failures, update the changelog",
                "",
                "\x1b[32m⏺\x1b[0m Bash \x1b[2mcargo test --workspace\x1b[0m",
                "  \x1b[2mtest tests::health::returns_200 … ok\x1b[0m",
                "  \x1b[31mtest tests::billing::prorates_midcycle … FAILED (4200 ≠ 5000)\x1b[0m",
                "\x1b[32m⏺\x1b[0m Update \x1b[36msrc/billing/proration.rs\x1b[0m \x1b[2m(+18 −7) — clamp the ratio\x1b[0m",
                "",
                "╭──────────────────────────────────────────────────────────────╮",
                "│ > _                                                          │",
                "│                                                              │",
                "╰──────────────────────────────────────────────────────────────╯",
                "  \x1b[2m⏵⏵ accept edits on (shift+tab to cycle)\x1b[0m",
                "  \x1b[2m? for shortcuts\x1b[0m",
                "  \x1b[36m✻ Crunching…\x1b[0m \x1b[2m(esc to interrupt · 1m24s · ↓ 6.2k tokens)\x1b[0m",
            ],
            cursor_row: Some(11),
        },
    ]
}

/// Replaces the working agent's spinner line once its turn lands: the transcript, the
/// chip, the tile band and the sidebar badge all have to tell the same story.
const FINISHED_TAIL: &[&str] = &[
    "  \x1b[2mLanes are reused now — a freed lane goes to the next commit, so\x1b[0m",
    "  \x1b[2mthe layout stops drifting right. 12 passed, clippy clean.\x1b[0m",
    "",
    "\x1b[1m>\x1b[0m ",
];

/// Width the app gives the project sidebar (`root_layout`'s default).
const SIDEBAR_W: f32 = 248.0;

/// `shown` lists the rows the wall mirrors, in the order they were put on it; `seam`
/// nudges the root split's ratio by that much (the seam starts at a fresh 50/50).
/// `finished` lands the first agent's turn: its badge goes green everywhere at once.
/// `phase` offsets how many frames the harness settles, so the Working spinner sits at
/// a different angle in each beat instead of freezing across the sequence.
fn render(
    selected: Option<usize>,
    shown: &[usize],
    seam: f32,
    finished: bool,
    phase: usize,
    size: egui::Vec2,
    out: &str,
) {
    let palette = Palette::dark();
    let term_pal = TermPalette::dark();
    let data = specs();
    let grids: Vec<SharedTerm> = data
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut body = s.body.to_vec();
            if finished && i == 0 {
                body.pop();
                body.extend_from_slice(FINISHED_TAIL);
            }
            let t = term(22, 110, &body);
            if let Some(row) = s.cursor_row {
                emu::feed(&t, format!("\x1b[{};1H", row + 1).as_bytes());
            }
            t
        })
        .collect();

    // The wall's own tree: one slot per shown agent, split the way the app splits it —
    // over the area the page really gets, the window minus the project sidebar.
    let area = helm::terminal::layout::Rect {
        x: 0.0,
        y: 0.0,
        w: size.x - SIDEBAR_W,
        h: size.y,
    };
    let mut wall = helm::agents_wall::AgentWall::new();
    for row in shown {
        wall.show(*row, area);
    }
    // Same seam the user drags: the root split, pinned by the first slot and the one
    // that halved it.
    if let ([(first, _), (second, _), ..], true) = (wall.slots(), seam != 0.0) {
        let (first, second) = (*first, *second);
        // Only the min-size clamp reads the cell metrics, so a bare context measuring
        // the real monospace font is enough here.
        let ctx = egui::Context::default();
        helm::theme::install_fonts(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let (cell_w, cell_h) = helm::ui::terminal_view::cell_metrics(&ctx, DEFAULT_FONT_SIZE);
        if let Some(layout) = wall.layout_mut() {
            layout.resize_split(first, second, seam, area, cell_w, cell_h);
        }
    }
    let slots: Vec<(PaneId, usize)> = wall.slots().to_vec();
    let wall_layout = wall.layout().cloned();
    let wall_full = wall.full();

    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let rows: Vec<AgentRow> = data
                .iter()
                .enumerate()
                .map(|(i, s)| AgentRow {
                    repo: s.repo,
                    branch: Some(s.branch),
                    tab: s.tab,
                    agent: s.agent,
                    badge: if finished && i == 0 {
                        AgentBadge::Done
                    } else {
                        s.badge
                    },
                    detail: if finished && i == 0 {
                        "Finished just now".to_owned()
                    } else {
                        s.detail.to_owned()
                    },
                    // The shots hold steady states; the arrival flash is a transient
                    // that would freeze into a stray ring across the beat's frames.
                    done_ago_ms: None,
                    lane: s.lane,
                })
                .collect();
            // Where the agents come from: the same project tree as every full-window
            // shot, Agents selected, and the `helm` rows carrying the turn's state.
            let mut items = demo_items();
            if finished {
                for item in &mut items {
                    match item {
                        SidebarItem::Header(h) if h.root == 0 => h.agent = AgentBadge::Done,
                        SidebarItem::Row(r) if r.index == 0 => r.agent = AgentBadge::Done,
                        _ => {}
                    }
                }
            }
            let projects = demo_projects();
            let keymap = Keymap::default();
            let mut sidebar_out = SidebarAction::default();
            egui::Panel::left("wall_sidebar")
                .exact_size(SIDEBAR_W)
                .frame(
                    egui::Frame::new()
                        .fill(palette.bg_sidebar)
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 10,
                            top: 40,
                            bottom: 10,
                        }),
                )
                .show_inside(ui, |ui| {
                    repo_sidebar(
                        ui,
                        &palette,
                        &items,
                        &CHILD_FLAGS,
                        &projects,
                        Some(0),
                        if finished {
                            AgentBadge::Done
                        } else {
                            AgentBadge::Working
                        },
                        true,
                        &[],
                        0,
                        false,
                        &keymap,
                        &mut sidebar_out,
                    );
                });
            let wall = WallView {
                layout: wall_layout.as_ref(),
                slots: &slots,
                full: wall_full,
            };
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(palette.bg_canvas))
                .show_inside(ui, |ui| {
                    agents_page(ui, &palette, &rows, selected, &wall, |idx, tui| {
                        terminal_view(
                            tui,
                            &grids[idx],
                            &term_pal,
                            DEFAULT_FONT_SIZE,
                            selected == Some(idx),
                            false,
                            None,
                            None,
                        )
                        .clicked
                    });
                });
        });

    // A Working row repaints forever (spinner) — settle a fixed number of frames.
    for _ in 0..6 + phase {
        harness.step();
    }
    std::fs::create_dir_all("verify-artifacts/shots").unwrap();
    harness
        .render()
        .expect("wgpu render")
        .save(format!("verify-artifacts/shots/{out}.png"))
        .unwrap();
}

/// One beat of the wall animation: the state its frame renders, and how long the GIF
/// holds it (centiseconds — the format's own unit).
struct Beat {
    shown: &'static [usize],
    /// Ratio the root seam has moved from its 50/50 start.
    seam: f32,
    /// The watched agent's turn has landed: Working → Done, sidebar included.
    finished: bool,
    hold_cs: u32,
}

/// The wall filling up — empty, then one agent picked from the strip, then two, then
/// three — the root seam dragged wider on the one being watched, and its turn landing.
const WALL_BEATS: &[Beat] = &[
    Beat {
        shown: &[],
        seam: 0.0,
        finished: false,
        hold_cs: 110,
    },
    Beat {
        shown: &[0],
        seam: 0.0,
        finished: false,
        hold_cs: 120,
    },
    Beat {
        shown: &[0, 1],
        seam: 0.0,
        finished: false,
        hold_cs: 120,
    },
    Beat {
        shown: &[0, 1, 2],
        seam: 0.0,
        finished: false,
        hold_cs: 130,
    },
    Beat {
        shown: &[0, 1, 2],
        seam: 0.03,
        finished: false,
        hold_cs: 7,
    },
    Beat {
        shown: &[0, 1, 2],
        seam: 0.06,
        finished: false,
        hold_cs: 7,
    },
    Beat {
        shown: &[0, 1, 2],
        seam: 0.09,
        finished: false,
        hold_cs: 7,
    },
    Beat {
        shown: &[0, 1, 2],
        seam: 0.11,
        finished: false,
        hold_cs: 7,
    },
    Beat {
        shown: &[0, 1, 2],
        seam: 0.12,
        finished: false,
        hold_cs: 110,
    },
    Beat {
        shown: &[0, 1, 2],
        seam: 0.12,
        finished: true,
        hold_cs: 180,
    },
];

#[test]
fn gen_agents_wall_frames() {
    let dir = "verify-artifacts/shots/agents-wall";
    std::fs::create_dir_all(dir).unwrap();
    let mut list = String::new();
    for (i, beat) in WALL_BEATS.iter().enumerate() {
        render(
            beat.shown.last().copied(),
            beat.shown,
            beat.seam,
            beat.finished,
            i,
            // Tall enough that the second split lands horizontal: the wall reads as a
            // column plus two stacked tiles, the shape the app gives a 4-up wall.
            egui::vec2(1440.0, 900.0),
            &format!("agents-wall/frame-{i:02}"),
        );
        let secs = f64::from(beat.hold_cs) / 100.0;
        list.push_str(&format!("file 'frame-{i:02}.png'\nduration {secs:.2}\n"));
    }
    // The concat demuxer drops the last entry's duration unless the file repeats.
    let last = WALL_BEATS.len() - 1;
    list.push_str(&format!("file 'frame-{last:02}.png'\n"));
    std::fs::write(format!("{dir}/frames.txt"), list).unwrap();
    println!("{} frames + frames.txt in {dir}", WALL_BEATS.len());
}

fn hero_term() -> SharedTerm {
    term(
        40,
        120,
        &[
            "\x1b[2mhelm  ~/dev/helm  main\x1b[0m",
            "$ claude",
            "\x1b[38;5;208mClaude Code\x1b[0m  \x1b[2mv2.1.162 · opus\x1b[0m",
            "",
            "\x1b[1m>\x1b[0m reuse freed lanes in the git graph allocator",
            "",
            "\x1b[32m⏺\x1b[0m Read \x1b[36msrc/git/graph.rs\x1b[0m \x1b[2m(412 lines)\x1b[0m",
            "\x1b[32m⏺\x1b[0m Update \x1b[36msrc/git/graph.rs\x1b[0m \x1b[2m(+46 −18)\x1b[0m",
            "  \x1b[2m⎿ pop the free list before growing next_id\x1b[0m",
            "\x1b[32m⏺\x1b[0m Update \x1b[36msrc/ui/git_graph.rs\x1b[0m \x1b[2m(+8 −2)\x1b[0m",
            "\x1b[32m⏺\x1b[0m Bash \x1b[2mcargo test graph\x1b[0m",
            "  \x1b[2mtest result: ok. 12 passed; 0 failed\x1b[0m",
            "",
            "\x1b[36m✻\x1b[0m \x1b[1mWorking…\x1b[0m \x1b[2m(esc to interrupt · 41s · ↓ 2.3k tokens)\x1b[0m",
        ],
    )
}

fn hero_status() -> RepoStatus {
    let entry = |path: &str, kind, additions, deletions| FileEntry {
        path: path.to_owned(),
        kind,
        additions,
        deletions,
    };
    RepoStatus {
        staged: vec![
            entry("src/git/graph.rs", ChangeKind::Modified, 46, 18),
            entry("src/ui/git_graph.rs", ChangeKind::Modified, 8, 2),
        ],
        unstaged: vec![
            entry("src/git/graph.rs", ChangeKind::Modified, 5, 1),
            entry("tests/graph_alloc.rs", ChangeKind::Added, 37, 0),
        ],
    }
}

/// The README money shot (specs/screenshots/README.md): the three zones at once
/// — project sidebar with agent badges + dirty stats, a live agent terminal, the
/// git staging sidebar with a drafted commit.
fn render_hero(out: &str) {
    let palette = Palette::dark();
    let term_pal = TermPalette::dark();
    let grid = hero_term();
    let status = hero_status();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            helm::theme::install_fonts(ui.ctx());
            // The real app's visuals (panel_fill = bg_canvas, text/selection
            // colors): the central zone behind the terminal then matches the
            // navy sidebars instead of egui's default neutral-gray panel fill.
            helm::theme::apply(ui.ctx(), helm::theme::ThemeMode::Dark, "helm", "helm");
            // Force the side panels fully open on the first frame instead of
            // animating in over several steps.
            let mut style = (*ui.ctx().global_style()).clone();
            style.animation_time = 0.0;
            ui.ctx().set_global_style(style);

            let keymap = Keymap::default();
            let items = vec![
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "helm",
                    path: "~/dev/helm",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: true,
                    agent: AgentBadge::Working,
                }),
                SidebarItem::Row(RepoRow {
                    index: 0,
                    name: "helm",
                    path: "~/dev/helm",
                    missing: false,
                    main: true,
                    branch: Some("main"),
                    deleting: false,
                    agent: AgentBadge::Working,
                    stats: Some((96, 21)),
                }),
                SidebarItem::Row(RepoRow {
                    index: 1,
                    name: "agents",
                    path: "~/dev/helm.worktrees/agents",
                    missing: false,
                    main: false,
                    branch: Some("agents-dashboard"),
                    deleting: false,
                    agent: AgentBadge::Done,
                    stats: Some((57, 9)),
                }),
                SidebarItem::Header(ProjectHeader {
                    root: 2,
                    name: "api",
                    path: "~/dev/api",
                    collapsed: false,
                    lane: 1,
                    can_create_worktree: true,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 2,
                    name: "api",
                    path: "~/dev/api",
                    missing: false,
                    main: true,
                    branch: Some("main"),
                    deleting: false,
                    agent: AgentBadge::Idle,
                    stats: None,
                }),
            ];
            let child_flags = [false, true, false];
            let projects = [
                ProjectVisibility {
                    root: 0,
                    name: "helm",
                    hidden: false,
                },
                ProjectVisibility {
                    root: 2,
                    name: "api",
                    hidden: false,
                },
            ];

            let mut git_state = GitPanelState {
                subject: "Reuse freed lanes in the git graph allocator".to_owned(),
                description: "Pop the free list before growing next_id so a closed pane \
                    returns its lane."
                    .to_owned(),
                ..Default::default()
            };
            let mut intents: Vec<GitIntent> = Vec::new();
            let mut sidebar = SidebarAction::default();
            let mut file_menu = FileMenuOutput::default();
            let mut open_commit_file = None;
            let mut open_workspace = None;
            let mut open_prefs = false;
            let mut open_feedback = false;
            let mut show_workspace = true;
            let mut show_git = true;
            let layout = Layout::new();

            root_layout(
                ui,
                &palette,
                &items,
                &child_flags,
                &projects,
                Some(0),
                "main",
                &status,
                false,
                None,
                &mut git_state,
                &mut intents,
                &mut show_workspace,
                &mut show_git,
                false,
                None,
                false,
                None,
                &mut open_commit_file,
                None,
                &mut file_menu,
                helm::ui::file_list::FileViewMode::default(),
                WorkspaceOpener::default(),
                &[],
                &mut open_workspace,
                &mut open_prefs,
                &mut open_feedback,
                AgentBadge::None,
                false,
                &[],
                0,
                false,
                false,
                &mut false,
                &mut sidebar,
                248.0,
                300.0,
                &keymap,
                false,
                false,
                0.0,
                |_ui| {},
                |ui| {
                    central_switch(ui, &palette, false, &keymap, None, None, true);
                    let mut rename = None;
                    let mut tab_action = TabBarAction::default();
                    tab_bar(
                        ui,
                        &palette,
                        &[String::from("Tab 1")],
                        0,
                        &mut rename,
                        &keymap,
                        &mut tab_action,
                    );
                    terminal_tree(ui, &layout, &palette, |ui, _id, focused| {
                        terminal_view(
                            ui,
                            &grid,
                            &term_pal,
                            DEFAULT_FONT_SIZE,
                            focused,
                            false,
                            None,
                            None,
                        );
                        false
                    });
                },
            );
        });

    // A Working badge + the agent spinner repaint forever — settle a fixed
    // number of frames before capturing.
    for _ in 0..6 {
        harness.step();
    }
    std::fs::create_dir_all("verify-artifacts/shots").unwrap();
    harness
        .render()
        .expect("wgpu render")
        .save(format!("verify-artifacts/shots/{out}.png"))
        .unwrap();
}

#[test]
fn gen_hero() {
    render_hero("hero");
}

// ───────────────────────────── shared scaffolding ─────────────────────────────

const CHILD_FLAGS: [bool; 3] = [false, true, false];

/// The demo project tree shared by every full-window shot: the `helm` project
/// (main worktree + an `agents` linked worktree) and a second `api` project.
fn demo_items() -> Vec<SidebarItem<'static>> {
    vec![
        SidebarItem::Header(ProjectHeader {
            root: 0,
            name: "helm",
            path: "~/dev/helm",
            collapsed: false,
            lane: 0,
            can_create_worktree: true,
            agent: AgentBadge::Working,
        }),
        SidebarItem::Row(RepoRow {
            index: 0,
            name: "helm",
            path: "~/dev/helm",
            missing: false,
            main: true,
            branch: Some("main"),
            deleting: false,
            agent: AgentBadge::Working,
            stats: Some((96, 21)),
        }),
        SidebarItem::Row(RepoRow {
            index: 1,
            name: "agents",
            path: "~/dev/helm.worktrees/agents",
            missing: false,
            main: false,
            branch: Some("agents-dashboard"),
            deleting: false,
            agent: AgentBadge::Done,
            stats: Some((57, 9)),
        }),
        SidebarItem::Header(ProjectHeader {
            root: 2,
            name: "api",
            path: "~/dev/api",
            collapsed: false,
            lane: 1,
            can_create_worktree: true,
            agent: AgentBadge::None,
        }),
        SidebarItem::Row(RepoRow {
            index: 2,
            name: "api",
            path: "~/dev/api",
            missing: false,
            main: true,
            branch: Some("main"),
            deleting: false,
            agent: AgentBadge::Idle,
            stats: None,
        }),
    ]
}

fn demo_projects() -> [ProjectVisibility<'static>; 2] {
    [
        ProjectVisibility {
            root: 0,
            name: "helm",
            hidden: false,
        },
        ProjectVisibility {
            root: 2,
            name: "api",
            hidden: false,
        },
    ]
}

/// Real-app visuals + zeroed animation so the panels are fully open on frame 0
/// (panel_fill = bg_canvas, so the central zone matches the navy sidebars).
fn boot(ui: &mut egui::Ui) {
    helm::theme::install_fonts(ui.ctx());
    helm::theme::apply(ui.ctx(), helm::theme::ThemeMode::Dark, "helm", "helm");
    let mut style = (*ui.ctx().global_style()).clone();
    style.animation_time = 0.0;
    ui.ctx().set_global_style(style);
}

/// Settle the spinners (Working badge, agent line) over a fixed number of frames,
/// then save the deterministic capture.
fn finish<S>(mut harness: Harness<'_, S>, out: &str) {
    for _ in 0..6 {
        harness.step();
    }
    std::fs::create_dir_all("verify-artifacts/shots").unwrap();
    harness
        .render()
        .expect("wgpu render")
        .save(format!("verify-artifacts/shots/{out}.png"))
        .unwrap();
}

/// The three-zone app shell with the shared demo project tree; the caller owns
/// the central area and chooses the right-sidebar mode (git panel vs commit
/// detail) via `show_commit_detail` / `commit_detail`.
#[allow(clippy::too_many_arguments)]
fn app_shell(
    ui: &mut egui::Ui,
    palette: &Palette,
    keymap: &Keymap,
    status: &RepoStatus,
    git_state: &mut GitPanelState,
    op: Option<&OpSummary>,
    show_commit_detail: bool,
    commit_detail: Option<&CommitDetail>,
    central: impl FnOnce(&mut egui::Ui),
) {
    let items = demo_items();
    let projects = demo_projects();
    let mut intents: Vec<GitIntent> = Vec::new();
    let mut sidebar = SidebarAction::default();
    let mut file_menu = FileMenuOutput::default();
    let mut open_commit_file = None;
    let mut open_workspace = None;
    let mut open_prefs = false;
    let mut open_feedback = false;
    let mut show_workspace = true;
    let mut show_git = true;
    root_layout(
        ui,
        palette,
        &items,
        &CHILD_FLAGS,
        &projects,
        Some(0),
        "main",
        status,
        op.is_some(),
        op,
        git_state,
        &mut intents,
        &mut show_workspace,
        &mut show_git,
        show_commit_detail,
        commit_detail,
        false,
        None,
        &mut open_commit_file,
        None,
        &mut file_menu,
        helm::ui::file_list::FileViewMode::default(),
        WorkspaceOpener::default(),
        &[],
        &mut open_workspace,
        &mut open_prefs,
        &mut open_feedback,
        AgentBadge::None,
        false,
        &[],
        0,
        false,
        false,
        &mut false,
        &mut sidebar,
        248.0,
        300.0,
        keymap,
        false,
        false,
        0.0,
        |_ui| {},
        central,
    );
}

// ──────────────────────────── curated git fixtures ────────────────────────────

fn oid(n: u8) -> git2::Oid {
    git2::Oid::from_str(&format!("{n:040x}")).unwrap()
}

/// A small history with a `feature/login` branch merged back into `main`, a
/// release tag, and a fork at the Run-strip commit — enough lanes to read.
fn demo_graph() -> Graph {
    let t = 1_718_500_000_i64;
    let day = 86_400_i64;
    let local = |name: &str, head: bool| GraphRef {
        name: name.to_owned(),
        kind: RefKind::Local,
        is_head: head,
        also_remote: head,
        counterpart: None,
        worktree_available: false,
    };
    let tag = |name: &str| GraphRef {
        name: name.to_owned(),
        kind: RefKind::Tag,
        is_head: false,
        also_remote: false,
        counterpart: None,
        worktree_available: false,
    };
    let commit =
        |n: u8, short: &str, summary: &str, parents: &[u8], refs: Vec<GraphRef>, time: i64| {
            GraphCommit {
                oid: oid(n),
                short_id: short.to_owned(),
                summary: summary.to_owned(),
                body: String::new(),
                author: "David Bonan".to_owned(),
                time,
                parents: parents.iter().map(|&p| oid(p)).collect(),
                refs,
                stash: false,
            }
        };
    Graph {
        commits: vec![
            commit(
                1,
                "7da5c78",
                "Merge branch 'feature/login'",
                &[2, 5],
                vec![local("main", true)],
                t,
            ),
            commit(
                2,
                "9d1336d",
                "Refresh sidebar branch/dirty stats off the UI thread",
                &[3],
                vec![],
                t - day,
            ),
            commit(
                5,
                "a4f9e21",
                "Add the login form + inline validation",
                &[7],
                vec![local("feature/login", false)],
                t - 2 * day,
            ),
            commit(
                3,
                "ed5fca4",
                "Preview image files in the diff view",
                &[4],
                vec![],
                t - 3 * day,
            ),
            commit(
                7,
                "c08b3da",
                "Scaffold the auth module",
                &[4],
                vec![],
                t - 4 * day,
            ),
            commit(
                4,
                "89c5da6",
                "Add Run strip to the git sidebar",
                &[6],
                vec![],
                t - 5 * day,
            ),
            commit(
                6,
                "b27763b",
                "Release v0.8.4",
                &[],
                vec![tag("v0.8.4")],
                t - 6 * day,
            ),
        ],
        has_more: false,
    }
}

fn demo_toolbar() -> ToolbarState {
    ToolbarState {
        pull_default: PullDefault::default(),
        busy: None,
        has_remote: true,
        has_upstream: true,
        detached: false,
        unborn: false,
        dirty: false,
        stash_count: 0,
        git_missing: false,
    }
}

/// Detail for the selected `feature/login` tip (oid 5) — shown in the right
/// sidebar of the graph shot.
fn commit_detail_login() -> CommitDetail {
    CommitDetail {
        meta: CommitMeta {
            oid: oid(5),
            short_id: "a4f9e21".to_owned(),
            author: "David Bonan".to_owned(),
            email: "david@helm.dev".to_owned(),
            time: 1_718_327_600,
            offset_minutes: 120,
            committer: "David Bonan".to_owned(),
            summary: "Add the login form + inline validation".to_owned(),
            body: "Render the email/password fields with inline validation and wire the \
                submit handler to the session store."
                .to_owned(),
            parents: vec![oid(7)],
        },
        files: vec![
            CommitFile {
                path: "src/ui/login.rs".to_owned(),
                kind: ChangeKind::Added,
                additions: 124,
                deletions: 0,
            },
            CommitFile {
                path: "src/auth/session.rs".to_owned(),
                kind: ChangeKind::Modified,
                additions: 18,
                deletions: 4,
            },
            CommitFile {
                path: "src/app/mod.rs".to_owned(),
                kind: ChangeKind::Modified,
                additions: 9,
                deletions: 2,
            },
        ],
    }
}

/// A two-hunk diff matching the hero's "reuse freed lanes" narrative.
fn staging_diff() -> FileDiff {
    let ctx = |s: &str, old: u32, new: u32| DiffLine {
        origin: LineOrigin::Context,
        content: s.to_owned(),
        old_lineno: Some(old),
        new_lineno: Some(new),
    };
    let add = |s: &str, new: u32| DiffLine {
        origin: LineOrigin::Addition,
        content: s.to_owned(),
        old_lineno: None,
        new_lineno: Some(new),
    };
    let del = |s: &str, old: u32| DiffLine {
        origin: LineOrigin::Deletion,
        content: s.to_owned(),
        old_lineno: Some(old),
        new_lineno: None,
    };
    FileDiff {
        path: "src/git/graph.rs".to_owned(),
        binary: false,
        oversize: false,
        editable: true,
        hunks: vec![
            Hunk {
                header: "@@ -18,7 +18,9 @@ impl LaneCache {".to_owned(),
                old_start: 18,
                old_lines: 7,
                new_start: 18,
                new_lines: 9,
                lines: vec![
                    ctx("    fn alloc(&mut self) -> usize {", 18, 18),
                    del("        let id = self.next_id;", 19),
                    del("        self.next_id += 1;", 20),
                    add("        if let Some(id) = self.free.pop() {", 19),
                    add("            return id;", 20),
                    add("        }", 21),
                    add("        let id = self.next_id;", 22),
                    add("        self.next_id += 1;", 23),
                    ctx("        id", 21, 24),
                    ctx("    }", 22, 25),
                ],
            },
            Hunk {
                header: "@@ -41,5 +43,6 @@ impl LaneCache {".to_owned(),
                old_start: 41,
                old_lines: 5,
                new_start: 43,
                new_lines: 6,
                lines: vec![
                    ctx("    fn free(&mut self, id: usize) {", 41, 43),
                    ctx("        self.lanes[id] = None;", 42, 44),
                    add("        self.free.push(id);", 45),
                    ctx("    }", 43, 46),
                ],
            },
        ],
        source_lines: Vec::new(),
        image: None,
    }
}

/// One conflicted file with two conflict regions (ours/base/theirs).
fn conflict_files() -> Vec<ConflictFile> {
    vec![ConflictFile {
        path: "src/auth/session.rs".to_owned(),
        kind: ConflictKind::BothModified,
        ours_label: "HEAD (main)".to_owned(),
        theirs_label: "feature/login".to_owned(),
        regions: vec![
            Region::Stable(vec![
                "pub fn authenticate(req: &Request) -> Result<Session> {".to_owned(),
                "    let token = req.header(\"authorization\").ok_or(AuthError::Missing)?;"
                    .to_owned(),
            ]),
            Region::Conflict {
                ours: vec!["    let user = verify_jwt(token)?;".to_owned()],
                theirs: vec![
                    "    let user = verify_jwt(token)".to_owned(),
                    "        .map_err(|_| AuthError::Invalid)?;".to_owned(),
                ],
                base: vec!["    let user = decode_token(token)?;".to_owned()],
            },
            Region::Stable(vec!["    let session = Session::new(user);".to_owned()]),
            Region::Conflict {
                ours: vec!["    session.set_ttl(Duration::from_secs(3600));".to_owned()],
                theirs: vec!["    session.set_ttl(Duration::from_secs(86_400));".to_owned()],
                base: vec![],
            },
            Region::Stable(vec!["    Ok(session)".to_owned(), "}".to_owned()]),
        ],
        has_base: true,
        eol: LineEnding::default(),
        disk_divergence: None,
    }]
}

// ────────────────────────────── feature shots ──────────────────────────────

#[test]
fn gen_terminal() {
    let palette = Palette::dark();
    let term_pal = TermPalette::dark();
    let agent = term(
        30,
        86,
        &[
            "\x1b[2mhelm  ~/dev/helm  main\x1b[0m",
            "$ claude",
            "\x1b[38;5;208mClaude Code\x1b[0m  \x1b[2mv2.1.162 · opus\x1b[0m",
            "",
            "\x1b[1m>\x1b[0m reuse freed lanes in the git graph allocator",
            "",
            "\x1b[32m⏺\x1b[0m Read \x1b[36msrc/git/graph.rs\x1b[0m \x1b[2m(412 lines)\x1b[0m",
            "\x1b[32m⏺\x1b[0m Update \x1b[36msrc/git/graph.rs\x1b[0m \x1b[2m(+46 −18)\x1b[0m",
            "\x1b[32m⏺\x1b[0m Bash \x1b[2mcargo test graph\x1b[0m",
            "  \x1b[2mtest result: ok. 12 passed; 0 failed\x1b[0m",
            "",
            "\x1b[36m✻\x1b[0m \x1b[1mWorking…\x1b[0m \x1b[2m(esc to interrupt · 41s)\x1b[0m",
        ],
    );
    let build = term(
        14,
        56,
        &[
            "\x1b[2m~/dev/helm  cargo\x1b[0m",
            "$ cargo build",
            "\x1b[2m   Compiling helm v0.8.4\x1b[0m",
            "\x1b[32m    Finished\x1b[0m \x1b[2mdev in 8.41s\x1b[0m",
            "$ ",
        ],
    );
    let server = term(
        14,
        56,
        &[
            "\x1b[2m~/dev/api  main\x1b[0m",
            "$ cargo run",
            "\x1b[32m  listening\x1b[0m on http://127.0.0.1:8787",
            "\x1b[2m  GET /health  200  1ms\x1b[0m",
            "\x1b[2m  GET /users   200  7ms\x1b[0m",
            "$ ",
        ],
    );
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let keymap = Keymap::default();
            let status = hero_status();
            let mut git_state = GitPanelState::default();
            let mut layout = Layout::new();
            layout.split(Orient::Vertical); // pane 1 to the right of pane 0
            layout.split(Orient::Horizontal); // pane 2 below pane 1
            layout.set_focus(PaneId(0));
            app_shell(
                ui,
                &palette,
                &keymap,
                &status,
                &mut git_state,
                None,
                false,
                None,
                |ui| {
                    central_switch(ui, &palette, false, &keymap, Some("helm"), None, true);
                    let mut rename = None;
                    let mut tab_action = TabBarAction::default();
                    tab_bar(
                        ui,
                        &palette,
                        &[String::from("Tab 1"), String::from("Tab 2")],
                        0,
                        &mut rename,
                        &keymap,
                        &mut tab_action,
                    );
                    terminal_tree(ui, &layout, &palette, |ui, id, focused| {
                        let grid = match id.0 {
                            0 => &agent,
                            1 => &build,
                            _ => &server,
                        };
                        terminal_view(
                            ui,
                            grid,
                            &term_pal,
                            DEFAULT_FONT_SIZE,
                            focused,
                            false,
                            None,
                            None,
                        );
                        false
                    });
                },
            );
        });
    finish(harness, "terminal");
}

#[test]
fn gen_git_graph() {
    let palette = Palette::dark();
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let keymap = Keymap::default();
            let status = hero_status();
            let mut git_state = GitPanelState::default();
            let graph = demo_graph();
            let toolbar = demo_toolbar();
            let detail = commit_detail_login();
            app_shell(
                ui,
                &palette,
                &keymap,
                &status,
                &mut git_state,
                None,
                true,
                Some(&detail),
                |ui| {
                    central_switch(ui, &palette, true, &keymap, Some("helm"), None, true);
                    let mut editor = BranchEditor::default();
                    let mut lanes = LaneCache::default();
                    let mut search = GraphSearch::default();
                    graph_toolbar(ui, &palette, &toolbar, &mut editor);
                    let _ = graph_view(
                        ui,
                        &palette,
                        &GraphViewState {
                            graph: Some(&graph),
                            wip: None,
                            selected: Some(oid(5)),
                            scroll_to_head: false,
                            keyboard_nav: false,
                            can_pull_request: false,
                        },
                        &mut lanes,
                        &mut editor,
                        &mut search,
                    );
                },
            );
        });
    finish(harness, "git_graph");
}

#[test]
fn gen_git_staging() {
    let palette = Palette::dark();
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let keymap = Keymap::default();
            let status = hero_status();
            let mut git_state = GitPanelState {
                subject: "Reuse freed lanes in the git graph allocator".to_owned(),
                description: "Pop the free list before growing next_id so a closed pane \
                    returns its lane."
                    .to_owned(),
                ..Default::default()
            };
            let file = staging_diff();
            app_shell(
                ui,
                &palette,
                &keymap,
                &status,
                &mut git_state,
                None,
                false,
                None,
                |ui| {
                    ui.add_space(40.0); // clear the macOS traffic-light line
                    let mut view = DiffViewState::default();
                    let mut intents: Vec<GitIntent> = Vec::new();
                    let _ = diff_view(
                        ui,
                        &palette,
                        &file,
                        DiffSurface::WorkingTree { staged: false },
                        &mut view,
                        &mut intents,
                        None,
                    );
                },
            );
        });
    finish(harness, "git_staging");
}

/// One beat of the WIP-edit animation: the state its frame renders, and how long the GIF
/// holds it (centiseconds — the format's own unit).
struct EditBeat {
    /// A click in the content column has put the caret on the anchor line.
    caret: bool,
    /// Characters of `EDIT_LINE` typed so far, under the newline that opened the line;
    /// `None` before that newline.
    typed: Option<usize>,
    /// The editor has been left: the write landed and the diff recomposed around it.
    landed: bool,
    hold_cs: u32,
}

/// The line typed into the working tree from the diff itself.
const EDIT_LINE: &str = "        debug_assert!(!self.free.contains(&id));";

/// Context line the caret is put on — the click lands at its end, so the newline opens
/// the line below it.
const EDIT_ANCHOR: &str = "        self.lanes[id] = None;";

/// The diff on screen, a caret placed in it, a line typed under that caret, and the
/// write landing when the editor is left.
const EDIT_BEATS: &[EditBeat] = &[
    EditBeat {
        caret: false,
        typed: None,
        landed: false,
        hold_cs: 130,
    },
    EditBeat {
        caret: true,
        typed: None,
        landed: false,
        hold_cs: 95,
    },
    EditBeat {
        caret: true,
        typed: Some(0),
        landed: false,
        hold_cs: 25,
    },
    EditBeat {
        caret: true,
        typed: Some(14),
        landed: false,
        hold_cs: 16,
    },
    EditBeat {
        caret: true,
        typed: Some(26),
        landed: false,
        hold_cs: 16,
    },
    EditBeat {
        caret: true,
        typed: Some(38),
        landed: false,
        hold_cs: 16,
    },
    EditBeat {
        caret: true,
        typed: Some(EDIT_LINE.len()),
        landed: false,
        hold_cs: 120,
    },
    EditBeat {
        caret: false,
        typed: None,
        landed: true,
        hold_cs: 210,
    },
];

/// The new side of the file the WIP-edit GIF is composed on: the working tree the inline
/// editor writes back to (git.md §4). The diff below and this listing are one fixture —
/// `source_lines` is what the editor seeds its buffer from.
const LANE_FILE: &[&str] = &[
    "use git2::Oid;",
    "",
    "/// Lanes the commit graph draws its edges in.",
    "#[derive(Default)]",
    "pub struct LaneCache {",
    "    lanes: Vec<Option<Oid>>,",
    "    free: Vec<usize>,",
    "    next_id: usize,",
    "}",
    "",
    "impl LaneCache {",
    "    fn alloc(&mut self) -> usize {",
    "        if let Some(id) = self.free.pop() {",
    "            return id;",
    "        }",
    "        let id = self.next_id;",
    "        self.next_id += 1;",
    "        id",
    "    }",
    "",
    "    fn free(&mut self, id: usize) {",
    "        self.lanes[id] = None;",
    "        self.free.push(id);",
    "    }",
    "}",
];

/// The unstaged diff of `LANE_FILE`; `landed` is the same diff once the typed line
/// reached the working tree — the line is an addition like any other.
fn wip_edit_diff(landed: bool) -> FileDiff {
    let ctx = |s: &str, old: u32, new: u32| DiffLine {
        origin: LineOrigin::Context,
        content: s.to_owned(),
        old_lineno: Some(old),
        new_lineno: Some(new),
    };
    let add = |s: &str, new: u32| DiffLine {
        origin: LineOrigin::Addition,
        content: s.to_owned(),
        old_lineno: None,
        new_lineno: Some(new),
    };
    let mut source: Vec<String> = LANE_FILE.iter().map(|l| (*l).to_owned()).collect();
    let mut free_hunk = Hunk {
        header: "@@ -18,3 +21,4 @@ impl LaneCache {".to_owned(),
        old_start: 18,
        old_lines: 3,
        new_start: 21,
        new_lines: 4,
        lines: vec![
            ctx("    fn free(&mut self, id: usize) {", 18, 21),
            ctx(EDIT_ANCHOR, 19, 22),
            add("        self.free.push(id);", 23),
            ctx("    }", 20, 24),
        ],
    };
    if landed {
        source.insert(22, EDIT_LINE.to_owned());
        free_hunk.header = "@@ -18,3 +21,5 @@ impl LaneCache {".to_owned();
        free_hunk.new_lines = 5;
        free_hunk.lines = vec![
            ctx("    fn free(&mut self, id: usize) {", 18, 21),
            ctx(EDIT_ANCHOR, 19, 22),
            add(EDIT_LINE, 23),
            add("        self.free.push(id);", 24),
            ctx("    }", 20, 25),
        ];
    }
    FileDiff {
        path: "src/git/graph.rs".to_owned(),
        binary: false,
        oversize: false,
        editable: true,
        hunks: vec![
            Hunk {
                header: "@@ -12,5 +12,8 @@ impl LaneCache {".to_owned(),
                old_start: 12,
                old_lines: 5,
                new_start: 12,
                new_lines: 8,
                lines: vec![
                    ctx("    fn alloc(&mut self) -> usize {", 12, 12),
                    add("        if let Some(id) = self.free.pop() {", 13),
                    add("            return id;", 14),
                    add("        }", 15),
                    ctx("        let id = self.next_id;", 13, 16),
                    ctx("        self.next_id += 1;", 14, 17),
                    ctx("        id", 15, 18),
                    ctx("    }", 16, 19),
                ],
            },
            free_hunk,
        ],
        source_lines: source,
        image: None,
    }
}

/// The sidebar beside that diff: the edited file's `+N` counts the typed line once it
/// lands, so the write is visible outside the diff too.
fn wip_edit_status(landed: bool) -> RepoStatus {
    let entry = |path: &str, kind, additions, deletions| FileEntry {
        path: path.to_owned(),
        kind,
        additions,
        deletions,
    };
    RepoStatus {
        staged: vec![entry("src/ui/git_graph.rs", ChangeKind::Modified, 8, 2)],
        unstaged: vec![
            entry(
                "src/git/graph.rs",
                ChangeKind::Modified,
                if landed { 5 } else { 4 },
                0,
            ),
            entry("tests/graph_alloc.rs", ChangeKind::Added, 37, 0),
        ],
    }
}

fn char_width(harness: &mut Harness<'_, ()>) -> f32 {
    harness.ctx.fonts_mut(|fonts| {
        fonts
            .glyph_width(&egui::FontId::monospace(12.0), ' ')
            .max(1.0)
    })
}

/// A plain click — press and release without a drag — at `pos`: the gesture that puts
/// the caret in the content column (git.md §4).
fn click_at(harness: &mut Harness<'_, ()>, pos: egui::Pos2) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    harness.step();
}

/// The real diff view driven the way a hand would drive it: the caret is placed by a
/// click in the content column, the line is typed into the field the hunk's rows became,
/// and the last beat is the diff after the write — the whole shell, so the sidebar's
/// `+N` ticks up in the same frame.
fn render_wip_edit(beat: &EditBeat, out: &str) {
    let palette = Palette::dark();
    let file = wip_edit_diff(beat.landed);
    let aim = file.clone();
    let status = wip_edit_status(beat.landed);
    let view = Rc::new(RefCell::new(DiffViewState::default()));
    let view_in_ui = view.clone();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            // The caret is what this GIF is about: a blink would leave it out of half the
            // frames, and which half would depend on the step count.
            let mut style = (*ui.ctx().global_style()).clone();
            style.visuals.text_cursor.blink = false;
            ui.ctx().set_global_style(style);
            let keymap = Keymap::default();
            let mut git_state = GitPanelState {
                subject: "Reuse freed lanes in the git graph allocator".to_owned(),
                description: "Pop the free list before growing next_id so a closed pane \
                    returns its lane."
                    .to_owned(),
                ..Default::default()
            };
            app_shell(
                ui,
                &palette,
                &keymap,
                &status,
                &mut git_state,
                None,
                false,
                None,
                |ui| {
                    ui.add_space(40.0); // clear the macOS traffic-light line
                    let mut intents: Vec<GitIntent> = Vec::new();
                    let _ = diff_view(
                        ui,
                        &palette,
                        &file,
                        DiffSurface::WorkingTree { staged: false },
                        &mut view_in_ui.borrow_mut(),
                        &mut intents,
                        None,
                    );
                },
            );
        });
    harness.run_steps(3);

    // The accessibility rect is in physical pixels; the events below are in points, like
    // the offsets the row layout is measured in.
    let ppp = harness.ctx.pixels_per_point();
    let row = harness.get_by_label_contains(EDIT_ANCHOR).rect();
    let char_w = char_width(&mut harness);
    // Past the end of the anchor line — the caret clamps to the line's end, and aiming at
    // the exact last boundary would round to the character before it.
    let anchor = egui::pos2(
        row.left() / ppp
            + helm::ui::diff_view::content_x_offset(&aim, char_w)
            + (EDIT_ANCHOR.chars().count() + 4) as f32 * char_w,
        row.center().y / ppp,
    );
    if beat.caret {
        click_at(&mut harness, anchor);
        harness.run_steps(3);
    } else if beat.landed {
        // Where the click that left the editor took the pointer: the write lands on the
        // way out, so the last beat still carries a cursor — off the rows, or it would
        // hover one of them.
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(
                anchor.x - 60.0,
                row.bottom() / ppp + 90.0,
            )));
        harness.run_steps(3);
    }
    if let Some(chars) = beat.typed {
        harness.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        harness.run_steps(3);
        if chars > 0 {
            harness
                .input_mut()
                .events
                .push(egui::Event::Text(EDIT_LINE[..chars].to_owned()));
            harness.run_steps(3);
        }
    }
    assert_eq!(
        view.borrow().inline_edit().is_some(),
        beat.caret,
        "{out}: the editor's state must be the beat's, not what the click happened to do"
    );
    finish(harness, out);
}

#[test]
fn gen_wip_edit_frames() {
    let dir = "verify-artifacts/shots/wip-edit";
    std::fs::create_dir_all(dir).unwrap();
    let mut list = String::new();
    for (i, beat) in EDIT_BEATS.iter().enumerate() {
        render_wip_edit(beat, &format!("wip-edit/frame-{i:02}"));
        let secs = f64::from(beat.hold_cs) / 100.0;
        list.push_str(&format!("file 'frame-{i:02}.png'\nduration {secs:.2}\n"));
    }
    // The concat demuxer drops the last entry's duration unless the file repeats.
    let last = EDIT_BEATS.len() - 1;
    list.push_str(&format!("file 'frame-{last:02}.png'\n"));
    std::fs::write(format!("{dir}/frames.txt"), list).unwrap();
    println!("{} frames + frames.txt in {dir}", EDIT_BEATS.len());
}

#[test]
fn gen_pr_review_comments() {
    use helm::review::{FileComments, ForgeThreads, LineComment, ReviewIntent, ThreadComment};
    use helm::ui::diff_view::DiffReview;
    use std::cell::RefCell;
    use std::rc::Rc;

    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let intents = Rc::new(RefCell::new(Vec::<ReviewIntent>::new()));
    let state_ui = state.clone();
    let intents_ui = intents.clone();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(840.0, 720.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let palette = Palette::dark();
            let file = staging_diff();

            let mut agent_notes = FileComments::new();
            agent_notes.insert(
                file.path.clone(),
                vec![LineComment {
                    old_lineno: None,
                    new_lineno: Some(22),
                    code: "        let id = self.next_id;".to_owned(),
                    note: "Rename `id` to `lane` — it shadows the struct field.".to_owned(),
                }],
            );
            let mut forge_notes = FileComments::new();
            forge_notes.insert(
                file.path.clone(),
                vec![LineComment {
                    old_lineno: None,
                    new_lineno: Some(20),
                    code: "            return id;".to_owned(),
                    note: "Return the freed lane directly — the fall-through below is now unreachable."
                        .to_owned(),
                }],
            );
            let mut threads = ForgeThreads::new();
            threads.entry(file.path.clone()).or_default().insert(
                (None, Some(18)),
                vec![
                    ThreadComment {
                        author: "Dax Vega".to_owned(),
                        body: "Can we keep alloc() under ten lines? It's creeping up.".to_owned(),
                        id: Some(1),
                        created_at: String::new(),
                        context: None,
                        resolved: false,
                        thread_id: None,
                    },
                    ThreadComment {
                        author: "Mira Lund".to_owned(),
                        body: "Agreed — pull the grow path into its own helper.".to_owned(),
                        id: Some(2),
                        created_at: String::new(),
                        context: None,
                        resolved: false,
                        thread_id: None,
                    },
                ],
            );

            ui.painter()
                .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, palette.bg_canvas);
            egui::Frame::new()
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    let mut git: Vec<GitIntent> = Vec::new();
                    let mut ri = intents_ui.borrow_mut();
                    let _ = diff_view(
                        ui,
                        &palette,
                        &file,
                        DiffSurface::PrReview,
                        &mut state_ui.borrow_mut(),
                        &mut git,
                        Some(&mut DiffReview {
                            comments: &agent_notes,
                            forge: Some(&forge_notes),
                            existing: &threads,
                            agent: "claude",
                            intents: &mut ri,
                        }),
                    );
                });
        });
    harness.run();
    finish(harness, "pr_review_comments");
}

#[test]
fn gen_conflicts() {
    let palette = Palette::dark();
    // The Output pane memoises its laid-out galley on first paint. On a fresh
    // egui_kittest harness the fonts aren't rasterised for ppp 2.0 until a couple
    // of frames in, so a frame-0 layout caches degenerate glyph advances forever.
    // Hold the editor back until the fonts are warm, then let it seed the memo.
    let frame = std::cell::Cell::new(0u32);
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let warm = frame.get() >= 3;
            frame.set(frame.get() + 1);
            let keymap = Keymap::default();
            let op = OpSummary {
                verb: "Merging",
                source: Some("feature/login".to_owned()),
                target: Some("main".to_owned()),
            };
            let status = RepoStatus {
                staged: Vec::new(),
                unstaged: vec![FileEntry {
                    path: "src/auth/session.rs".to_owned(),
                    kind: ChangeKind::Conflicted,
                    additions: 0,
                    deletions: 0,
                }],
            };
            let mut git_state = GitPanelState::default();
            app_shell(
                ui,
                &palette,
                &keymap,
                &status,
                &mut git_state,
                Some(&op),
                false,
                None,
                |ui| {
                    ui.add_space(40.0);
                    if warm {
                        let mut state = ConflictEditorState::new(conflict_files());
                        let _ = conflict_view(ui, &palette, &mut state, false);
                    }
                },
            );
        });
    finish(harness, "conflicts");
}

#[test]
fn gen_worktrees() {
    let palette = Palette::dark();
    let term_pal = TermPalette::dark();
    let grid = hero_term();
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let keymap = Keymap::default();
            let status = hero_status();
            let mut git_state = GitPanelState::default();
            let layout = Layout::new();
            app_shell(
                ui,
                &palette,
                &keymap,
                &status,
                &mut git_state,
                None,
                false,
                None,
                |ui| {
                    central_switch(ui, &palette, false, &keymap, Some("helm"), None, true);
                    let mut rename = None;
                    let mut tab_action = TabBarAction::default();
                    tab_bar(
                        ui,
                        &palette,
                        &[String::from("Tab 1")],
                        0,
                        &mut rename,
                        &keymap,
                        &mut tab_action,
                    );
                    terminal_tree(ui, &layout, &palette, |ui, _id, focused| {
                        terminal_view(
                            ui,
                            &grid,
                            &term_pal,
                            DEFAULT_FONT_SIZE,
                            focused,
                            false,
                            None,
                            None,
                        );
                        false
                    });
                },
            );

            // The create-worktree modal overlays the dimmed window (egui::Modal).
            let sources = vec![
                WorktreeSource {
                    name: "feature/login".to_owned(),
                    kind: WorktreeSourceKind::Local,
                    local_branch: "feature/login".to_owned(),
                    path: PathBuf::from("~/dev/helm.worktrees/feature-login"),
                },
                WorktreeSource {
                    name: "fix/empty-repo-crash".to_owned(),
                    kind: WorktreeSourceKind::Local,
                    local_branch: "fix/empty-repo-crash".to_owned(),
                    path: PathBuf::from("~/dev/helm.worktrees/fix-empty-repo-crash"),
                },
                WorktreeSource {
                    name: "release/0.9".to_owned(),
                    kind: WorktreeSourceKind::Local,
                    local_branch: "release/0.9".to_owned(),
                    path: PathBuf::from("~/dev/helm.worktrees/release-0.9"),
                },
                WorktreeSource {
                    name: "origin/dependabot/bump-egui".to_owned(),
                    kind: WorktreeSourceKind::Remote,
                    local_branch: "dependabot/bump-egui".to_owned(),
                    path: PathBuf::from("~/dev/helm.worktrees/dependabot-bump-egui"),
                },
            ];
            let taken = HashSet::new();
            let root = Path::new("~/dev/helm");
            let prompt = CreateWorktreePrompt {
                root_label: "helm",
                root,
                base: None, // ⇒ default `<root>.worktrees`, shown verbatim
                sources: &sources,
                selected: Some(CreateSelection::Source(0)),
                base_branch: "main",
                taken: &taken,
                error: None,
                loading: false,
                busy: false,
            };
            let mut state = CreateWorktreeState {
                query: String::new(),
                focused: true, // already latched ⇒ no blinking text cursor
                name: "login".to_owned(),
                name_edited: true,
            };
            let mut action = CreateWorktreeModalAction::default();
            create_worktree_modal(ui, &palette, &prompt, &mut state, &mut action);
        });
    finish(harness, "worktrees");
}

#[test]
fn gen_ai_rebase() {
    let palette = Palette::dark();
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let keymap = Keymap::default();
            let status = hero_status();
            let mut git_state = GitPanelState::default();
            let graph = demo_graph();
            let toolbar = demo_toolbar();
            let detail = commit_detail_login();
            app_shell(
                ui,
                &palette,
                &keymap,
                &status,
                &mut git_state,
                None,
                true,
                Some(&detail),
                |ui| {
                    central_switch(ui, &palette, true, &keymap, Some("helm"), None, true);
                    let mut editor = BranchEditor::default();
                    let mut lanes = LaneCache::default();
                    let mut search = GraphSearch::default();
                    graph_toolbar(ui, &palette, &toolbar, &mut editor);
                    let _ = graph_view(
                        ui,
                        &palette,
                        &GraphViewState {
                            graph: Some(&graph),
                            wip: None,
                            selected: Some(oid(5)),
                            scroll_to_head: false,
                            keyboard_nav: false,
                            can_pull_request: false,
                        },
                        &mut lanes,
                        &mut editor,
                        &mut search,
                    );
                },
            );

            let mut page = AiRebasePage {
                current: "feature/login".to_owned(),
                onto: "main".to_owned(),
                loading: false,
                error: None,
                commits: vec![
                    RebaseCommit {
                        oid: oid(5),
                        short_id: "a4f9e21".to_owned(),
                        summary: "Add the login form + inline validation".to_owned(),
                        message: String::new(),
                        author: "David Bonan".to_owned(),
                    },
                    RebaseCommit {
                        oid: oid(7),
                        short_id: "c08b3da".to_owned(),
                        summary: "Scaffold the auth module".to_owned(),
                        message: String::new(),
                        author: "David Bonan".to_owned(),
                    },
                    RebaseCommit {
                        oid: oid(8),
                        short_id: "f1029ab".to_owned(),
                        summary: "Wire the session store".to_owned(),
                        message: String::new(),
                        author: "David Bonan".to_owned(),
                    },
                ],
                instructions: "Squash everything into a single commit.".to_owned(),
            };
            let _ = ai_rebase_modal(ui, &palette, &mut page, AiProvider::Claude, false);
        });
    finish(harness, "ai_rebase");
}

#[test]
fn gen_agents() {
    let palette = Palette::dark();
    let harness = Harness::builder()
        .with_size(egui::vec2(300.0, 360.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let keymap = Keymap::default();
            let items = demo_items();
            let projects = demo_projects();
            let mut sidebar = SidebarAction::default();
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(palette.bg_sidebar)
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 10,
                            top: 40,
                            bottom: 10,
                        }),
                )
                .show_inside(ui, |ui| {
                    repo_sidebar(
                        ui,
                        &palette,
                        &items,
                        &CHILD_FLAGS,
                        &projects,
                        Some(0),
                        AgentBadge::None,
                        false,
                        &[],
                        0,
                        false,
                        &keymap,
                        &mut sidebar,
                    );
                });
        });
    finish(harness, "agents");
}

#[test]
fn gen_preferences() {
    let palette = Palette::dark();
    let harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 470.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let mut section = PreferencesSection::Appearance;
            let mut mode = helm::theme::ThemeMode::Dark;
            let mut light = String::from("helm");
            let mut dark = String::from("helm");
            let mut pull = PullDefault::default();
            let mut ai = AiProvider::Claude;
            let mut ai_instr = String::new();
            let mut ai_rebase = AiProvider::Claude;
            let mut editor = Editor::default();
            let mut notify = true;
            let mut keymap = Keymap::default();
            let mut keyboard = KeyboardState::default();
            let updates = UpdatesView {
                version: "0.8.4".to_owned(),
                state: UpdateState::default(),
                bundled: true,
            };
            let mut release_notes_cache = egui_commonmark::CommonMarkCache::default();
            let mut bitbucket_email = String::new();
            let mut bitbucket_token = String::new();
            let pr_sources = PrSourcesView {
                github: SourceStatus::Absent,
                bitbucket: SourceStatus::Absent,
                loaded: false,
            };
            let _ = preferences_page(
                ui,
                &palette,
                &mut section,
                &mut mode,
                &mut light,
                &mut dark,
                &mut pull,
                &mut ai,
                &mut ai_instr,
                &mut ai_rebase,
                &mut String::from("claude"),
                &mut editor,
                &mut bitbucket_email,
                &mut bitbucket_token,
                &pr_sources,
                &mut notify,
                &mut keymap,
                &mut keyboard,
                &updates,
                &helm::cli::ShellCommand::Unbundled,
                &mut release_notes_cache,
                None,
            );
        });
    finish(harness, "preferences");
}

#[test]
fn gen_pr_list() {
    use helm::pull_requests::model::{
        Checks, ForgeKind, PrRole, PrState, PullRequest, Review, Reviewer,
    };
    use helm::ui::file_list::FileViewMode;
    use helm::ui::pull_requests_view::{pull_requests_page, PrSourceHints};

    let reviewer = |name: &str, state| Reviewer {
        name: name.to_owned(),
        state,
    };
    let mk = |repo: &str,
              number: u64,
              title: &str,
              author: &str,
              src: &str,
              role,
              state,
              checks,
              review,
              reviewers: Vec<Reviewer>| PullRequest {
        forge_kind: ForgeKind::GitHub,
        repo_label: repo.to_owned(),
        number,
        title: title.to_owned(),
        role,
        state,
        author: author.to_owned(),
        source_branch: src.to_owned(),
        dest_branch: "main".to_owned(),
        url: format!("https://example.test/{repo}/pull/{number}"),
        updated_at: "2026-06-20T10:00:00Z".to_owned(),
        checks,
        review,
        reviewers,
        labels: Vec::new(),
        diffstat: Some((number as u32 * 3, number as u32)),
        comment_count: Some((number % 7) as u32),
    };
    // A chain of PRs each targeting the one below it, so the shot carries the stack
    // block: its header, the numbered spine and the "Review first" flag on the base.
    let stacked = |number: u64, title: &str, src: &str, dest: &str| PullRequest {
        dest_branch: dest.to_owned(),
        ..mk(
            "acme/web",
            number,
            title,
            "mira",
            src,
            PrRole::ToReview,
            PrState::Open,
            Checks::Passing,
            Review::Pending,
            vec![reviewer("octocat", Review::Pending)],
        )
    };

    let prs = vec![
        stacked(
            140,
            "counter catalogue spike",
            "feat/ACME-701-counter-catalogue",
            "main",
        ),
        stacked(
            141,
            "setup the periodic batch and read CLIENTMAIL",
            "feat/ACME-702-setup-batch",
            "feat/ACME-701-counter-catalogue",
        ),
        stacked(
            142,
            "registry, dispatcher and the ENVOIMAIL log line",
            "feat/ACME-703-dispatcher",
            "feat/ACME-702-setup-batch",
        ),
        stacked(
            143,
            "daily lifecycle orchestrator",
            "feat/ACME-704-daily-orchestrator",
            "feat/ACME-703-dispatcher",
        ),
        mk(
            "acme/web",
            128,
            "Fix the login redirect loop on expired sessions",
            "mira",
            "fix/login-loop",
            PrRole::ToReview,
            PrState::Open,
            Checks::Passing,
            Review::Pending,
            vec![reviewer("octocat", Review::Pending)],
        ),
        mk(
            "acme/api",
            96,
            "Bump the cache TTL to 15 minutes",
            "dax",
            "perf/cache-ttl",
            PrRole::ToReview,
            PrState::Open,
            Checks::Failing,
            Review::ChangesRequested,
            vec![reviewer("octocat", Review::ChangesRequested)],
        ),
        mk(
            "acme/web",
            131,
            "Draft: extract the session store",
            "lena",
            "refactor/session-store",
            PrRole::Mine,
            PrState::Draft,
            Checks::Pending,
            Review::None,
            vec![
                reviewer("mira", Review::Approved),
                reviewer("dax", Review::Pending),
                reviewer("kai", Review::ChangesRequested),
                reviewer("ren", Review::Pending),
            ],
        ),
        mk(
            "acme/cli",
            54,
            "Add the --json flag to status",
            "lena",
            "feat/json-status",
            PrRole::Mine,
            PrState::Open,
            Checks::Passing,
            Review::Approved,
            vec![reviewer("mira", Review::Approved)],
        ),
    ];

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 720.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let palette = Palette::dark();
            ui.painter()
                .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, palette.bg_canvas);
            let _ = pull_requests_page(
                ui,
                &palette,
                &prs,
                None,
                &PrSourceHints {
                    // Two minutes back, so the header carries its age note; the label
                    // is a function of the delta, so the shot stays reproducible.
                    refreshed_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64 - 120)
                        .ok(),
                    ..Default::default()
                },
                None,
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.run();
    finish(harness, "pr_list");
}

/// The review surface's **Files** tab: the toolbar over the continuous column of
/// changed-file bands, with the open file's diff expanded inline.
#[test]
fn gen_pr_files() {
    use helm::git::commit_detail::CommitFile;
    use helm::git::diff::{DiffLine, FileDiff, Hunk, LineOrigin};
    use helm::git::status::ChangeKind;
    use helm::pull_requests::model::{
        Checks, ForgeKind, PrDetail, PrRole, PrState, PullRequest, Review, ReviewVerdict, Reviewer,
    };
    use helm::review::{FileComments, ForgeThreads};
    use helm::ui::diff_view::DiffViewState;
    use helm::ui::file_list::FileViewMode;
    use helm::ui::pull_requests_view::{pull_requests_page, PrReviewView, PrSourceHints};

    let pr = PullRequest {
        forge_kind: ForgeKind::GitHub,
        repo_label: "acme/web".to_owned(),
        number: 1284,
        title: "Dedupe webhook deliveries during retry storms".to_owned(),
        role: PrRole::ToReview,
        state: PrState::Open,
        author: "Thomas Lenoir".to_owned(),
        source_branch: "feat/webhook-dedupe".to_owned(),
        dest_branch: "develop".to_owned(),
        url: "https://example.test/acme/web/pull/1284".to_owned(),
        updated_at: "2026-06-20T10:00:00Z".to_owned(),
        checks: Checks::Passing,
        review: Review::Pending,
        reviewers: vec![Reviewer {
            name: "Camille Rey".to_owned(),
            state: Review::Pending,
        }],
        labels: Vec::new(),
        diffstat: Some((142, 38)),
        comment_count: None,
    };
    let detail = PrDetail {
        created_at: "2026-06-17T09:00:00Z".to_owned(),
        ..PrDetail::default()
    };
    let file = |path: &str, additions: usize, deletions: usize| CommitFile {
        path: path.to_owned(),
        kind: ChangeKind::Modified,
        additions,
        deletions,
    };
    let files = vec![
        file("src/webhooks/delivery/DeliveryQueueStore.ts", 28, 6),
        file("src/webhooks/delivery/BackoffPolicy.ts", 18, 2),
        file("src/webhooks/transport/WebhookTransport.ts", 9, 12),
        file("tests/delivery_queue_store.spec.ts", 41, 0),
    ];
    let line = |origin, content: &str, old, new| DiffLine {
        origin,
        content: content.to_owned(),
        old_lineno: old,
        new_lineno: new,
    };
    let diff = FileDiff {
        path: "src/webhooks/delivery/DeliveryQueueStore.ts".to_owned(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -118,7 +118,13 @@ class DeliveryQueueStore".to_owned(),
            old_start: 118,
            old_lines: 7,
            new_start: 118,
            new_lines: 13,
            lines: vec![
                line(
                    LineOrigin::Context,
                    "  enqueue(delivery: QueuedDelivery) {\n",
                    Some(120),
                    Some(120),
                ),
                line(
                    LineOrigin::Deletion,
                    "    this.pending.set(delivery.id, delivery);\n",
                    Some(121),
                    None,
                ),
                line(
                    LineOrigin::Addition,
                    "    const existing = this.pending.get(delivery.id);\n",
                    None,
                    Some(121),
                ),
                line(
                    LineOrigin::Addition,
                    "    if (existing && existing.attempt >= delivery.attempt) return;\n",
                    None,
                    Some(122),
                ),
                line(LineOrigin::Context, "  }\n", Some(122), Some(124)),
            ],
        }],
        source_lines: Vec::new(),
        image: None,
        editable: false,
    };
    // The column diffs every file, so the shot carries a second band under the first.
    let backoff = FileDiff {
        path: "src/webhooks/delivery/BackoffPolicy.ts".to_owned(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -12,5 +12,8 @@ export class BackoffPolicy".to_owned(),
            old_start: 12,
            old_lines: 5,
            new_start: 12,
            new_lines: 8,
            lines: vec![
                line(
                    LineOrigin::Context,
                    "  next(attempt: number): number {\n",
                    Some(12),
                    Some(12),
                ),
                line(
                    LineOrigin::Deletion,
                    "    return 2 ** attempt * 1000;\n",
                    Some(13),
                    None,
                ),
                line(
                    LineOrigin::Addition,
                    "    const capped = Math.min(attempt, MAX_ATTEMPT);\n",
                    None,
                    Some(13),
                ),
                line(
                    LineOrigin::Addition,
                    "    return 2 ** capped * 1000 + jitter();\n",
                    None,
                    Some(14),
                ),
                line(LineOrigin::Context, "  }\n", Some(14), Some(15)),
            ],
        }],
        source_lines: Vec::new(),
        image: None,
        editable: false,
    };

    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let palette = Palette::dark();
            ui.painter()
                .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, palette.bg_canvas);
            let mut review = PrReviewView {
                pr: &pr,
                detail: Some(&detail),
                detail_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(0),
                commits: &[],
                selected_commit: None,
                diffs: vec![Some(&diff), Some(&backoff), None, None],
                diff_errors: vec![None, None, None, None],
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: Some("Sam Rivers"),
            };
            let _ = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                300.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.run();
    finish(harness, "pr_files");
}

#[test]
fn gen_pr_detail() {
    use helm::pull_requests::model::{
        Checks, ForgeKind, PrComment, PrDetail, PrRole, PrState, PullRequest, Review,
        ReviewVerdict, Reviewer,
    };
    use helm::review::{FileComments, ForgeThreads};
    use helm::ui::diff_view::DiffViewState;
    use helm::ui::file_list::FileViewMode;
    use helm::ui::pull_requests_view::{pull_requests_page, PrReviewView, PrSourceHints};

    let comment = |author: &str, body: &str| PrComment {
        author: author.to_owned(),
        body: body.to_owned(),
        path: None,
        old_lineno: None,
        new_lineno: None,
        id: None,
        parent_id: None,
        context: None,
        created_at: String::new(),
        resolved: false,
        thread_id: None,
    };
    let pr = PullRequest {
        forge_kind: ForgeKind::GitHub,
        repo_label: "acme/web".to_owned(),
        number: 128,
        title: "Fix the login redirect loop on expired sessions".to_owned(),
        role: PrRole::ToReview,
        state: PrState::Open,
        author: "mira".to_owned(),
        source_branch: "fix/login-loop".to_owned(),
        dest_branch: "main".to_owned(),
        url: "https://example.test/acme/web/pull/128".to_owned(),
        updated_at: "2026-06-24T08:00:00Z".to_owned(),
        checks: Checks::Passing,
        review: Review::Pending,
        reviewers: vec![
            Reviewer {
                name: "octocat".to_owned(),
                state: Review::Approved,
            },
            Reviewer {
                name: "dax".to_owned(),
                state: Review::Pending,
            },
        ],
        labels: vec!["bug".to_owned(), "auth".to_owned(), "priority".to_owned()],
        diffstat: Some((142, 38)),
        comment_count: Some(4),
    };
    let detail = PrDetail {
        body: "## Problem\n\nExpired sessions sent users into a **redirect loop**: the \
               auth guard bounced `/login` back through the stale token.\n\n\
               ## Fixes\n\n- Reset the session cookie when the token is stale\n\
               - Short-circuit `AuthGuard::check` instead of redirecting\n\n\
               Covered by a regression test on the guard."
            .to_owned(),
        comments: vec![
            comment(
                "mira",
                "Splitting the **guard fix** from the cookie reset for review.",
            ),
            comment(
                "octocat",
                "Guard change looks right. One nit on the `cookie_name` constant.",
            ),
            comment("dax", "Can we add a metric on the loop-break path?"),
        ],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: "2026-06-23T09:30:00Z".to_owned(),
    };
    let files: &[helm::git::commit_detail::CommitFile] = &[];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 680.0))
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            boot(ui);
            let palette = Palette::dark();
            ui.painter()
                .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, palette.bg_canvas);
            let mut review = PrReviewView {
                pr: &pr,
                detail: Some(&detail),
                detail_loading: false,
                detail_error: None,
                files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: Some("Sam Rivers"),
            };
            let _ = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                380.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.run();
    finish(harness, "pr_detail");
}
