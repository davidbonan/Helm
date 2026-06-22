//! Screenshot generator for the README (run on demand, not part of the gate):
//!   cargo test --features headless-verify --test shots_gen -- --nocapture
//! Renders the cross-repo agents dashboard (List + Columns) with authentic
//! terminal content fed through the real emulator, and saves PNGs under
//! verify-artifacts/shots/. Deterministic — no PTY, no timing.
#![cfg(feature = "headless-verify")]

use egui_kittest::Harness;

use helm::agent_watch::AgentBadge;
use helm::ai::AiProvider;
use helm::git::commit_detail::{CommitDetail, CommitFile, CommitMeta};
use helm::git::conflict::{ConflictFile, ConflictKind, Region};
use helm::git::diff::{DiffLine, FileDiff, Hunk, LineOrigin};
use helm::git::graph::{Graph, GraphCommit, GraphRef, LaneCache, RefKind};
use helm::git::rebase::RebaseCommit;
use helm::git::status::{ChangeKind, FileEntry, OpSummary, RepoStatus};
use helm::git::sync::PullDefault;
use helm::git::worktree::{WorktreeSource, WorktreeSourceKind};
use helm::keybindings::Keymap;
use helm::terminal::emu::{self, SharedTerm, DEFAULT_FONT_SIZE};
use helm::terminal::layout::{Layout, Orient, PaneId};
use helm::terminal::links::Editor;
use helm::terminal::palette::TermPalette;
use helm::theme::Palette;
use helm::ui::agents_view::{agents_page, AgentRow, AgentsViewMode, TermView, AGENT_PREVIEW_LINES};
use helm::ui::ai_rebase_modal::{ai_rebase_modal, AiRebasePage};
use helm::ui::conflict_view::{conflict_view, ConflictEditorState};
use helm::ui::diff_view::{diff_view, DiffViewState};
use helm::ui::file_list::FileMenuOutput;
use helm::ui::git_panel::{GitIntent, GitPanelState};
use helm::ui::graph_toolbar::{graph_toolbar, ToolbarState};
use helm::ui::graph_view::{graph_view, BranchEditor, GraphSearch, GraphViewState};
use helm::ui::preferences::{preferences_page, KeyboardState, PreferencesSection, UpdatesView};
use helm::ui::repo_sidebar::{
    create_worktree_modal, repo_sidebar, CreateSelection, CreateWorktreeModalAction,
    CreateWorktreePrompt, CreateWorktreeState, ProjectHeader, ProjectVisibility, RepoRow,
    SidebarAction, SidebarItem,
};
use helm::ui::tab_bar::{tab_bar, TabBarAction};
use helm::ui::terminal_view::{terminal_tree, terminal_view, terminal_view_preview};
use helm::ui::{central_switch, root_layout};
use helm::update::UpdateState;
use helm::workspace_launcher::WorkspaceOpener;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

