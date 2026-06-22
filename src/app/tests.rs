use super::git_session::PaneKey;
use super::keys::{
    git_command, layout_command, open_agents_command, positional_key, select_repo_command,
    select_tab_command, tab_action, zoom_command, FocusZone, LayoutCommand, TabAction, ZoomCommand,
};
use super::*;
use crate::persistence::Project;

fn cmd(extra: egui::Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        mac_cmd: true,
        ..extra
    }
}

fn wheel(dy: f32, phase: egui::TouchPhase) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        modifiers: egui::Modifiers::default(),
        phase,
    }
}

fn wheel_deltas(events: &[egui::Event]) -> Vec<(f32, egui::TouchPhase)> {
    events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel { delta, phase, .. } => Some((delta.y, *phase)),
            _ => None,
        })
        .collect()
}

#[test]
fn wheel_start_keeps_its_delta_as_a_following_move() {
    let mut events = vec![wheel(-9.0, egui::TouchPhase::Start)];
    let mut deferred = None;
    rewrite_wheel_phases(&mut events, &mut deferred);
    assert_eq!(
        wheel_deltas(&events),
        [
            (0.0, egui::TouchPhase::Start),
            (-9.0, egui::TouchPhase::Move)
        ],
        "egui drops a Start event's delta: it must arrive as a Move"
    );
    assert!(deferred.is_none());
}

#[test]
fn wheel_end_is_deferred_to_the_next_frames_front() {
    let mut events = vec![
        wheel(-5.0, egui::TouchPhase::Move),
        wheel(0.0, egui::TouchPhase::End),
    ];
    let mut deferred = None;
    rewrite_wheel_phases(&mut events, &mut deferred);
    // This frame keeps its motion; the End (which would wipe it) is withheld.
    assert_eq!(wheel_deltas(&events), [(-5.0, egui::TouchPhase::Move)]);
    assert!(deferred.is_some());

    let mut next = vec![egui::Event::PointerGone];
    rewrite_wheel_phases(&mut next, &mut deferred);
    assert_eq!(wheel_deltas(&next), [(0.0, egui::TouchPhase::End)]);
    assert_eq!(next[1], egui::Event::PointerGone, "End replayed first");
    assert!(deferred.is_none(), "replayed once");
}

#[test]
fn wheel_start_in_the_same_frame_cancels_the_withheld_end() {
    // Touch gesture ends and the momentum stream begins within one frame:
    // the reset must not fire mid-gesture on the next frame.
    let mut events = vec![
        wheel(0.0, egui::TouchPhase::End),
        wheel(-12.0, egui::TouchPhase::Start),
    ];
    let mut deferred = None;
    rewrite_wheel_phases(&mut events, &mut deferred);
    assert_eq!(
        wheel_deltas(&events),
        [
            (0.0, egui::TouchPhase::Start),
            (-12.0, egui::TouchPhase::Move)
        ]
    );
    assert!(deferred.is_none());
}

#[test]
fn wheel_moves_and_other_events_pass_through_untouched() {
    let mut events = vec![
        egui::Event::PointerGone,
        wheel(3.0, egui::TouchPhase::Move),
        wheel(4.0, egui::TouchPhase::Move),
    ];
    let mut deferred = None;
    rewrite_wheel_phases(&mut events, &mut deferred);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], egui::Event::PointerGone);
    assert_eq!(
        wheel_deltas(&events),
        [(3.0, egui::TouchPhase::Move), (4.0, egui::TouchPhase::Move)],
        "mouse-notch scrolling (no phases) keeps egui's smoothing path"
    );
}

#[test]
fn post_create_payload_runs_through_bash_not_the_interactive_shell() {
    // A real bash script (shebang + bash-isms) must not be typed line by
    // line into zsh, where `#!` triggers history expansion (worktrees.md §6).
    assert_eq!(
            post_create_payload("#!/usr/bin/env bash\nset -euo pipefail\necho hi"),
            "bash -s <<'HELM_POST_CREATE_EOF'\n#!/usr/bin/env bash\nset -euo pipefail\necho hi\nHELM_POST_CREATE_EOF\n"
        );
    // A trailing newline in the stored script must not push a blank line
    // before the closing delimiter.
    assert_eq!(
        post_create_payload("echo hi\n"),
        "bash -s <<'HELM_POST_CREATE_EOF'\necho hi\nHELM_POST_CREATE_EOF\n"
    );
}

#[test]
fn command_failures_name_the_action_before_the_git_message() {
    let err = git2::Error::from_str("nothing to stash");
    assert_eq!(
        command_failure_message(&GitCommand::Stash, &err),
        "Stash failed — nothing to stash"
    );

    let err = git2::Error::from_str("conflicts while applying — the stash was kept");
    assert_eq!(
        command_failure_message(&GitCommand::StashPop, &err),
        "Stash pop failed — conflicts while applying — the stash was kept"
    );

    let err = git2::Error::from_str("1 conflict prevents checkout");
    assert_eq!(
        command_failure_message(&GitCommand::Checkout("feat".into()), &err),
        "Checkout of 'feat' failed — 1 conflict prevents checkout"
    );

    let err = git2::Error::from_str("a branch named 'feat' already exists");
    assert_eq!(
        command_failure_message(&GitCommand::CreateBranch("feat".into()), &err),
        "Creating branch 'feat' failed — a branch named 'feat' already exists"
    );

    let err = git2::Error::from_str("could not write index");
    assert_eq!(
        command_failure_message(&GitCommand::Commit("msg".into()), &err),
        "Commit failed — could not write index"
    );
    assert_eq!(
        command_failure_message(&GitCommand::Discard("a.txt".into()), &err),
        "Discard failed — could not write index"
    );
    assert_eq!(
        command_failure_message(&GitCommand::StageAll, &err),
        "Stage failed — could not write index"
    );
    assert_eq!(
        command_failure_message(
            &GitCommand::UnstageHunk {
                path: "a.txt".into(),
                hunk: 0
            },
            &err
        ),
        "Unstage failed — could not write index"
    );
    assert_eq!(
        command_failure_message(
            &GitCommand::DiscardHunk {
                path: "a.txt".into(),
                hunk: 0
            },
            &err
        ),
        "Discard failed — could not write index"
    );
    assert_eq!(
        command_failure_message(&GitCommand::Status, &err),
        "Git status failed — could not write index"
    );
}

#[test]
fn diff_overlay_zone_blocks_terminal_shortcuts() {
    // Diff view open ⇒ DiffView zone: §2 (split/zoom) inactive, even if a pane kept
    // egui focus (keybindings §4).
    let zone = focus_zone(true, true);
    assert_eq!(zone, FocusZone::DiffView);
    assert!(
        !zone.terminal_shortcuts_active(),
        "terminal shortcuts must be inert while the diff overlay is open"
    );
}

#[test]
fn terminal_focus_enables_terminal_shortcuts() {
    let zone = focus_zone(false, true);
    assert_eq!(zone, FocusZone::Terminal);
    assert!(zone.terminal_shortcuts_active());
}

#[test]
fn no_terminal_focus_routes_to_other_zone() {
    // Sidebar / commit field / no focus ⇒ Other: §2 inactive.
    let zone = focus_zone(false, false);
    assert_eq!(zone, FocusZone::Other);
    assert!(!zone.terminal_shortcuts_active());
}

