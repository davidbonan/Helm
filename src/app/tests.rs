use super::git_session::PaneKey;
use super::keys::{
    git_command, layout_command, open_agents_command, positional_key, select_repo_command,
    select_tab_command, tab_action, zoom_command, FocusZone, LayoutCommand, TabAction, ZoomCommand,
};
use super::render::sync_agents_wall;
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
fn rekey_carries_a_renamed_worktree_caches_over_instead_of_dropping_them() {
    let mut ws = workspace_with(&["a", "b"]);
    let mut caches = RepoCaches::default();
    caches.sync(&ws);
    let (a, b) = (key_of(&ws, 0, 0), key_of(&ws, 1, 0));
    caches.panes.insert(a.clone(), tagged_panes("a-t0"));
    caches.panes.insert(b.clone(), tagged_panes("b-t0"));
    caches.branch_labels.insert(b.0.clone(), "dev".to_owned());

    assert!(ws.set_repo_path(1, PathBuf::from("/tmp/renamed")));
    let renamed = key_of(&ws, 1, 0);
    caches.rekey(&b.0, &renamed.0);
    caches.sync(&ws);

    assert_eq!(
        tag_of(&caches.panes, &renamed),
        Some("b-t0"),
        "the renamed worktree keeps its live panes under the new key"
    );
    assert!(
        !caches.panes.contains_key(&b),
        "nothing left on the old key"
    );
    assert_eq!(
        caches.branch_labels.get(&renamed.0).map(String::as_str),
        Some("dev")
    );
    assert_eq!(
        tag_of(&caches.panes, &a),
        Some("a-t0"),
        "the other repos are untouched"
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

/// Agents dashboard fixture: `agent` entries on the first repo's first tab, one per
/// pane id, in the order given. Mirrors what the agent watch rebuilds each tick.
fn app_with_agents(agents: &[(&'static str, AgentBadge)]) -> HelmApp {
    let mut app = app_with(&["a"]);
    let (repo_key, tab_id) = key_of(&app.workspace, 0, 0);
    app.caches.agents = agents
        .iter()
        .enumerate()
        .map(|(i, (agent, badge))| crate::app::git_session::AgentEntry {
            repo_key: repo_key.clone(),
            group_name: "a".to_owned(),
            branch: Some("main".to_owned()),
            tab_id,
            tab_name: format!("Tab {}", i + 1),
            pane_id: PaneId(i as u32),
            agent,
            badge: *badge,
            last_output_ms: 0,
        })
        .collect();
    app
}

fn agent_keys(app: &HelmApp) -> Vec<(RepoKey, TabId, PaneId)> {
    app.caches
        .agents
        .iter()
        .map(|e| (e.repo_key.clone(), e.tab_id, e.pane_id))
        .collect()
}

#[test]
fn opening_the_dashboard_seeds_the_wall_with_the_selected_agent() {
    let mut app = app_with_agents(&[("claude", AgentBadge::Idle), ("codex", AgentBadge::Working)]);
    let keys = agent_keys(&app);
    // The page resolves its selection first (most urgent = the Working one), then the
    // wall syncs: an empty wall opens on that agent, not on nothing.
    let selected = app.resolve_selected_agent();
    assert_eq!(selected.as_ref(), Some(&keys[1]));
    sync_agents_wall(
        &mut app.agents_wall,
        &mut app.agents_wall_seeded,
        &keys,
        selected.as_ref(),
        true,
    );
    assert_eq!(app.agents_wall.len(), 1);
    assert_eq!(app.agents_wall.focused(), Some(&keys[1]));
}

#[test]
fn a_wall_the_user_emptied_stays_empty_until_the_page_is_left() {
    let mut app = app_with_agents(&[("claude", AgentBadge::Working)]);
    let keys = agent_keys(&app);
    let selected = app.resolve_selected_agent();
    let sync = |app: &mut HelmApp, on_screen: bool| {
        sync_agents_wall(
            &mut app.agents_wall,
            &mut app.agents_wall_seeded,
            &keys,
            selected.as_ref(),
            on_screen,
        )
    };
    sync(&mut app, true);
    assert_eq!(app.agents_wall.len(), 1);
    // Hiding the last tile is an answer, not a gap: further frames leave it empty.
    app.agents_wall.hide(&keys[0]);
    sync(&mut app, true);
    sync(&mut app, true);
    assert!(app.agents_wall.is_empty());
    // Leaving the dashboard rearms the seed, so the next visit opens populated again.
    sync(&mut app, false);
    sync(&mut app, true);
    assert_eq!(app.agents_wall.len(), 1);
}

#[test]
fn an_agent_that_stops_running_loses_its_tile() {
    let mut app = app_with_agents(&[("claude", AgentBadge::Working), ("codex", AgentBadge::Idle)]);
    let keys = agent_keys(&app);
    for key in &keys {
        app.agents_wall.show(key.clone(), rect(egui::Rect::ZERO));
    }
    assert_eq!(app.agents_wall.len(), 2);
    // Its pane closed (or the agent left the foreground): the watch drops the entry.
    app.caches.agents.remove(1);
    let live = agent_keys(&app);
    sync_agents_wall(
        &mut app.agents_wall,
        &mut app.agents_wall_seeded,
        &live,
        Some(&keys[0]),
        true,
    );
    assert_eq!(app.agents_wall.len(), 1);
    assert!(app.agents_wall.shows(&keys[0]));
    // Off screen the wall is left alone — `live` is empty then, and pruning against it
    // would wipe a wall the user set up.
    sync_agents_wall(
        &mut app.agents_wall,
        &mut app.agents_wall_seeded,
        &[],
        None,
        false,
    );
    assert_eq!(app.agents_wall.len(), 1);
}

#[test]
fn a_chip_toggles_its_terminal_onto_the_wall_and_off_again() {
    let mut app = app_with_agents(&[("claude", AgentBadge::Working), ("codex", AgentBadge::Idle)]);
    let keys = agent_keys(&app);
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    app.toggle_wall_agent(0, area);
    app.toggle_wall_agent(1, area);
    assert_eq!(app.agents_wall.len(), 2);
    // Showing an agent makes it the active tile — the keyboard follows it there.
    assert_eq!(app.selected_agent.as_ref(), Some(&keys[1]));
    // Hiding it hands the keyboard to the sibling that took its room.
    app.toggle_wall_agent(1, area);
    assert!(!app.agents_wall.shows(&keys[1]));
    assert_eq!(app.selected_agent.as_ref(), Some(&keys[0]));
    // An index no longer in the list (the watch rebuilt between click and apply) is a
    // no-op, not a panic.
    app.toggle_wall_agent(9, area);
    assert_eq!(app.agents_wall.len(), 1);
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
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );

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
fn a_checkout_that_auto_stashed_says_where_the_changes_went() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &head, false).unwrap();
    std::fs::write(tmp.path().join("wip.txt"), "x\n").unwrap();
    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );

    session.worker.send(GitCommand::Checkout("feature".into()));

    let mut editor = BranchEditor::default();
    let mut panel = GitPanelState::default();
    let mut toasts = Toasts::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "the checkout never reported back"
        );
        session.drain(
            &mut None,
            &mut editor,
            &mut panel,
            &mut None,
            &mut None,
            &mut None,
            &mut toasts,
            0.0,
        );
        if session.branch.label() == "feature" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let messages: Vec<&str> = toasts.items().iter().map(|t| t.message.as_str()).collect();
    assert_eq!(
        messages,
        ["Checked out feature — your changes were stashed"],
        "the working tree left with the auto-stash: the toast must say so"
    );
}