struct Spec {
    repo: &'static str,
    branch: &'static str,
    tab: &'static str,
    agent: &'static str,
    badge: AgentBadge,
    detail: &'static str,
    worktree_id: usize,
    stats: Option<(usize, usize)>,
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
            worktree_id: 0,
            stats: Some((128, 34)),
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
        Spec {
            repo: "helm",
            branch: "agents-dashboard",
            tab: "Tab 1",
            agent: "codex",
            badge: AgentBadge::Done,
            detail: "Finished 3m ago",
            worktree_id: 1,
            stats: Some((57, 9)),
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
            worktree_id: 0,
            stats: None,
            // A full-screen TUI at the pane's real 110-col width, ending in Claude
            // Code's bottom chrome block — a multi-row boxed composer plus mode /
            // hint / status lines under it: the condensed preview must detect and
            // drop that whole block and show only the conversation above it.
            body: &[
                "\x1b[2mapi  ~/dev/api  main\x1b[0m",
                "\x1b[38;5;208mClaude Code\x1b[0m  \x1b[2mv2.1.162 · opus\x1b[0m",
                "",
                "\x1b[1m>\x1b[0m run the full test suite, fix failures, then update the changelog and open a PR",
                "",
                "\x1b[32m⏺\x1b[0m Bash \x1b[2mcargo test --workspace\x1b[0m",
                "  \x1b[2mtest tests::health::returns_200 … ok\x1b[0m",
                "  \x1b[31mtest tests::billing::prorates_midcycle … FAILED — expected 4200, got 5000\x1b[0m",
                "\x1b[32m⏺\x1b[0m Update \x1b[36msrc/billing/proration.rs\x1b[0m \x1b[2m(+18 −7) — clamp the partial-period ratio to [0,1]\x1b[0m",
                "",
                "╭────────────────────────────────────────────────────────────────────────────────────────────────╮",
                "│ > _                                                                                            │",
                "│                                                                                                │",
                "╰────────────────────────────────────────────────────────────────────────────────────────────────╯",
                "  \x1b[2m⏵⏵ accept edits on (shift+tab to cycle)\x1b[0m",
                "  \x1b[2m? for shortcuts\x1b[0m",
                "  \x1b[36m✻ Crunching…\x1b[0m \x1b[2m(esc to interrupt · 1m24s · ↓ 6.2k tokens)\x1b[0m",
            ],
            cursor_row: Some(11),
        },
    ]
}

fn render(view: AgentsViewMode, selected: Option<usize>, size: egui::Vec2, out: &str) {
    let palette = Palette::dark();
    let term_pal = TermPalette::dark();
    let data = specs();
    let grids: Vec<SharedTerm> = data
        .iter()
        .map(|s| {
            let t = term(22, 110, s.body);
            if let Some(row) = s.cursor_row {
                emu::feed(&t, format!("\x1b[{};1H", row + 1).as_bytes());
            }
            t
        })
        .collect();

    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(2.0)
        .build_ui(move |ui| {
            helm::theme::install_fonts(ui.ctx());
            ui.ctx().set_visuals(egui::Visuals::dark());
            let rows: Vec<AgentRow> = data
                .iter()
                .map(|s| AgentRow {
                    repo: s.repo,
                    branch: Some(s.branch),
                    tab: s.tab,
                    agent: s.agent,
                    badge: s.badge,
                    detail: s.detail.to_owned(),
                    worktree_id: s.worktree_id,
                    lane: 0,
                    stats: s.stats,
                })
                .collect();
            agents_page(
                ui,
                &palette,
                &rows,
                selected,
                view,
                620.0,
                380.0,
                |idx, tui, view| match view {
                    TermView::Full => {
                        terminal_view(
                            tui,
                            &grids[idx],
                            &term_pal,
                            DEFAULT_FONT_SIZE,
                            selected == Some(idx),
                            false,
                            None,
                            None,
                        );
                    }
                    TermView::Preview => {
                        terminal_view_preview(
                            tui,
                            &grids[idx],
                            &term_pal,
                            DEFAULT_FONT_SIZE,
                            AGENT_PREVIEW_LINES,
                        );
                    }
                },
            );
        });

    // A Working row repaints forever (spinner) — settle a fixed number of frames.
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
fn gen_agents_list() {
    render(
        AgentsViewMode::List,
        Some(0),
        egui::vec2(1280.0, 800.0),
        "agents_list",
    );
}

#[test]
fn gen_agents_columns() {
    render(
        AgentsViewMode::Columns,
        Some(0),
        egui::vec2(1680.0, 900.0),
        "agents_columns",
    );
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
                    let _ = diff_view(ui, &palette, &file, false, false, &mut view, &mut intents);
                },
            );
        });
    finish(harness, "git_staging");
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
                &mut editor,
                &mut notify,
                &mut keymap,
                &mut keyboard,
                &updates,
                &mut release_notes_cache,
                None,
            );
        });
    finish(harness, "preferences");
}