#[test]
fn cmd_d_splits_vertical_shift_d_splits_horizontal() {
    let keymap = Keymap::default();
    assert_eq!(
        layout_command(&keymap, egui::Key::D, cmd(egui::Modifiers::default())),
        Some(LayoutCommand::Split(Orient::Vertical))
    );
    assert_eq!(
        layout_command(
            &keymap,
            egui::Key::D,
            cmd(egui::Modifiers {
                shift: true,
                ..Default::default()
            })
        ),
        Some(LayoutCommand::Split(Orient::Horizontal))
    );
}

#[test]
fn cmd_w_closes() {
    let keymap = Keymap::default();
    assert_eq!(
        layout_command(&keymap, egui::Key::W, cmd(egui::Modifiers::default())),
        Some(LayoutCommand::Close)
    );
}

#[test]
fn cmd_alt_arrows_move_focus() {
    let keymap = Keymap::default();
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        layout_command(&keymap, egui::Key::ArrowLeft, cmd(alt)),
        Some(LayoutCommand::Focus(Dir::Left))
    );
    assert_eq!(
        layout_command(&keymap, egui::Key::ArrowRight, cmd(alt)),
        Some(LayoutCommand::Focus(Dir::Right))
    );
    assert_eq!(
        layout_command(&keymap, egui::Key::ArrowUp, cmd(alt)),
        Some(LayoutCommand::Focus(Dir::Up))
    );
    assert_eq!(
        layout_command(&keymap, egui::Key::ArrowDown, cmd(alt)),
        Some(LayoutCommand::Focus(Dir::Down))
    );
}

#[test]
fn cmd_ctrl_arrows_resize() {
    let keymap = Keymap::default();
    let ctrl = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    assert_eq!(
        layout_command(&keymap, egui::Key::ArrowLeft, cmd(ctrl)),
        Some(LayoutCommand::Resize(Dir::Left))
    );
    assert_eq!(
        layout_command(&keymap, egui::Key::ArrowDown, cmd(ctrl)),
        Some(LayoutCommand::Resize(Dir::Down))
    );
}

#[test]
fn without_command_modifier_nothing_routes() {
    let keymap = Keymap::default();
    assert_eq!(
        layout_command(&keymap, egui::Key::D, egui::Modifiers::default()),
        None
    );
    assert_eq!(
        layout_command(
            &keymap,
            egui::Key::ArrowLeft,
            egui::Modifiers {
                alt: true,
                ..Default::default()
            }
        ),
        None
    );
}

#[test]
fn rebound_combo_routes_and_its_default_goes_dead() {
    let mut keymap = Keymap::default();
    keymap.set(Action::SplitRight, Some(Shortcut::cmd_shift(egui::Key::X)));
    assert_eq!(
        layout_command(
            &keymap,
            egui::Key::X,
            cmd(egui::Modifiers {
                shift: true,
                ..Default::default()
            })
        ),
        Some(LayoutCommand::Split(Orient::Vertical))
    );
    assert_eq!(
        layout_command(&keymap, egui::Key::D, cmd(egui::Modifiers::default())),
        None,
        "the default Cmd+D goes dead once Split right is rebound"
    );
}

#[test]
fn unbound_action_is_inert() {
    let mut keymap = Keymap::default();
    keymap.set(Action::SplitRight, None);
    assert_eq!(
        layout_command(&keymap, egui::Key::D, cmd(egui::Modifiers::default())),
        None
    );
    assert_eq!(
        layout_command(&keymap, egui::Key::W, cmd(egui::Modifiers::default())),
        Some(LayoutCommand::Close),
        "unbinding one action leaves the others routed"
    );
}