#[test]
fn a_superseded_status_reply_still_runs_its_command_side_effects() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &head, false).unwrap();
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "one\n").unwrap();
    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );

    // Every mutation replies with a status snapshot: queued back to back, each
    // reply but the last is superseded. Their snapshots are droppable (the
    // fresher one is in flight), but the commit that emptied the composer and
    // the checkout that moved HEAD are reported by those replies and no other.
    // The fresh reply here fails, so nothing can paper over the loss.
    session.worker.send(GitCommand::StageAll);
    session
        .worker
        .send(GitCommand::Commit("wip\n\nbody".into()));
    session.worker.send(GitCommand::Checkout("feature".into()));
    session.worker.send(GitCommand::Checkout("nope".into()));

    let mut editor = BranchEditor::default();
    let mut panel = GitPanelState {
        subject: "wip".to_owned(),
        description: "body".to_owned(),
        ..GitPanelState::default()
    };
    let mut toasts = Toasts::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while session.worker.has_pending(ResultKind::Status) {
        assert!(
            std::time::Instant::now() < deadline,
            "the checkouts never reported back"
        );
        session.drain(
            &mut None,
            &mut editor,
            &mut panel,
            &mut None,
            &mut None,
            &mut None,
            &mut toasts,
            0.0,
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(
        panel.subject.is_empty() && panel.description.is_empty(),
        "the committed message stayed in the composer"
    );
    let messages: Vec<&str> = toasts.items().iter().map(|t| t.message.as_str()).collect();
    assert!(
        messages.contains(&"Checked out feature"),
        "the superseded reply took the checkout toast with it: {messages:?}"
    );
    assert!(
        !session.status_loaded,
        "the superseded snapshot was adopted"
    );
}

/// Spawns a session the way `sync_git_session` does: the lock comes from the
/// caches, not from the session.
fn spawn_session(caches: &mut RepoCaches, path: &Path, ctx: &egui::Context) -> GitSession {
    let key = RepoKey::of(path);
    let lock = caches.mutation_lock(&key);
    GitSession::spawn(key, path, ctx, AiRunner::new(path, || {}), lock)
}

#[test]
fn a_session_respawned_on_the_same_repo_inherits_the_running_op_s_lock() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let other = tempfile::tempdir().unwrap();
    init_repo_with_commit(other.path());
    let ctx = egui::Context::default();
    let mut caches = RepoCaches::default();

    // A long op takes the repo's lock, then the user switches away: the session
    // goes, its op does not (`SyncRunner` never joins its thread).
    let first = spawn_session(&mut caches, tmp.path(), &ctx);
    let guard = first
        .mutation_lock
        .try_acquire()
        .expect("lock free at spawn");
    drop(first);

    let session = spawn_session(&mut caches, tmp.path(), &ctx);
    assert!(
        session.lock_busy(),
        "the sidebar would offer mutations the op left running still refuses"
    );
    std::fs::write(tmp.path().join("a.txt"), "one\n").unwrap();
    session.worker.send(GitCommand::StageAll);
    let (_, result) = session.worker.recv().expect("the worker replied");
    let GitResult::Status { result, .. } = result else {
        panic!("StageAll replies with a status snapshot");
    };
    assert_eq!(
        result.err().map(|err| err.message().to_owned()),
        Some("another Git operation is in progress".to_owned()),
        "the running op no longer holds a lock the new session can bypass"
    );

    // Another repo has its own lock: it is not collateral damage.
    let elsewhere = spawn_session(&mut caches, other.path(), &ctx);
    assert!(!elsewhere.lock_busy());

    drop(guard);
    assert!(!session.lock_busy(), "the lock is released with the op");
}

#[test]
fn graph_poll_does_not_supersede_an_in_flight_graph() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );

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
fn a_reload_that_changes_the_diff_disarms_the_discard_hunk_confirmation() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let file = tmp.path().join("a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );

    let mut diff = Some(open_diff("a.txt", false));
    let mut modal = None;
    let mut toasts = Toasts::default();
    let mut load =
        |session: &mut GitSession, diff: &mut Option<DiffState>, modal: &mut Option<Modal>| {
            session.worker.send(GitCommand::Diff {
                path: "a.txt".to_owned(),
                staged: false,
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while session.worker.has_pending(ResultKind::Diff) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "the diff never arrived"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
                session.drain(
                    diff,
                    &mut BranchEditor::default(),
                    &mut GitPanelState::default(),
                    &mut None,
                    &mut None,
                    modal,
                    &mut toasts,
                    0.0,
                );
            }
        };

    load(&mut session, &mut diff, &mut modal);
    assert!(diff.as_ref().unwrap().loaded.is_some());

    // Confirmation armed on the content displayed…
    modal = Some(Modal::DiscardHunk {
        path: "a.txt".to_owned(),
        hunk: 0,
    });
    // …and the 1 s poll reloads the same file, unchanged: nothing addressed by the
    // confirmation moved, it stays armed (otherwise the modal would never survive a
    // second on screen).
    load(&mut session, &mut diff, &mut modal);
    assert!(
        matches!(modal, Some(Modal::DiscardHunk { hunk: 0, .. })),
        "an unchanged reload must not disarm the confirmation"
    );

    // The file changes on disk (editor, terminal `git` command): hunk 0 of the
    // reloaded diff is no longer the one the user pointed at.
    std::fs::write(&file, "zero\none\ntwo\n").unwrap();
    load(&mut session, &mut diff, &mut modal);
    assert!(
        modal.is_none(),
        "a reload that replaces the displayed diff must disarm the discard-hunk confirmation"
    );

    // A confirmation aimed at another file is not this reload's business.
    modal = Some(Modal::DiscardHunk {
        path: "b.txt".to_owned(),
        hunk: 0,
    });
    std::fs::write(&file, "one\n").unwrap();
    load(&mut session, &mut diff, &mut modal);
    assert!(matches!(
        modal,
        Some(Modal::DiscardHunk { ref path, .. }) if path == "b.txt"
    ));
}

#[test]
fn an_edit_reply_toasts_where_it_landed_and_re_requests_the_status() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let file = tmp.path().join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let repo = git2::Repository::open(tmp.path()).unwrap();
    crate::git::stage::stage(&repo, "a.txt").unwrap();
    // The Staged section's precondition is gone by the time the buffer flushes.
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();

    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );
    let mut toasts = Toasts::default();
    session.worker.send(GitCommand::EditFile(EditRequest {
        path: "a.txt".to_owned(),
        range: 1..2,
        original: vec!["two".to_owned()],
        replacement: "TWO".to_owned(),
        stage_after: true,
        force: false,
    }));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while session.worker.has_pending(ResultKind::Edit) {
        assert!(
            std::time::Instant::now() < deadline,
            "the edit reply never arrived"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        session.drain(
            &mut None,
            &mut BranchEditor::default(),
            &mut GitPanelState::default(),
            &mut None,
            &mut None,
            &mut None,
            &mut toasts,
            0.0,
        );
    }

    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "one\nTWO\nthree\n",
        "the write must land even when its stage is skipped"
    );
    assert!(
        toasts
            .items()
            .iter()
            .any(|toast| toast.message == "Saved — the file also has unstaged changes"),
        "got {:?}",
        toasts.items()
    );
    // The reply carries no snapshot: the panel is refreshed behind it, without
    // waiting for the 1 s poll.
    assert!(session.worker.has_pending(ResultKind::Status));
}

#[test]
fn an_open_inline_editor_freezes_the_diff_under_it() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    std::fs::write(tmp.path().join("a.txt"), "one\ntwo\n").unwrap();
    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );
    let mut diff = open_diff("a.txt", false);
    diff.view
        .open_editor_for_test("a.txt", 0, 0..2, &["one", "two"]);

    session.poll(10.0, Some(&diff), false);

    assert!(
        session.worker.has_pending(ResultKind::Status),
        "the status keeps polling — only the diff is frozen (git.md §7)"
    );
    assert!(
        !session.worker.has_pending(ResultKind::Diff),
        "nothing may reflow under the caret while the editor is open"
    );

    diff.view.clear();
    session.poll(20.0, Some(&diff), false);

    assert!(
        session.worker.has_pending(ResultKind::Diff),
        "the diff recomposes once the editor is gone"
    );
}

#[test]
fn a_repo_switch_writes_the_open_buffer_before_it_parks_the_session() {
    // The switch drops the whole overlay: the editor never gets another frame to blur
    // on, and the write has to reach the **leaving** repo's worker (git.md §4).
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    let file = a.path().join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();

    let mut workspace = Workspace::new();
    workspace.add(Repo::new(a.path().to_path_buf()));
    workspace.add(Repo::new(b.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(workspace);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);

    let mut diff = open_diff("a.txt", false);
    diff.adopt(loaded_file("a.txt"));
    diff.view
        .open_editor_for_test("a.txt", 0, 0..2, &["one", "two"]);
    diff.view.type_for_test("one\nTWO");
    app.diff = Some(diff);

    app.workspace.set_active(1);
    app.sync_git_session(&ctx);

    // `GitWorker::drop` joins on a queued mutation, so the write has landed by now.
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "one\nTWO\n",
        "the buffer must reach the repo it was typed in, not the one being opened"
    );
    assert!(app.diff.is_none(), "the switch drops the overlay");
}