#[test]
fn cmd_ctrl_digits_select_repos_one_through_nine() {
    let cmd_ctrl = cmd(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    assert_eq!(select_repo_command(egui::Key::Num1, cmd_ctrl), Some(0));
    assert_eq!(select_repo_command(egui::Key::Num9, cmd_ctrl), Some(8));
    assert_eq!(
        select_repo_command(egui::Key::Num0, cmd_ctrl),
        None,
        "there is no repo 0 selector"
    );
}

#[test]
fn cmd_ctrl_zero_opens_the_agents_dashboard() {
    let cmd_ctrl = cmd(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    assert!(open_agents_command(egui::Key::Num0, cmd_ctrl));
    // Shares the repo family's modifiers: Ctrl is required, Shift/Alt refused.
    assert!(!open_agents_command(
        egui::Key::Num0,
        cmd(egui::Modifiers::default())
    ));
    let cmd_ctrl_shift = cmd(egui::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    });
    assert!(!open_agents_command(egui::Key::Num0, cmd_ctrl_shift));
    // Only the 0 slot; the digit selectors stay with the repos.
    assert!(!open_agents_command(egui::Key::Num1, cmd_ctrl));
}

#[test]
fn positional_key_prefers_the_physical_digit_over_a_punctuation_logical() {
    // AZERTY-FR: the physical Num4 emits `'` (logical Quote). The shortcut is
    // positional, so the physical slot wins and ⌃⌘4 still selects repo 4.
    assert_eq!(
        positional_key(egui::Key::Quote, Some(egui::Key::Num4)),
        egui::Key::Num4
    );
    // Non-digit physical (Cmd+T): keep the logical key, untouched.
    assert_eq!(
        positional_key(egui::Key::T, Some(egui::Key::T)),
        egui::Key::T
    );
    // No physical key (synthetic event): fall back to the logical key.
    assert_eq!(positional_key(egui::Key::Num4, None), egui::Key::Num4);
}

#[test]
fn select_repo_resolves_an_azerty_quote_to_repo_four() {
    let cmd_ctrl = cmd(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    let key = positional_key(egui::Key::Quote, Some(egui::Key::Num4));
    assert_eq!(select_repo_command(key, cmd_ctrl), Some(3));
}

#[test]
fn select_repo_requires_command_and_ctrl_only() {
    let cmd_ctrl = cmd(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    assert_eq!(select_repo_command(egui::Key::Num1, cmd_ctrl), Some(0));
    assert_eq!(
        select_repo_command(egui::Key::Num1, cmd(egui::Modifiers::default())),
        None,
        "Cmd+1 without Ctrl selects a tab, not a repo"
    );
    let cmd_ctrl_shift = cmd(egui::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    });
    assert_eq!(select_repo_command(egui::Key::Num1, cmd_ctrl_shift), None);
    assert_eq!(
        select_repo_command(egui::Key::Num1, egui::Modifiers::default()),
        None
    );
}

#[test]
fn cmd_digits_select_tabs_one_through_nine() {
    let base = cmd(egui::Modifiers::default());
    assert_eq!(select_tab_command(egui::Key::Num1, base), Some(0));
    assert_eq!(select_tab_command(egui::Key::Num3, base), Some(2));
    assert_eq!(select_tab_command(egui::Key::Num9, base), Some(8));
    assert_eq!(
        select_tab_command(egui::Key::Num0, base),
        None,
        "Cmd+0 is reserved for the zoom reset, not a tab selector"
    );
}

#[test]
fn select_tab_requires_lone_command_modifier() {
    let cmd_ctrl = cmd(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    assert_eq!(
        select_tab_command(egui::Key::Num1, cmd_ctrl),
        None,
        "Cmd+Ctrl+1 selects a repo, not a tab"
    );
    let cmd_shift = cmd(egui::Modifiers {
        shift: true,
        ..Default::default()
    });
    assert_eq!(select_tab_command(egui::Key::Num1, cmd_shift), None);
    assert_eq!(
        select_tab_command(egui::Key::Num1, egui::Modifiers::default()),
        None
    );
}

#[test]
fn tab_action_maps_new_and_select() {
    let keymap = Keymap::default();
    assert_eq!(
        tab_action(&keymap, egui::Key::T, None, cmd(egui::Modifiers::default())),
        Some(TabAction::New)
    );
    assert_eq!(
        tab_action(
            &keymap,
            egui::Key::Num2,
            None,
            cmd(egui::Modifiers::default())
        ),
        Some(TabAction::Select(1))
    );
    let cmd_ctrl = cmd(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    assert_eq!(
        tab_action(&keymap, egui::Key::Num1, None, cmd_ctrl),
        None,
        "Cmd+Ctrl+1 (repo selector) is not a tab action"
    );
}

#[test]
fn rebinding_new_tab_leaves_the_positional_selectors_alone() {
    let mut keymap = Keymap::default();
    keymap.set(Action::NewTab, Some(Shortcut::cmd_alt(egui::Key::N)));
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        tab_action(&keymap, egui::Key::N, None, cmd(alt)),
        Some(TabAction::New)
    );
    assert_eq!(
        tab_action(&keymap, egui::Key::T, None, cmd(egui::Modifiers::default())),
        None,
        "the default Cmd+T goes dead once New tab is rebound"
    );
    assert_eq!(
        tab_action(
            &keymap,
            egui::Key::Num2,
            None,
            cmd(egui::Modifiers::default())
        ),
        Some(TabAction::Select(1)),
        "Cmd+1..9 is reserved, not affected by rebinding"
    );
}

#[test]
fn cmd_alt_d_does_not_split() {
    let keymap = Keymap::default();
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    assert_eq!(layout_command(&keymap, egui::Key::D, cmd(alt)), None);
}

#[test]
fn cmd_equals_minus_zero_map_to_zoom_commands() {
    let keymap = Keymap::default();
    let base = cmd(egui::Modifiers::default());
    assert_eq!(
        zoom_command(&keymap, egui::Key::Equals, base),
        Some(ZoomCommand::In)
    );
    assert_eq!(
        zoom_command(&keymap, egui::Key::Plus, base),
        Some(ZoomCommand::In)
    );
    // Real-world ⌘+ arrives as ⌘⇧= on layouts where `+` is the shifted `=`:
    // the Plus event folds onto Equals with the shift stripped.
    assert_eq!(
        zoom_command(
            &keymap,
            egui::Key::Plus,
            cmd(egui::Modifiers {
                shift: true,
                ..Default::default()
            })
        ),
        Some(ZoomCommand::In)
    );
    assert_eq!(
        zoom_command(&keymap, egui::Key::Minus, base),
        Some(ZoomCommand::Out)
    );
    assert_eq!(
        zoom_command(&keymap, egui::Key::Num0, base),
        Some(ZoomCommand::Reset)
    );
}

#[test]
fn zoom_requires_command_and_rejects_alt_ctrl() {
    let keymap = Keymap::default();
    assert_eq!(
        zoom_command(&keymap, egui::Key::Equals, egui::Modifiers::default()),
        None
    );
    let cmd_alt = cmd(egui::Modifiers {
        alt: true,
        ..Default::default()
    });
    assert_eq!(zoom_command(&keymap, egui::Key::Equals, cmd_alt), None);
    let cmd_ctrl = cmd(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    assert_eq!(zoom_command(&keymap, egui::Key::Minus, cmd_ctrl), None);
}

#[test]
fn default_and_min_window_geometry() {
    let opts = native_options();
    assert_eq!(opts.viewport.inner_size, Some(egui::vec2(1280.0, 800.0)));
    assert_eq!(opts.viewport.min_inner_size, Some(egui::vec2(900.0, 600.0)));
}

#[test]
fn window_geometry_persisted_by_eframe() {
    assert!(native_options().persist_window);
}

#[test]
fn dock_icon_embedded_in_viewport() {
    let icon = native_options().viewport.icon.expect("icon missing");
    assert_eq!((icon.width, icon.height), (512, 512));
}

#[test]
fn native_window_stays_opaque_with_hidden_chrome() {
    let viewport = native_options().viewport;
    assert_eq!(viewport.fullsize_content_view, Some(true));
    assert_eq!(viewport.titlebar_shown, Some(false));
    assert_eq!(
        viewport.title_shown,
        Some(false),
        "the app name must not be drawn over the content"
    );
    assert_eq!(
        viewport.transparent, None,
        "opaque native window: the sidebar no longer shows through (D-2026-06-03)"
    );
}

fn workspace_with(names: &[&str]) -> Workspace {
    let mut ws = Workspace::new();
    for name in names {
        ws.add(Repo::new(PathBuf::from(format!("/tmp/{name}"))));
    }
    ws
}

fn tagged_panes(tag: &str) -> Panes {
    let mut panes = Panes::new();
    panes.insert(PaneId(0), TerminalState::Failed(tag.to_owned()));
    panes
}

fn tag_of<'a>(panes: &'a HashMap<PaneKey, Panes>, key: &PaneKey) -> Option<&'a str> {
    match panes.get(key)?.get(&PaneId(0)) {
        Some(TerminalState::Failed(msg)) => Some(msg.as_str()),
        _ => None,
    }
}

fn key_of(ws: &Workspace, repo: usize, tab: usize) -> PaneKey {
    (
        RepoKey::of(&ws.repo(repo).unwrap().path),
        ws.tab_id(repo, tab).unwrap(),
    )
}

#[test]
fn sync_drops_the_caches_of_a_removed_repo_and_leaves_survivors_untouched() {
    let mut ws = workspace_with(&["a", "b", "c"]);
    let mut caches = RepoCaches::default();
    caches.sync(&ws);
    let (a, b, c) = (key_of(&ws, 0, 0), key_of(&ws, 1, 0), key_of(&ws, 2, 0));
    caches.panes.insert(a.clone(), tagged_panes("a-t0"));
    caches.panes.insert(b.clone(), tagged_panes("b-t0"));
    caches.panes.insert(c.clone(), tagged_panes("c-t0"));
    caches.branch_labels.insert(b.0.clone(), "main".to_owned());
    caches.branch_labels.insert(c.0.clone(), "dev".to_owned());
    caches.graph_cache.insert(
        b.0.clone(),
        (
            Graph {
                commits: Vec::new(),
                has_more: false,
            },
            10,
        ),
    );

    ws.remove(1);
    caches.sync(&ws);

    assert_eq!(
        tag_of(&caches.panes, &a),
        Some("a-t0"),
        "survivor keys do not shift on removal"
    );
    assert_eq!(tag_of(&caches.panes, &c), Some("c-t0"));
    assert!(
        !caches.panes.contains_key(&b),
        "the removed repo's PTY set is dropped"
    );
    assert!(!caches.branch_labels.contains_key(&b.0));
    assert!(!caches.graph_cache.contains_key(&b.0));
    assert_eq!(
        caches.branch_labels.get(&c.0).map(String::as_str),
        Some("dev"),
        "the survivor keeps its label under the same key"
    );
}

#[test]
fn apply_group_refresh_merges_by_key_clears_on_none_and_ignores_unknown() {
    let mut app = HelmApp::with_workspace(workspace_with(&["a", "b", "c"]));
    let key = |i: usize| RepoKey::of(&app.workspace.repo(i).unwrap().path);
    let (a, b, c) = (key(0), key(1), key(2));
    app.caches.branch_labels.insert(b.clone(), "old".to_owned());
    app.caches.dirty.insert(b.clone(), (3, 1));
    app.caches.dirty.insert(c.clone(), (5, 5));

    app.apply_group_refresh(vec![
        RepoRefresh {
            key: a.clone(),
            branch: Some("main".to_owned()),
            dirty: Some((2, 0)),
        },
        RepoRefresh {
            key: b.clone(),
            branch: Some("dev".to_owned()),
            dirty: None,
        },
        RepoRefresh {
            key: RepoKey::of(Path::new("/tmp/not-in-workspace")),
            branch: Some("ghost".to_owned()),
            dirty: Some((9, 9)),
        },
    ]);

    assert_eq!(
        app.caches.branch_labels.get(&a).map(String::as_str),
        Some("main")
    );
    assert_eq!(
        app.caches.branch_labels.get(&b).map(String::as_str),
        Some("dev"),
        "a present key is updated to the fresh label"
    );
    assert_eq!(app.caches.dirty.get(&a).copied(), Some((2, 0)));
    assert!(
        !app.caches.dirty.contains_key(&b),
        "a None dirty clears the entry (the repo went clean)"
    );
    assert_eq!(
        app.caches.dirty.get(&c).copied(),
        Some((5, 5)),
        "a repo absent from the refresh keeps its prior entry"
    );
    assert!(
        !app.caches.branch_labels.values().any(|v| v == "ghost"),
        "a key no longer in the workspace is ignored"
    );
}

#[test]
fn sync_keeps_the_caches_of_entries_moved_by_a_group_sync() {
    let mut ws = Workspace::new();
    ws.add_group(
        Repo::new(PathBuf::from("/tmp/proj")),
        vec![
            Repo::new(PathBuf::from("/tmp/wt-a")),
            Repo::new(PathBuf::from("/tmp/wt-b")),
        ],
    );
    let mut caches = RepoCaches::default();
    caches.sync(&ws);
    let wt_b = key_of(&ws, 2, 0);
    caches.panes.insert(wt_b.clone(), tagged_panes("b-t0"));

    // Purging wt-a shifts wt-b up one slot: positions shift, identities don't.
    ws.sync_group(
        Path::new("/tmp/proj"),
        vec![Repo::new(PathBuf::from("/tmp/wt-b"))],
    );
    caches.sync(&ws);

    assert_eq!(ws.repo(1).unwrap().path, PathBuf::from("/tmp/wt-b"));
    assert_eq!(
        tag_of(&caches.panes, &wt_b),
        Some("b-t0"),
        "the moved entry keeps its PTY set without any reindexing"
    );
}

#[test]
fn remove_on_a_group_root_removes_the_whole_group_and_kills_its_panes() {
    let mut ws = Workspace::new();
    ws.add_group(
        Repo::new(PathBuf::from("/tmp/proj")),
        vec![Repo::new(PathBuf::from("/tmp/wt-a"))],
    );
    ws.add(Repo::new(PathBuf::from("/tmp/solo")));
    let mut caches = RepoCaches::default();
    caches.sync(&ws);
    let (root, wt, solo) = (key_of(&ws, 0, 0), key_of(&ws, 1, 0), key_of(&ws, 2, 0));
    caches.panes.insert(root, tagged_panes("root-t0"));
    caches.panes.insert(wt, tagged_panes("wt-t0"));
    caches.panes.insert(solo.clone(), tagged_panes("solo-t0"));

    remove_repo_or_group(&mut ws, 0);
    caches.sync(&ws);

    assert_eq!(ws.len(), 1, "root and child are gone together");
    assert_eq!(ws.repo(0).unwrap().path, PathBuf::from("/tmp/solo"));
    assert_eq!(
        tag_of(&caches.panes, &solo),
        Some("solo-t0"),
        "the survivor keeps its set under its unchanged key"
    );
    assert_eq!(caches.panes.len(), 1, "the group's pane sets were killed");
}

#[test]
fn picked_non_git_folder_is_refused_and_keeps_active() {
    let tmp = tempfile::tempdir().unwrap();
    let mut ws = workspace_with(&["a", "b"]);
    ws.set_active(1);

    let outcome = add_picked_folders(&mut ws, vec![tmp.path().to_path_buf()]);

    assert!(outcome.syncs.is_empty());
    assert_eq!(outcome.rejected, vec![tmp.path().to_path_buf()]);
    assert_eq!(ws.len(), 2, "a non-git folder is not added");
    assert_eq!(ws.active(), Some(1), "a refused add does not steal focus");
}

#[test]
fn remove_active_repo_switches_to_neighbor_and_drops_all_its_tabs() {
    let mut ws = workspace_with(&["a", "b", "c"]);
    ws.set_active(1);
    ws.add_tab();
    let mut caches = RepoCaches::default();
    caches.sync(&ws);
    let (a, b0, b1, c) = (
        key_of(&ws, 0, 0),
        key_of(&ws, 1, 0),
        key_of(&ws, 1, 1),
        key_of(&ws, 2, 0),
    );
    caches.panes.insert(a.clone(), tagged_panes("a-t0"));
    caches.panes.insert(b0.clone(), tagged_panes("b-t0"));
    caches.panes.insert(b1.clone(), tagged_panes("b-t1"));
    caches.panes.insert(c.clone(), tagged_panes("c-t0"));

    remove_repo_or_group(&mut ws, 1);
    caches.sync(&ws);

    assert_eq!(
        ws.active(),
        Some(1),
        "removing the active middle repo falls back to its right neighbor"
    );
    assert_eq!(ws.active_repo().unwrap().name, "c");
    assert!(
        !caches.panes.contains_key(&b0) && !caches.panes.contains_key(&b1),
        "all tabs of the removed repo are gone, not just the first"
    );
    assert_eq!(
        tag_of(&caches.panes, &c),
        Some("c-t0"),
        "repo c keeps its panes under its unchanged key"
    );
    assert_eq!(tag_of(&caches.panes, &a), Some("a-t0"));
    assert_eq!(caches.panes.len(), 2);
}

#[test]
fn empty_workspace_has_no_panes() {
    let app = HelmApp::default();
    assert!(app.workspace.is_empty());
    assert_eq!(
        app.pane_count(),
        0,
        "with no repo there is no active layout, so no PTY is ever opened"
    );
}

fn app_with(names: &[&str]) -> HelmApp {
    HelmApp::with_workspace(workspace_with(names))
}

#[test]
fn closing_a_tab_kills_only_its_panes_and_keeps_the_others_keys() {
    let mut app = app_with(&["a"]);
    app.workspace.add_tab();
    app.workspace.add_tab();
    let (t0, t1, t2) = (
        key_of(&app.workspace, 0, 0),
        key_of(&app.workspace, 0, 1),
        key_of(&app.workspace, 0, 2),
    );
    app.caches.panes.insert(t0.clone(), tagged_panes("t0"));
    app.caches.panes.insert(t1.clone(), tagged_panes("t1"));
    app.caches.panes.insert(t2.clone(), tagged_panes("t2"));
    app.workspace.set_active_tab(1);

    assert!(app.close_active_tab(1));

    assert_eq!(app.workspace.tab_count(), Some(2));
    assert!(
        !app.caches.panes.contains_key(&t1),
        "the closed tab's PTY set is dropped"
    );
    assert_eq!(
        tag_of(&app.caches.panes, &t0),
        Some("t0"),
        "the other tabs keep their PTY set under their unchanged key"
    );
    assert_eq!(
        tag_of(&app.caches.panes, &t2),
        Some("t2"),
        "the tab above the closed one keeps its live PTY — its id never shifts"
    );
    assert_eq!(
        key_of(&app.workspace, 0, 1),
        t2,
        "the surviving tab slid down positionally but kept its id"
    );
}

#[test]
fn closing_the_sole_tab_kills_its_panes_but_keeps_the_repo() {
    let mut app = app_with(&["a"]);
    let sole = key_of(&app.workspace, 0, 0);
    app.caches.panes.insert(sole.clone(), tagged_panes("only"));

    assert!(app.close_active_tab(0));

    assert_eq!(
        app.workspace.tab_count(),
        Some(1),
        "the repo never drops to zero tabs (terminal.md §11)"
    );
    assert!(
        !app.caches.panes.contains_key(&sole),
        "the sole tab's stale PTY set is killed; the fresh tab has a fresh id"
    );
    assert_ne!(
        key_of(&app.workspace, 0, 0),
        sole,
        "the replacement tab gets a fresh id, never reusing the closed one's"
    );
}

#[test]
fn close_active_tab_without_active_repo_is_a_no_op() {
    let mut app = HelmApp::default();
    assert!(!app.close_active_tab(0));
}

#[test]
fn switching_tab_leaves_the_other_tabs_pty_sets_untouched() {
    // The render only prunes (`retain`) the **active** tab's key; the other tabs'
    // sets survive the switch (terminal.md §10: no PTY killed).
    let mut app = app_with(&["a"]);
    app.workspace.add_tab();
    let (t0, t1) = (key_of(&app.workspace, 0, 0), key_of(&app.workspace, 0, 1));
    app.caches.panes.insert(t0, tagged_panes("t0"));
    app.caches.panes.insert(t1.clone(), tagged_panes("t1"));

    app.workspace.set_active_tab(0);
    let active = app.workspace.active().unwrap();
    let active_tab = app.workspace.active_tab().unwrap();
    let pane_key = app
        .caches
        .pane_key(&app.workspace, active, active_tab)
        .unwrap();
    let live: std::collections::HashSet<PaneId> = app
        .workspace
        .active_layout()
        .unwrap()
        .pane_ids()
        .into_iter()
        .collect();
    if let Some(panes) = app.caches.panes.get_mut(&pane_key) {
        panes.retain(|id, _| live.contains(id));
    }

    assert_eq!(
        tag_of(&app.caches.panes, &t1),
        Some("t1"),
        "the inactive tab's PTY set is not pruned by the active tab's retain"
    );
}

#[test]
fn switching_repo_keeps_every_tab_pty_set_of_the_repo_left_behind() {
    // Switch A→B→A: no PTY is killed on a repo switch (terminal.md §10). The render
    // only prunes the active tab's key of the **current** repo; the sets of all the
    // other repo's tabs stay intact, so A gets its set back.
    let mut app = app_with(&["a", "b"]);
    app.workspace.set_active(0);
    app.workspace.add_tab();
    app.workspace.set_active_tab(1);
    let (a0, a1) = (key_of(&app.workspace, 0, 0), key_of(&app.workspace, 0, 1));
    app.caches.panes.insert(a0.clone(), tagged_panes("a-t0"));
    app.caches.panes.insert(a1.clone(), tagged_panes("a-t1"));

    app.workspace.set_active(1);
    let active = app.workspace.active().unwrap();
    let active_tab = app.workspace.active_tab().unwrap();
    let pane_key = app
        .caches
        .pane_key(&app.workspace, active, active_tab)
        .unwrap();
    let live: std::collections::HashSet<PaneId> = app
        .workspace
        .active_layout()
        .unwrap()
        .pane_ids()
        .into_iter()
        .collect();
    app.caches
        .panes
        .entry(pane_key.clone())
        .or_default()
        .insert(PaneId(0), TerminalState::Failed("b-t0".to_owned()));
    if let Some(panes) = app.caches.panes.get_mut(&pane_key) {
        panes.retain(|id, _| live.contains(id));
    }

    assert!(app.workspace.set_active(0));
    assert_eq!(
        app.workspace.active_tab(),
        Some(1),
        "switching back restores a's active tab"
    );
    assert_eq!(
        tag_of(&app.caches.panes, &a0),
        Some("a-t0"),
        "a's first tab keeps its live PTY across the round trip"
    );
    assert_eq!(
        tag_of(&app.caches.panes, &a1),
        Some("a-t1"),
        "a's second tab keeps its live PTY across the round trip"
    );
}

#[test]
fn restart_gives_each_repo_exactly_one_fresh_tab() {
    // Cross-session persistence: the repo list is restored, but neither the PTYs nor
    // the tab count/order (terminal.md §10). On restart each repo restarts with a
    // fresh tab, and no terminal session is resurrected.
    let prefs = Prefs {
        projects: vec![
            Project {
                root: PathBuf::from("/tmp/helm-restart-a"),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            },
            Project {
                root: PathBuf::from("/tmp/helm-restart-b"),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            },
        ],
        active: Some(PathBuf::from("/tmp/helm-restart-b")),
        ..Prefs::default()
    };

    let mut app = HelmApp::from_prefs(prefs);

    assert!(
        app.caches.panes.is_empty(),
        "no terminal session is restored from prefs"
    );
    for repo in 0..app.workspace.len() {
        assert!(app.workspace.set_active(repo));
        assert_eq!(
            app.workspace.tab_count(),
            Some(1),
            "each restored repo starts with a single tab"
        );
        assert_eq!(app.workspace.active_tab(), Some(0));
        assert_eq!(
            app.workspace.active_layout().unwrap().pane_ids().len(),
            1,
            "the fresh tab is a single-pane tree"
        );
    }
}

fn init_repo_with_commit(dir: &Path) {
    let repo = git2::Repository::init(dir).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
}

/// Adds an empty commit (same tree) on HEAD.
fn add_empty_commit(dir: &Path, message: &str) {
    let repo = git2::Repository::open(dir).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
        .unwrap();
}

#[test]
fn a_superseded_graph_reply_is_discarded() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    add_empty_commit(tmp.path(), "c2");
    add_empty_commit(tmp.path(), "c3");
    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(0, tmp.path(), &ctx, AiRunner::new(tmp.path(), || {}));

    // FIFO worker: a graph request superseded by a fresher one (reset on
    // entering Graph mode, **Load more**) precedes it — the staleness gate
    // (M17-13) must drop its reply (page truncated to 2 commits): never
    // adopted, never realigning `graph_limit`.
    session.worker.send(GitCommand::Graph { limit: 2 });
    session.worker.send(GitCommand::Graph {
        limit: session.graph_limit,
    });

    let mut diff = None;
    let mut editor = BranchEditor::default();
    let mut panel = GitPanelState::default();
    let mut toasts = Toasts::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "the fresh graph never arrived"
        );
        session.drain(
            &mut diff,
            &mut editor,
            &mut panel,
            &mut None,
            &mut None,
            &mut None,
            &mut toasts,
            0.0,
        );
        if let Some(graph) = &session.graph {
            assert_eq!(
                graph.commits.len(),
                3,
                "the stale reply (limit 2) was adopted"
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(session.graph_limit, graph::PAGE_SIZE);
}

#[test]
fn graph_poll_does_not_supersede_an_in_flight_graph() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(0, tmp.path(), &ctx, AiRunner::new(tmp.path(), || {}));

    session.worker.send(GitCommand::Graph {
        limit: graph::PAGE_SIZE,
    });
    session.last_poll = 0.0;

    session.poll(GIT_POLL_INTERVAL.as_secs_f64(), None, true);

    assert!(
        !session.worker.superseded(1, ResultKind::Graph),
        "the poll must not replace a graph request that is still awaiting adoption"
    );
}

#[test]
fn cached_graph_defers_the_head_autoscroll_to_the_fresh_one() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(a.path().to_path_buf()));
    ws.add(Repo::new(b.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    app.central_mode = CentralMode::Graph;
    let ctx = egui::Context::default();

    let wait_graph = |app: &mut HelmApp, ctx: &egui::Context, what: &str| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.git.as_ref().is_none_or(|g| g.graph.is_none()) {
            assert!(std::time::Instant::now() < deadline, "{what} never arrived");
            std::thread::sleep(std::time::Duration::from_millis(10));
            app.sync_git_session(ctx);
        }
    };
    app.sync_git_session(&ctx);
    wait_graph(&mut app, &ctx, "A's graph");

    // Switch A → B → A: on return, the graph served from cache may date from a HEAD
    // moved in the meantime — not fresh, the scroll-to-head one-shot must not be
    // consumed on it.
    app.workspace.set_active(1);
    app.sync_git_session(&ctx);
    app.workspace.set_active(0);
    app.sync_git_session(&ctx);
    {
        let git = app.git.as_ref().unwrap();
        assert!(git.graph.is_some(), "graph served from the cache");
        assert!(!git.graph_fresh, "the cached graph is not fresh");
        assert!(git.scroll_to_head, "the scroll-to-head stays armed");
    }

    // The worker's fresh graph arrives: the session becomes fresh again, consuming
    // the one-shot is honored once more.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.git.as_ref().unwrap().graph_fresh {
        assert!(
            std::time::Instant::now() < deadline,
            "the fresh graph never replaced the cache"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.sync_git_session(&ctx);
    }
    assert!(app.git.as_ref().unwrap().scroll_to_head);
}