#[test]
fn a_refused_write_raises_the_notice_and_keeps_the_buffer() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let file = tmp.path().join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();

    let ctx = egui::Context::default();
    let mut session = GitSession::spawn(
        RepoKey::of(tmp.path()),
        tmp.path(),
        &ctx,
        AiRunner::new(tmp.path(), || {}),
        MutationLock::new(),
    );
    let mut diff = Some(open_diff("a.txt", false));
    let open = diff.as_mut().unwrap();
    open.adopt(loaded_file("a.txt"));
    open.view.open_editor_for_test("a.txt", 0, 1..2, &["two"]);
    let mut toasts = Toasts::default();
    // The anchor the caret was opened on is not what the file holds any more.
    session.worker.send(GitCommand::EditFile(EditRequest {
        path: "a.txt".to_owned(),
        range: 1..2,
        original: vec!["gone".to_owned()],
        replacement: "TWO".to_owned(),
        stage_after: false,
        force: false,
    }));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while session.worker.has_pending(ResultKind::Edit) {
        assert!(
            std::time::Instant::now() < deadline,
            "the edit reply never arrived"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        session.drain(
            &mut diff,
            &mut BranchEditor::default(),
            &mut GitPanelState::default(),
            &mut None,
            &mut None,
            &mut None,
            &mut toasts,
            0.0,
        );
    }

    let open = diff.as_ref().unwrap();
    assert_eq!(
        open.view.edit_divergence().map(|r| r.replacement.clone()),
        Some("TWO".to_owned()),
        "the notice carries the very buffer that was refused"
    );
    assert!(
        open.view.inline_edit().is_some(),
        "the editor stays open on its buffer — the user arbitrates"
    );
    assert!(
        toasts.items().is_empty(),
        "the notice replaces the toast, got {:?}",
        toasts.items()
    );
    assert!(
        !session.worker.has_pending(ResultKind::Diff),
        "the diff stays frozen: the editor is still open"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
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
    assert_eq!(git.key, RepoKey::of(a.path()));
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
fn a_repo_switch_disarms_the_panel_confirmations_and_selection() {
    use crate::ui::git_panel::{DiscardTarget, GitFileSelection};

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

    // Confirmations armed on A's files, plus its selection.
    let selection = GitFileSelection {
        path: "a.txt".to_owned(),
        staged: false,
    };
    app.git_panel_state.pending_discard = Some(DiscardTarget::All);
    app.git_panel_state.pending_stash = Some(vec!["a.txt".to_owned()]);
    app.git_panel_state.selected_file = Some(selection.clone());
    app.git_panel_state.marked_files = vec![selection.clone()];
    app.git_panel_state.selection_anchor = Some(selection);

    // Switch A → B: nothing armed against A may re-render over B's session.
    app.workspace.set_active(1);
    app.sync_git_session(&ctx);
    assert!(app.git_panel_state.pending_discard.is_none());
    assert!(app.git_panel_state.pending_stash.is_none());
    assert!(app.git_panel_state.selected_file.is_none());
    assert!(app.git_panel_state.marked_files.is_empty());
    assert!(app.git_panel_state.selection_anchor.is_none());
}

#[test]
fn a_repo_switch_drops_the_confirmation_armed_on_the_previous_repo() {
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

    // Force push confirmed on A: its branch and lease describe A's remote, so
    // confirmed over B's session it would resolve the push against B.
    app.modal = Some(Modal::ForcePush {
        branch: "main".to_owned(),
        remote: "origin".to_owned(),
        lease: git2::Oid::ZERO_SHA1,
    });
    app.workspace.set_active(1);
    app.sync_git_session(&ctx);
    assert!(app.modal.is_none());
    assert!(app.modal_repo.is_none());

    // Same for a confirmation that acts by name — B may well have a `main` too.
    app.modal = Some(Modal::DeleteBranch(DeleteBranchTarget::Local(
        "main".to_owned(),
    )));
    app.workspace.set_active(0);
    app.sync_git_session(&ctx);
    assert!(app.modal.is_none());

    // A modal that addresses no repo is not the switch's business.
    app.modal = Some(Modal::WhatsNew);
    app.workspace.set_active(1);
    app.sync_git_session(&ctx);
    assert!(matches!(app.modal, Some(Modal::WhatsNew)));
}

#[test]
fn an_index_shift_on_the_same_repo_keeps_the_armed_confirmation() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(a.path().to_path_buf()));
    ws.add(Repo::new(b.path().to_path_buf()));
    ws.set_active(0);
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);

    app.modal = Some(Modal::AbortOp);
    // Index shift (sidebar reorder, worktree discovery): the active repo keeps its
    // identity, the confirmation stays armed on it.
    assert!(app.workspace.reorder(0, 1, true));
    app.caches.sync(&app.workspace);
    assert_eq!(app.workspace.active(), Some(1));
    app.sync_git_session(&ctx);
    assert!(matches!(app.modal, Some(Modal::AbortOp)));
}

#[test]
fn removing_the_active_repo_moves_the_session_to_the_repo_taking_its_place() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(a.path().to_path_buf()));
    ws.add(Repo::new(b.path().to_path_buf()));
    ws.set_active(0);
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);
    assert_eq!(app.git.as_ref().unwrap().key, RepoKey::of(a.path()));

    // Remove-from-sidebar on the active repo: `active` stays index 0, which now
    // holds B. A session keyed by index would keep reading — and writing — the
    // removed repo.
    remove_repo_or_group(&mut app.workspace, 0);
    app.caches.sync(&app.workspace);
    assert_eq!(app.workspace.active(), Some(0));
    app.sync_git_session(&ctx);
    assert_eq!(app.git.as_ref().unwrap().key, RepoKey::of(b.path()));
}

#[test]
fn an_index_shift_on_the_same_repo_does_not_respawn_the_session() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init_repo_with_commit(a.path());
    init_repo_with_commit(b.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(a.path().to_path_buf()));
    ws.add(Repo::new(b.path().to_path_buf()));
    ws.set_active(0);
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);

    app.diff = Some(open_diff("a.txt", false));
    app.branch_editor.open = true;
    app.rebase_page = Some(RebasePage {
        current: "main".to_owned(),
        onto: "origin/main".to_owned(),
        loading: true,
        error: None,
        entries: Vec::new(),
    });
    app.conflict_editor = Some(ConflictEditorState::new(Vec::new()));

    // Sidebar reorder: the active repo only shifts index, so nothing is a switch —
    // the open diff, branch editor, rebase plan and conflict editor all survive.
    assert!(app.workspace.reorder(0, 1, true));
    app.caches.sync(&app.workspace);
    assert_eq!(app.workspace.active(), Some(1));
    app.sync_git_session(&ctx);

    assert_eq!(app.git.as_ref().unwrap().key, RepoKey::of(a.path()));
    assert!(app.diff.is_some(), "the open diff was closed by a respawn");
    assert!(app.branch_editor.open);
    assert!(app.rebase_page.is_some());
    assert!(app.conflict_editor.is_some());
}

#[test]
fn a_refused_continue_leaves_the_conflict_editor_open() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_with_commit(dir.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(dir.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);
    app.conflict_editor = Some(ConflictEditorState::new(Vec::new()));

    // Runner busy ⇒ the `ContinueOp` request is refused (toast, nothing queued):
    // closing the editor here would drop the composition for an op never started.
    assert!(app
        .git
        .as_mut()
        .unwrap()
        .sync
        .request(SyncCommand::FetchAll));
    app.continue_op(0.0);
    assert!(
        app.conflict_editor.is_some(),
        "a refused Continue must not close the editor"
    );

    // Runner free again: the same Continue is accepted and closes the editor.
    while app.git.as_mut().unwrap().sync.try_recv().is_none() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.continue_op(0.0);
    assert!(app.conflict_editor.is_none());
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
        editable: false,
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
fn granular_staging_intents_are_dropped_while_the_diff_is_inherited() {
    // The overlay still shows `a.rs` frozen while `b.rs` loads: joining a hunk
    // index to `b.rs` would stage a hunk of the file on screen into another file.
    let mut slot = Some(open_diff("a.rs", false));
    slot.as_mut().unwrap().adopt(loaded_file("a.rs"));
    DiffState::open(
        &mut slot,
        DiffSource::WorkingTree { staged: false },
        "b.rs".into(),
    );
    let open = slot.as_ref().unwrap();
    assert!(open.inherited);

    assert_eq!(
        overlay_or_command(GitIntent::StageHunk(0), Some(open)),
        None
    );
    assert_eq!(
        overlay_or_command(GitIntent::UnstageHunk(0), Some(open)),
        None
    );
    assert_eq!(
        overlay_or_command(
            GitIntent::StageLines {
                hunk: 0,
                lines: vec![1],
            },
            Some(open),
        ),
        None
    );
    assert_eq!(
        overlay_or_command(
            GitIntent::UnstageLines {
                hunk: 0,
                lines: vec![1],
            },
            Some(open),
        ),
        None
    );

    // File-level intents keep flowing: they carry their own path.
    assert_eq!(
        overlay_or_command(GitIntent::Stage("c.rs".into()), Some(open)),
        Some(GitCommand::Stage("c.rs".into()))
    );

    // The requested diff arrives ⇒ the granular path reopens on `b.rs`.
    slot.as_mut().unwrap().adopt(loaded_file("b.rs"));
    assert_eq!(
        overlay_or_command(GitIntent::StageHunk(0), slot.as_ref()),
        Some(GitCommand::StageHunk {
            path: "b.rs".into(),
            hunk: 0,
        })
    );
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
        git_file_view: crate::ui::file_list::FileViewMode::default(),
        run_panel_height: 200.0,
        run_panel_collapsed: false,
        workspace_opener: WorkspaceOpener::default(),
        last_seen_version: String::new(),
        review_agent_command: "claude".to_owned(),
        bitbucket_email: String::new(),
        pr_detail_width: 460.0,
        pr_rail_collapsed: false,
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

#[test]
fn send_to_agent_opens_a_new_tab_with_a_prebuilt_agent_pane() {
    let tmp = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new();
    ws.add(Repo::new(tmp.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    // The agent runs as a job of an interactive login shell (so the terminal
    // survives the agent exiting): the pane is the shell, and the configured
    // command is fed into it rather than exec'd as the pane's root process.
    app.review_agent_command = "/bin/echo".to_owned();
    app.central_mode = CentralMode::Graph;

    let key = RepoKey::of(tmp.path());
    let mut store = crate::review::FileComments::new();
    crate::review::add_comment(
        &mut store,
        "src/a.rs",
        crate::review::LineComment {
            old_lineno: None,
            new_lineno: Some(2),
            code: "    work();".into(),
            note: "rename".into(),
        },
    );
    app.review.insert(key.clone(), store);

    let tabs_before = app.workspace.tab_count().unwrap();
    let ctx = egui::Context::default();
    app.send_review_to_agent(&ctx);

    assert!(
        !app.review.contains_key(&key),
        "the repo's comments are cleared once handed off to the agent"
    );
    assert_eq!(app.workspace.tab_count().unwrap(), tabs_before + 1);
    let active_tab = app.workspace.active_tab().unwrap();
    assert_eq!(
        active_tab, tabs_before,
        "the agent tab is appended and active"
    );
    let tab_id = app.workspace.tab_id(0, active_tab).unwrap();
    let pane = app
        .caches
        .panes
        .get(&(key, tab_id))
        .and_then(|p| p.get(&PaneId(0)));
    assert!(
        matches!(pane, Some(TerminalState::Live(_))),
        "the new tab must carry a live agent pane (the login shell running the CLI)"
    );
    assert_eq!(app.central_mode, CentralMode::Terminal);
    assert!(app.diff.is_none());
    assert_eq!(
        app.workspace.tab_titles().unwrap()[active_tab],
        "/bin/echo",
        "the agent tab is named after the configured command"
    );
}

// --- PR review cache (M-PR3 T1) ---------------------------------------------

fn github_pr(repo: &str, number: u64) -> crate::pull_requests::model::PullRequest {
    use crate::pull_requests::model::{Checks, ForgeKind, PrRole, PrState, Review};
    crate::pull_requests::model::PullRequest {
        forge_kind: ForgeKind::GitHub,
        repo_label: repo.to_owned(),
        number,
        title: "Test PR".to_owned(),
        role: PrRole::Mine,
        state: PrState::Open,
        author: "octocat".to_owned(),
        source_branch: "feature".to_owned(),
        dest_branch: "main".to_owned(),
        url: format!("https://example.test/{repo}/pull/{number}"),
        updated_at: "today".to_owned(),
        checks: Checks::Passing,
        review: Review::Pending,
        reviewers: Vec::new(),
        labels: Vec::new(),
    }
}

fn seed_review(
    key: &crate::pull_requests::runner::PrReviewKey,
    pr: &crate::pull_requests::model::PullRequest,
    root: &std::path::Path,
) -> PrReview {
    PrReview {
        key: key.clone(),
        pr: pr.clone(),
        root: root.to_path_buf(),
        fetched_at: 0.0,
        detail: None,
        detail_error: None,
        files: Vec::new(),
        base: None,
        head: None,
        all_base: None,
        all_head: None,
        selected_commit: None,
        files_loading: false,
        files_error: None,
        selected_file: None,
        diffs: std::collections::HashMap::new(),
        comment_diff_requests: std::collections::HashSet::new(),
        diff_loading: false,
        diff_error: None,
        diff_view: crate::ui::diff_view::DiffViewState::default(),
        existing: crate::review::ForgeThreads::new(),
        draft: crate::review::FileComments::new(),
        agent_notes: crate::review::FileComments::new(),
        verdict: crate::pull_requests::model::ReviewVerdict::default(),
        summary: String::new(),
        posting: false,
        post_error: None,
    }
}

#[test]
fn review_open_builds_when_absent_adopts_when_fresh_refetches_when_stale() {
    assert_eq!(review_open(false, 0.0, 60.0), ReviewOpen::Build);
    assert_eq!(review_open(true, 0.0, 60.0), ReviewOpen::Adopt);
    assert_eq!(review_open(true, 59.9, 60.0), ReviewOpen::Adopt);
    assert_eq!(review_open(true, 60.0, 60.0), ReviewOpen::AdoptAndRefetch);
    assert_eq!(review_open(true, 120.0, 60.0), ReviewOpen::AdoptAndRefetch);
}

#[test]
fn should_refresh_pr_throttles_focus_regain_but_not_cold_or_repo_change() {
    // Cold or a workspace change always refreshes, regardless of age/focus.
    assert!(should_refresh_pr(true, false, false, 0.0, 30.0));
    assert!(should_refresh_pr(false, true, false, 0.0, 30.0));
    // A focus regain refreshes only once the cache is at least `min_age` old.
    assert!(!should_refresh_pr(false, false, true, 29.9, 30.0));
    assert!(should_refresh_pr(false, false, true, 30.0, 30.0));
    // No trigger at all: never refresh, however old the cache.
    assert!(!should_refresh_pr(false, false, false, 120.0, 30.0));
}

#[test]
fn reopening_a_fresh_cached_pr_review_adopts_without_refetching() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("web");
    std::fs::create_dir_all(&root).unwrap();
    let repo = git2::Repository::init(&root).unwrap();
    repo.remote("origin", "git@github.com:acme/web.git")
        .unwrap();

    let mut ws = Workspace::new();
    ws.add(Repo::new(root.clone()));
    let mut app = HelmApp::with_workspace(ws);

    let pr = github_pr("acme/web", 7);
    app.pr_cache.pull_requests = vec![pr.clone()];
    let key = crate::pull_requests::runner::PrReviewKey {
        forge_kind: pr.forge_kind,
        repo_label: pr.repo_label.clone(),
        number: pr.number,
    };

    // A fully-loaded, fresh (fetched_at == ctx time 0.0) cached surface with a draft.
    let mut review = seed_review(&key, &pr, &root);
    crate::review::add_comment(
        &mut review.draft,
        "src/a.rs",
        crate::review::LineComment {
            old_lineno: None,
            new_lineno: Some(3),
            code: "let x = 1;".to_owned(),
            note: "keep".to_owned(),
        },
    );
    app.pr_reviews.insert(key.clone(), review);
    app.pr_active = Some(key.clone());
    app.pr_review_lru.touch(key.clone());

    let ctx = egui::Context::default();
    app.open_pr_review(0, &ctx);

    assert!(
        app.pr_review_runner.is_none() && app.pr_detail_runner.is_none(),
        "a fresh cached surface is adopted without re-running the fetch runners"
    );
    let active = app.active_review().expect("surface still active");
    assert!(
        !active.files_loading,
        "no loading spinner on a cached reopen"
    );
    assert_eq!(
        crate::review::count(&active.draft),
        1,
        "the draft survives the reopen"
    );
}

/// Two-commit repo whose `base..head` changes both `a.txt` and `b.txt`, with a GitHub
/// `origin` — the fixture for the per-PR diff cache (T2). Returns the `(base, head)` oids.
fn repo_with_two_file_diff(root: &std::path::Path) -> (git2::Oid, git2::Oid) {
    std::fs::create_dir_all(root).unwrap();
    let repo = git2::Repository::init(root).unwrap();
    repo.remote("origin", "git@github.com:acme/web.git")
        .unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();

    let commit = |tag: &str, parents: &[git2::Oid]| -> git2::Oid {
        std::fs::write(root.join("a.txt"), format!("a{tag}\n")).unwrap();
        std::fs::write(root.join("b.txt"), format!("b{tag}\n")).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.add_path(Path::new("b.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let parent_commits: Vec<git2::Commit> = parents
            .iter()
            .map(|oid| repo.find_commit(*oid).unwrap())
            .collect();
        let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &parent_refs)
            .unwrap()
    };

    let base = commit("1", &[]);
    let head = commit("2", &[base]);
    (base, head)
}

/// Builds an app whose active review for `acme/web#7` has `base..head` loaded with the
/// two changed files; seeds `diffs` with the files at `cached_indices`. Returns the app
/// and the two changed files (in `pr_changed_files` order).
fn app_with_diff_cache(
    root: &std::path::Path,
    base: git2::Oid,
    head: git2::Oid,
    cached_indices: &[usize],
) -> (HelmApp, Vec<crate::git::commit_detail::CommitFile>) {
    let repo = git2::Repository::open(root).unwrap();
    let files = crate::git::diff::pr_changed_files(&repo, base, head).unwrap();
    assert_eq!(files.len(), 2, "fixture changes exactly two files");

    let mut ws = Workspace::new();
    ws.add(Repo::new(root.to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);

    let pr = github_pr("acme/web", 7);
    app.pr_cache.pull_requests = vec![pr.clone()];
    let key = crate::pull_requests::runner::PrReviewKey {
        forge_kind: pr.forge_kind,
        repo_label: pr.repo_label.clone(),
        number: pr.number,
    };
    let mut review = seed_review(&key, &pr, root);
    review.base = Some(base);
    review.head = Some(head);
    review.files = files.clone();
    review.selected_file = Some(0);
    for &i in cached_indices {
        let diff = crate::git::diff::pr_file_diff(&repo, base, head, &files[i].path).unwrap();
        review
            .diffs
            .insert((base, head, files[i].path.clone()), diff);
    }
    app.pr_reviews.insert(key.clone(), review);
    app.pr_active = Some(key.clone());
    app.pr_review_lru.touch(key);
    (app, files)
}

#[test]
fn switching_between_cached_pr_diffs_does_not_refetch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("web");
    let (base, head) = repo_with_two_file_diff(&root);
    // Both files already cached.
    let (mut app, _files) = app_with_diff_cache(&root, base, head, &[0, 1]);

    let ctx = egui::Context::default();
    app.select_pr_file(0, &ctx);
    app.select_pr_file(1, &ctx);
    app.select_pr_file(0, &ctx);

    assert!(
        app.pr_review_runner.is_none(),
        "navigating between cached file diffs never fires a fetch"
    );
    assert!(
        !app.active_review().unwrap().diff_loading,
        "a cached file shows no diff spinner"
    );
}

#[test]
fn selecting_an_uncached_pr_diff_fetches_it_once() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("web");
    let (base, head) = repo_with_two_file_diff(&root);
    // Only the first file is cached.
    let (mut app, _files) = app_with_diff_cache(&root, base, head, &[0]);

    let ctx = egui::Context::default();
    app.select_pr_file(0, &ctx);
    assert!(
        app.pr_review_runner.is_none(),
        "the cached file is served without a fetch"
    );

    app.select_pr_file(1, &ctx);
    assert!(
        app.pr_review_runner.is_some(),
        "a cache miss fires exactly one diff fetch"
    );
    assert!(
        app.active_review().unwrap().diff_loading,
        "the selected uncached file shows its loading state"
    );
}

fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    std::fs::create_dir_all(dir).unwrap();
    let repo = git2::Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn commit_file(repo: &git2::Repository, name: &str, body: &str) {
    let dir = repo.workdir().unwrap();
    std::fs::write(dir.join(name), body).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = repo.signature().unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &parent_refs)
        .unwrap();
}

/// What one off-thread pass reads for the whole workspace, in workspace order.
fn probed(ws: &Workspace) -> Vec<RepoRefresh> {
    workspace_probes(ws).iter().map(probe_repo).collect()
}

fn probed_branches(ws: &Workspace) -> Vec<Option<String>> {
    probed(ws).into_iter().map(|r| r.branch).collect()
}

#[test]
fn probed_branches_reflect_each_repo_head_and_refuse_non_git_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    let repo = init_repo_with_identity(&repo_dir);
    commit_file(&repo, "a.txt", "x\n");
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feat/sidebar", &head, false).unwrap();
    repo.set_head("refs/heads/feat/sidebar").unwrap();
    let plain_dir = tmp.path().join("notes");
    std::fs::create_dir_all(&plain_dir).unwrap();

    let mut ws = Workspace::new();
    let outcome = add_picked_folders(&mut ws, vec![repo_dir, plain_dir.clone()]);

    assert_eq!(
        outcome.rejected,
        vec![plain_dir],
        "a non-git folder is refused"
    );
    assert_eq!(probed_branches(&ws), vec![Some("feat/sidebar".to_owned())]);
}

#[test]
fn probed_branches_of_a_worktree_group_follow_each_working_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt", "x\n");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![root_dir]);

    let head = repo.head().unwrap().shorthand().unwrap().to_owned();
    assert_eq!(
        probed_branches(&ws),
        vec![Some(head), Some("feature-x".to_owned())]
    );
}