#[test]
fn repo_switch_in_graph_mode_serves_the_cached_graph_instantly() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(a.path().to_path_buf()));
    ws.add(Repo::new(b.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    app.central_mode = CentralMode::Graph;
    let ctx = egui::Context::default();

    // First pass on A: the graph arrives from the worker (bounded wait).
    app.sync_git_session(&ctx);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.git.as_ref().is_none_or(|g| g.graph.is_none()) {
        assert!(
            std::time::Instant::now() < deadline,
            "repo A's graph never arrived"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.sync_git_session(&ctx);
    }

    // Switch A → B: A's graph goes into the cache.
    app.workspace.set_active(1);
    app.sync_git_session(&ctx);
    assert!(app.caches.graph_cache.contains_key(&RepoKey::of(a.path())));

    // Switch-back: the graph is served from the cache, without a loader nor a worker
    // wait (the fresh reload will replace it).
    app.workspace.set_active(0);
    app.sync_git_session(&ctx);
    let git = app.git.as_ref().unwrap();
    assert_eq!(git.index, 0);
    let graph = git.graph.as_ref().expect("graph served from the cache");
    assert_eq!(graph.commits.len(), 1);
}

/// Fake provider binary: ignores its args and prints `reply` (an AI
/// commit-message reply) on stdout.
fn fake_commit_provider(dir: &Path, reply: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-ai");
    std::fs::write(&path, format!("#!/bin/sh\nprintf '%s' '{reply}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Stages a new file so AI generation has a staged diff to describe.
fn stage_a_file(dir: &Path) {
    let repo = git2::Repository::open(dir).unwrap();
    std::fs::write(dir.join("staged.rs"), "fn added() {}\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("staged.rs")).unwrap();
    index.write().unwrap();
}

#[test]
fn a_commit_draft_stays_with_its_repo_across_a_switch() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(a.path().to_path_buf()));
    ws.add(Repo::new(b.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);

    // A draft typed on A.
    app.git_panel_state.subject = "Add login form".to_owned();
    app.git_panel_state.description = "Wire the auth flow.".to_owned();

    // Switch A → B: the draft must not follow into B's sidebar.
    app.workspace.set_active(1);
    app.sync_git_session(&ctx);
    assert_eq!(app.git_panel_state.subject, "");
    assert_eq!(app.git_panel_state.description, "");

    // Switch-back B → A: A's draft is restored.
    app.workspace.set_active(0);
    app.sync_git_session(&ctx);
    assert_eq!(app.git_panel_state.subject, "Add login form");
    assert_eq!(app.git_panel_state.description, "Wire the auth flow.");
}

#[test]
fn an_ai_generation_survives_a_repo_switch() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    stage_a_file(a.path());
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_commit_provider(bin.path(), "Generated subject\n\nGenerated body.");
    let mut ws = Workspace::new();
    ws.add(Repo::new(a.path().to_path_buf()));
    ws.add(Repo::new(b.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);

    // Launch generation on A, then switch away before it finishes: the runner is
    // parked, not dropped with the session.
    assert!(app
        .git
        .as_mut()
        .unwrap()
        .ai
        .request_program(provider, &[], String::new()));
    app.workspace.set_active(1);
    app.sync_git_session(&ctx);

    // Back on A: the queued suggestion lands in the commit inputs (bounded wait).
    app.workspace.set_active(0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.git_panel_state.subject.is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the AI suggestion never landed after the switch-back"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.sync_git_session(&ctx);
    }
    assert_eq!(app.git_panel_state.subject, "Generated subject");
    assert_eq!(app.git_panel_state.description, "Generated body.");
}

#[test]
fn prefs_do_not_carry_tab_state() {
    // Non-regression guardrail: if a tab field were added to `Prefs`, this
    // round-trip would break — tabs must never transit through the TOML.
    let prefs = prefs_from_workspace(Prefs::default(), &workspace_with(&["a"]));
    let restored = Prefs::from_toml(&prefs.to_toml().unwrap()).unwrap();
    assert_eq!(restored, prefs);
}

#[test]
fn prefs_rewritten_from_workspace_keep_order_and_active() {
    let mut ws = workspace_with(&["a", "b", "c"]);
    ws.set_active(2);
    ws.remove(0);

    let prefs = prefs_from_workspace(Prefs::default(), &ws);

    let roots: Vec<&str> = prefs
        .projects
        .iter()
        .map(|p| p.root.to_str().unwrap())
        .collect();
    assert_eq!(roots, vec!["/tmp/b", "/tmp/c"]);
    assert_eq!(prefs.active, Some(PathBuf::from("/tmp/c")));
}

#[test]
fn from_prefs_restores_groups_and_activates_the_saved_path() {
    let prefs = Prefs {
        projects: vec![
            Project {
                root: PathBuf::from("/tmp/proj"),
                worktrees: vec![PathBuf::from("/tmp/feat"), PathBuf::from("/tmp/alpha")],
                collapsed: false,
                hidden: false,
            },
            Project {
                root: PathBuf::from("/tmp/solo"),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            },
        ],
        active: Some(PathBuf::from("/tmp/feat")),
        ..Prefs::default()
    };

    let app = HelmApp::from_prefs(prefs);

    let names: Vec<&str> = app.workspace.repos().map(|r| r.name.as_str()).collect();
    assert_eq!(
            names,
            vec!["proj", "feat", "alpha", "solo"],
            "flattened order: root then children in their persisted (manual) order, then the next project"
        );
    assert_eq!(
        app.workspace.parent_root(1),
        Some(Path::new("/tmp/proj")),
        "feat is restored as a worktree of proj"
    );
    assert_eq!(app.workspace.active_repo().unwrap().name, "feat");
}

#[test]
fn from_prefs_restores_a_collapsed_group_and_round_trips_the_flag() {
    let prefs = Prefs {
        projects: vec![
            Project {
                root: PathBuf::from("/tmp/proj"),
                worktrees: vec![PathBuf::from("/tmp/feat")],
                collapsed: true,
                hidden: false,
            },
            Project {
                root: PathBuf::from("/tmp/solo"),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            },
        ],
        active: Some(PathBuf::from("/tmp/proj")),
        ..Prefs::default()
    };

    let app = HelmApp::from_prefs(prefs);

    assert!(
        app.workspace.is_collapsed(0),
        "the saved fold state is restored on the group root"
    );
    assert!(
        app.workspace.is_hidden(0) && app.workspace.is_hidden(1),
        "a collapsed group hides its main and worktree rows — only the header shows"
    );

    let saved = prefs_from_workspace(Prefs::default(), &app.workspace);
    assert!(
        saved.projects[0].collapsed,
        "the fold state survives back into prefs"
    );
    assert!(
        !saved.projects[1].collapsed,
        "the standalone project stays expanded"
    );
}

#[test]
fn prefs_from_workspace_nests_worktrees_under_their_root() {
    let mut ws = Workspace::new();
    ws.add_group(
        Repo::new(PathBuf::from("/tmp/proj")),
        vec![Repo::new(PathBuf::from("/tmp/feat"))],
    );
    ws.add(Repo::new(PathBuf::from("/tmp/solo")));
    ws.set_active(1);

    let prefs = prefs_from_workspace(Prefs::default(), &ws);

    assert_eq!(
        prefs.projects,
        vec![
            Project {
                root: PathBuf::from("/tmp/proj"),
                worktrees: vec![PathBuf::from("/tmp/feat")],
                collapsed: false,
                hidden: false,
            },
            Project {
                root: PathBuf::from("/tmp/solo"),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            },
        ]
    );
    assert_eq!(prefs.active, Some(PathBuf::from("/tmp/feat")));
}

// Regression: a test exercising `remove_repo` rewrote the user's real prefs.toml
// (their open repos were replaced by /tmp/a, /tmp/c).
#[test]
fn persist_is_a_no_op_without_an_injected_prefs_path_and_targets_it_otherwise() {
    let mut app = app_with(&["a"]);
    assert_eq!(
        app.prefs_path, None,
        "test/headless apps must stay ephemeral"
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.toml");
    app.prefs_path = Some(path.clone());
    let next = prefs_from_workspace(app.prefs.clone(), &app.workspace);
    app.persist(move |_| next);
    app.flush_prefs();

    let saved = Prefs::load_from(&path);
    assert_eq!(
        saved.projects,
        vec![Project {
            root: PathBuf::from("/tmp/a"),
            worktrees: Vec::new(),
            collapsed: false,
            hidden: false,
        }]
    );
    assert_eq!(saved.active, Some(PathBuf::from("/tmp/a")));
}

#[test]
fn persist_buffers_in_memory_and_flush_writes_once() {
    let mut app = app_with(&["a"]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.toml");
    app.prefs_path = Some(path.clone());

    app.persist(|prefs| Prefs {
        left_sidebar_width: 300.0,
        ..prefs
    });
    assert!(!path.exists(), "no disk write inside the frame");
    app.persist(|prefs| Prefs {
        right_sidebar_width: 400.0,
        ..prefs
    });

    app.flush_prefs();
    let saved = Prefs::load_from(&path);
    assert_eq!(saved.left_sidebar_width, 300.0, "burst coalesced");
    assert_eq!(saved.right_sidebar_width, 400.0);

    std::fs::remove_file(&path).unwrap();
    app.flush_prefs();
    assert!(!path.exists(), "flush without a pending change is a no-op");
}

#[test]
fn prefs_flush_waits_for_the_debounce_then_is_due() {
    let dirty_at = Instant::now();
    let wait = prefs_flush_wait(dirty_at, dirty_at).unwrap();
    assert_eq!(wait, PREFS_DEBOUNCE);

    let almost = dirty_at + PREFS_DEBOUNCE - Duration::from_millis(1);
    assert_eq!(
        prefs_flush_wait(dirty_at, almost),
        Some(Duration::from_millis(1))
    );

    assert_eq!(prefs_flush_wait(dirty_at, dirty_at + PREFS_DEBOUNCE), None);
}

#[test]
fn sidebar_visibility_is_persisted_only_when_a_toggle_changed_it() {
    let mut app = app_with(&["a"]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.toml");
    app.prefs_path = Some(path.clone());

    app.persist_sidebar_visibility_if_changed(app.sidebars);
    app.flush_prefs();
    assert!(!path.exists(), "no write when nothing changed");

    let was = app.sidebars;
    app.sidebars.workspace = false;
    app.sidebars.git = true;
    app.persist_sidebar_visibility_if_changed(was);
    app.flush_prefs();

    let saved = Prefs::load_from(&path);
    assert!(!saved.show_workspace);
    assert!(saved.show_git);
}

#[test]
fn the_preferences_route_toggles_between_the_two_pages() {
    assert_eq!(Page::Main.toggled(), Page::Preferences);
    assert_eq!(Page::Preferences.toggled(), Page::Main);
}

fn open_diff(path: &str, staged: bool) -> DiffState {
    DiffState {
        source: DiffSource::WorkingTree { staged },
        path: path.to_owned(),
        loaded: None,
        inherited: false,
        view: DiffViewState::default(),
    }
}

fn loaded_file(path: &str) -> FileDiff {
    FileDiff {
        path: path.to_owned(),
        binary: false,
        oversize: false,
        hunks: Vec::new(),
        source_lines: Vec::new(),
        image: None,
    }
}

#[test]
fn open_inherits_loaded_content_within_the_same_source_kind() {
    let mut slot = Some(open_diff("a.rs", false));
    slot.as_mut().unwrap().adopt(loaded_file("a.rs"));

    DiffState::open(
        &mut slot,
        DiffSource::WorkingTree { staged: true },
        "b.rs".into(),
    );

    let open = slot.unwrap();
    assert_eq!(open.path, "b.rs");
    assert!(open.inherited, "previous content stays frozen on screen");
    assert!(open.loaded.is_some());
}

#[test]
fn open_across_source_kinds_starts_fresh() {
    let mut slot = Some(open_diff("a.rs", false));
    slot.as_mut().unwrap().adopt(loaded_file("a.rs"));

    DiffState::open(
        &mut slot,
        DiffSource::Commit(git2::Oid::ZERO_SHA1),
        "a.rs".into(),
    );

    let open = slot.unwrap();
    assert!(
        !open.inherited && open.loaded.is_none(),
        "a commit diff must not inherit working-tree content (wrong chrome)"
    );
}

#[test]
fn adopt_resets_the_inherited_flag_and_takes_the_new_content() {
    let mut slot = Some(open_diff("a.rs", false));
    slot.as_mut().unwrap().adopt(loaded_file("a.rs"));
    DiffState::open(
        &mut slot,
        DiffSource::WorkingTree { staged: false },
        "b.rs".into(),
    );

    let open = slot.as_mut().unwrap();
    open.adopt(loaded_file("b.rs"));

    assert!(!open.inherited);
    assert_eq!(open.loaded.as_ref().unwrap().path, "b.rs");
}

#[test]
fn granular_staging_intents_are_dropped_on_a_commit_diff() {
    let open = DiffState {
        source: DiffSource::Commit(git2::Oid::ZERO_SHA1),
        path: "src/main.rs".to_owned(),
        loaded: None,
        inherited: false,
        view: DiffViewState::default(),
    };
    assert_eq!(
        overlay_or_command(GitIntent::StageHunk(0), Some(&open)),
        None
    );
    assert_eq!(
        overlay_or_command(
            GitIntent::StageLines {
                hunk: 0,
                lines: vec![1],
            },
            Some(&open),
        ),
        None
    );
}

#[test]
fn granular_staging_intents_join_the_open_diff_path() {
    let open = open_diff("src/main.rs", false);
    assert_eq!(
        overlay_or_command(GitIntent::StageHunk(2), Some(&open)),
        Some(GitCommand::StageHunk {
            path: "src/main.rs".into(),
            hunk: 2,
        })
    );
    assert_eq!(
        overlay_or_command(GitIntent::UnstageHunk(0), Some(&open)),
        Some(GitCommand::UnstageHunk {
            path: "src/main.rs".into(),
            hunk: 0,
        })
    );
    assert_eq!(
        overlay_or_command(
            GitIntent::StageLines {
                hunk: 1,
                lines: vec![3, 4],
            },
            Some(&open),
        ),
        Some(GitCommand::StageLines {
            path: "src/main.rs".into(),
            hunk: 1,
            lines: vec![3, 4],
        })
    );
    assert_eq!(
        overlay_or_command(
            GitIntent::UnstageLines {
                hunk: 1,
                lines: vec![3, 4],
            },
            Some(&open),
        ),
        Some(GitCommand::UnstageLines {
            path: "src/main.rs".into(),
            hunk: 1,
            lines: vec![3, 4],
        })
    );
}

#[test]
fn discard_hunk_is_never_routed_straight_to_the_worker() {
    // It arms a confirmation modal in the intent loop instead; the command
    // path must not produce a worker command for it.
    let open = open_diff("src/main.rs", false);
    assert_eq!(
        overlay_or_command(GitIntent::DiscardHunk(0), Some(&open)),
        None
    );
    assert_eq!(overlay_or_command(GitIntent::DiscardHunk(0), None), None);
}

#[test]
fn granular_staging_intents_are_dropped_without_an_open_diff() {
    assert_eq!(overlay_or_command(GitIntent::StageHunk(0), None), None);
    assert_eq!(overlay_or_command(GitIntent::UnstageHunk(0), None), None);
    assert_eq!(
        overlay_or_command(
            GitIntent::StageLines {
                hunk: 0,
                lines: vec![1],
            },
            None,
        ),
        None
    );
    assert_eq!(
        overlay_or_command(
            GitIntent::UnstageLines {
                hunk: 0,
                lines: vec![1],
            },
            None,
        ),
        None
    );
}

#[test]
fn from_prefs_restores_repos_active_theme_and_sidebar_state() {
    let prefs = Prefs {
        projects: vec![
            Project {
                root: PathBuf::from("/tmp/helm-from-prefs-a"),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            },
            Project {
                root: PathBuf::from("/tmp/helm-from-prefs-b"),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            },
        ],
        active: Some(PathBuf::from("/tmp/helm-from-prefs-b")),
        theme: ThemeMode::Dark,
        light_theme: "github".to_owned(),
        dark_theme: "tokyo".to_owned(),
        left_sidebar_width: 240.0,
        right_sidebar_width: 360.0,
        show_workspace: false,
        show_git: true,
        pull_default: PullDefault::Rebase,
        ai_provider: AiProvider::Opencode,
        ai_instructions: "Use conventional commits.".to_owned(),
        ai_rebase_provider: AiProvider::Codex,
        editor: Editor::default(),
        notify_on_agent_completion: true,
        agents_view: crate::ui::agents_view::AgentsViewMode::default(),
        agents_column_width: 672.0,
        agents_terminal_height: 360.0,
        git_file_view: crate::ui::file_list::FileViewMode::default(),
        run_panel_height: 200.0,
        run_panel_collapsed: false,
        workspace_opener: WorkspaceOpener::default(),
        last_seen_version: String::new(),
        keybindings: std::collections::BTreeMap::new(),
        project_settings: Vec::new(),
    };

    let app = HelmApp::from_prefs(prefs);

    let names: Vec<&str> = app.workspace.repos().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["helm-from-prefs-a", "helm-from-prefs-b"],
        "the persisted repo order is restored"
    );
    assert_eq!(
        app.workspace.active(),
        Some(1),
        "the persisted active repo is restored"
    );
    assert_eq!(app.theme_mode, ThemeMode::Dark);
    assert_eq!(app.light_theme, "github");
    assert_eq!(app.dark_theme, "tokyo");
    assert_eq!(app.left_sidebar_width, 240.0);
    assert_eq!(app.right_sidebar_width, 360.0);
    assert!(
        !app.sidebars.workspace,
        "persisted closed state is restored"
    );
    assert!(app.sidebars.git, "persisted open state is restored");
    assert_eq!(app.pull_default, PullDefault::Rebase);
    assert_eq!(app.ai_provider, AiProvider::Opencode);
    assert_eq!(app.ai_instructions, "Use conventional commits.");
    assert_eq!(app.ai_rebase_provider, AiProvider::Codex);
}

#[test]
fn from_prefs_ignores_unknown_active_path_and_keeps_the_first_repo() {
    let prefs = Prefs {
        projects: vec![Project {
            root: PathBuf::from("/tmp/helm-from-prefs-only"),
            worktrees: Vec::new(),
            collapsed: false,
            hidden: false,
        }],
        active: Some(PathBuf::from("/tmp/helm-not-in-the-list")),
        ..Prefs::default()
    };

    let app = HelmApp::from_prefs(prefs);

    assert_eq!(app.workspace.len(), 1);
    assert_eq!(
        app.workspace.active(),
        Some(0),
        "an unknown persisted active path falls back to the first added repo"
    );
}

#[test]
fn default_app_carries_default_sidebar_widths() {
    let app = HelmApp::default();
    let defaults = Prefs::default();
    assert_eq!(app.left_sidebar_width, defaults.left_sidebar_width);
    assert_eq!(app.right_sidebar_width, defaults.right_sidebar_width);
}

#[test]
fn persist_egui_memory_is_disabled_so_toml_owns_sidebar_widths() {
    let app = HelmApp::default();
    assert!(
        !eframe::App::persist_egui_memory(&app),
        "egui memory persistence must be off so the TOML is the only width source"
    );
}

#[test]
fn open_diff_intent_is_not_a_worker_command() {
    // Opening is handled by the app (overlay state), not sent as is.
    assert_eq!(
        git_command(GitIntent::OpenDiff {
            path: "a.txt".into(),
            staged: false,
        }),
        None
    );
}

#[cfg(target_os = "macos")]
#[test]
fn marked_path_survives_rc_file_noise() {
    let output = "rc banner\n__helm_path__/opt/homebrew/bin:/usr/bin__helm_path__\n";
    assert_eq!(
        parse_marked_path(output).as_deref(),
        Some("/opt/homebrew/bin:/usr/bin")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn marked_path_rejects_a_missing_or_empty_fence() {
    assert_eq!(parse_marked_path("no markers here"), None);
    assert_eq!(parse_marked_path("__helm_path____helm_path__"), None);
    assert_eq!(parse_marked_path("__helm_path__   __helm_path__"), None);
}

#[cfg(target_os = "macos")]
#[test]
fn bundle_launch_is_distinguished_from_a_dev_binary() {
    assert!(exe_in_app_bundle(Path::new(
        "/Applications/Helm.app/Contents/MacOS/helm"
    )));
    assert!(!exe_in_app_bundle(Path::new(
        "/Users/me/dev/helm/target/debug/helm"
    )));
}