#[test]
fn a_bare_root_and_a_gone_path_have_no_probed_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let bare_dir = tmp.path().join("proj.git");
    let repo = git2::Repository::init_bare(&bare_dir).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    let wt_path = tmp.path().join("checkout");
    repo.worktree("checkout", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![wt_path]);
    ws.add(Repo::new(PathBuf::from("/no/such/repo")));

    assert_eq!(
        probed_branches(&ws),
        vec![None, Some("checkout".to_owned()), None],
        "bare root and unreadable path stay single-line"
    );
}

#[test]
fn a_probe_reports_line_stats_only_for_the_dirty_repos() {
    let tmp = tempfile::tempdir().unwrap();
    let dirty_dir = tmp.path().join("dirty");
    let clean_dir = tmp.path().join("clean");
    let dirty = init_repo_with_identity(&dirty_dir);
    commit_file(&dirty, "a.txt", "one\ntwo\n");
    let clean = init_repo_with_identity(&clean_dir);
    commit_file(&clean, "a.txt", "one\n");
    std::fs::write(dirty_dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![dirty_dir, clean_dir]);

    let stats: Vec<Option<(usize, usize)>> = probed(&ws).into_iter().map(|r| r.dirty).collect();
    assert_eq!(
        stats,
        vec![Some((1, 0)), None],
        "the clean repo pays only the `is_dirty` probe and shows no `+N −M`"
    );
}

#[test]
fn the_headless_seam_seeds_the_sidebar_caches_from_the_probe_pass() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    let repo = init_repo_with_identity(&repo_dir);
    commit_file(&repo, "a.txt", "one\n");
    std::fs::write(repo_dir.join("a.txt"), "one\ntwo\n").unwrap();
    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![repo_dir.clone()]);

    let app = HelmApp::with_workspace(ws);

    let key = RepoKey::of(&repo_dir);
    assert_eq!(
        app.caches.branch_labels.get(&key),
        Some(&repo.head().unwrap().shorthand().unwrap().to_owned()),
        "the headless seam has its labels on frame 1, with no context to spawn on"
    );
    assert_eq!(app.caches.dirty.get(&key).copied(), Some((1, 0)));
}

fn recv_group_refresh(runner: &mut GroupRefreshRunner) -> Vec<RepoRefresh> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(reply) = runner.try_recv() {
            return reply;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the group refresh never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn a_group_refresh_requested_mid_pass_is_re_issued_once_that_pass_lands() {
    let tmp = tempfile::tempdir().unwrap();
    let first_dir = tmp.path().join("first");
    let second_dir = tmp.path().join("second");
    commit_file(&init_repo_with_identity(&first_dir), "a.txt", "x\n");
    commit_file(&init_repo_with_identity(&second_dir), "a.txt", "x\n");
    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![first_dir.clone()]);
    let before_import = workspace_probes(&ws);
    add_picked_folders(&mut ws, vec![second_dir.clone()]);
    let after_import = workspace_probes(&ws);

    let mut runner = GroupRefreshRunner::new(|| {});
    runner.request(before_import);
    // The import lands while the previous pass is in flight: dropping its request
    // would leave the new repo unlabelled until the next sync trigger.
    runner.request(after_import);

    assert_eq!(recv_group_refresh(&mut runner).len(), 1);
    let second = recv_group_refresh(&mut runner);
    assert_eq!(
        second.iter().map(|r| r.key.clone()).collect::<Vec<_>>(),
        vec![RepoKey::of(&first_dir), RepoKey::of(&second_dir)],
        "the queued request runs after the in-flight pass"
    );
    assert!(
        runner.try_recv().is_none(),
        "the requests coalesce into a single re-run"
    );
}

/// The lease the confirmation carries is the remote tip the session is showing,
/// captured when the modal is armed. Read at push time instead, it would be the
/// ref the background fetch had just refreshed — a lease that can never refuse.
#[test]
fn the_force_push_confirmation_pins_the_lease_to_the_displayed_tip() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_with_commit(dir.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(dir.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);

    let displayed = git2::Oid::from_str(&"1".repeat(40)).unwrap();
    {
        let git = app.git.as_mut().expect("a session for the active repo");
        git.branch = Branch::Named("main".to_owned());
        git.upstream_remote = Some("origin".to_owned());
        git.upstream_oid = Some(displayed);
    }

    match crate::app::render::armed_force_push(app.git.as_ref()) {
        Some(Modal::ForcePush {
            branch,
            remote,
            lease,
        }) => {
            assert_eq!(branch, "main");
            assert_eq!(remote, "origin");
            assert_eq!(lease, displayed, "the lease is not the tip that was shown");
        }
        _ => panic!("expected an armed force push confirmation"),
    }
}

#[test]
fn without_an_upstream_tip_there_is_no_force_push_to_arm() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_with_commit(dir.path());
    let mut ws = Workspace::new();
    ws.add(Repo::new(dir.path().to_path_buf()));
    let mut app = HelmApp::with_workspace(ws);
    let ctx = egui::Context::default();
    app.sync_git_session(&ctx);
    {
        let git = app.git.as_mut().expect("a session for the active repo");
        git.branch = Branch::Named("main".to_owned());
        git.upstream_remote = Some("origin".to_owned());
        git.upstream_oid = None;
    }

    assert!(crate::app::render::armed_force_push(app.git.as_ref()).is_none());
}

#[test]
fn a_cli_target_leaves_preferences_and_lands_on_the_terminal() {
    let tmp = tempfile::tempdir().unwrap();
    let other = tmp.path().join("other");
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    init_repo_with_commit(&other);
    init_repo_with_commit(&target);
    let mut ws = Workspace::new();
    ws.add(Repo::new(other.clone()));
    let mut app = HelmApp::with_workspace(ws);
    app.page = Page::Preferences;
    app.central_mode = CentralMode::Agents;
    let ctx = egui::Context::default();

    app.open_cli_target(&target, &ctx);

    assert_eq!(app.page, Page::Main, "the Preferences page is left");
    assert_eq!(app.central_mode, CentralMode::Terminal);
    assert_eq!(
        app.workspace.active_repo().map(|r| r.path.clone()),
        Some(std::fs::canonicalize(&target).unwrap()),
        "the target is the active row, the previously open repo is not"
    );
    assert_eq!(app.workspace.len(), 2, "the unknown project was imported");
    assert!(
        !app.caches.keys.is_empty(),
        "the per-repo caches follow the new membership"
    );
}

#[test]
fn a_refused_cli_target_changes_nothing_but_the_toasts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let plain = tmp.path().join("documents");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&plain).unwrap();
    init_repo_with_commit(&repo);
    let mut ws = Workspace::new();
    ws.add(Repo::new(repo.clone()));
    let mut app = HelmApp::with_workspace(ws);
    app.page = Page::Preferences;
    let ctx = egui::Context::default();

    app.open_cli_target(&plain, &ctx);

    assert_eq!(app.page, Page::Preferences, "a refusal moves nothing");
    assert_eq!(app.workspace.len(), 1);
    assert_eq!(
        app.workspace.active_repo().map(|r| r.path.clone()),
        Some(repo)
    );
}
