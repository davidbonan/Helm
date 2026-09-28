use egui::Modifiers;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lucide_icons::Icon;

use helm::command_palette::{CommandPalette, Screen};
use helm::theme::Palette;
use helm::ui::command_palette::{command_palette_modal, Leading, PaletteRow, PaletteView};

struct PaletteState {
    palette: CommandPalette,
    rows: Vec<PaletteRow>,
    recent_rows: usize,
    activated: Option<usize>,
    removed: Option<usize>,
    dismissed: bool,
}

fn harness(palette: CommandPalette, rows: Vec<PaletteRow>) -> Harness<'static, PaletteState> {
    harness_with_recent(palette, rows, 0)
}

fn harness_with_recent(
    palette: CommandPalette,
    rows: Vec<PaletteRow>,
    recent_rows: usize,
) -> Harness<'static, PaletteState> {
    Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui_state(
            |ui, state| {
                let view = PaletteView {
                    rows: Some(&state.rows),
                    scope: None,
                    recent_rows: state.recent_rows,
                };
                let action = command_palette_modal(ui, &Palette::dark(), &mut state.palette, &view);
                state.activated = state.activated.or(action.activate);
                state.removed = state.removed.or(action.remove);
                state.dismissed |= action.dismiss;
            },
            PaletteState {
                palette,
                rows,
                recent_rows,
                activated: None,
                removed: None,
                dismissed: false,
            },
        )
}

fn commands() -> Vec<PaletteRow> {
    vec![
        PaletteRow::command(Icon::Server, "Running servers", true),
        PaletteRow::command(Icon::Archive, "Stash changes", false),
    ]
}

fn numbered_commands(count: usize) -> Vec<PaletteRow> {
    (0..count)
        .map(|i| PaletteRow::command(Icon::Server, &format!("Command {i}"), true))
        .collect()
}

fn server(project: &str, branch: &str, port: u16) -> PaletteRow {
    PaletteRow {
        leading: Leading::Running,
        title: project.to_owned(),
        context: Some((Icon::GitBranch, branch.to_owned())),
        detail: Some(format!("pnpm dev --port {port}")),
        badge: Some(format!(":{port}")),
        opens_screen: false,
        enter_label: "Show",
        remove_label: Some("Stop server"),
    }
}

fn servers_screen() -> CommandPalette {
    let mut palette = CommandPalette::default();
    palette.enter(Screen::RunningServers);
    palette
}

fn type_query(harness: &mut Harness<'_, PaletteState>, text: &str) {
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput")
        .type_text(text);
    harness.run();
}

#[test]
fn typing_keeps_only_the_fuzzy_matches() {
    let mut harness = harness(CommandPalette::default(), commands());
    harness.run();

    type_query(&mut harness, "stsh");

    harness.get_by_label("Stash changes");
    assert!(harness.query_by_label("Running servers").is_none());
}

#[test]
fn enter_activates_the_best_match_rather_than_the_first_row() {
    let mut harness = harness(CommandPalette::default(), commands());
    harness.run();
    type_query(&mut harness, "stash");

    harness.key_press(egui::Key::Enter);
    harness.run();

    assert_eq!(harness.state().activated, Some(1));
}

#[test]
fn recent_commands_are_headed_until_a_query_is_typed() {
    let mut harness = harness_with_recent(CommandPalette::default(), commands(), 1);
    harness.run();
    harness.get_by_label("RECENT");
    harness.get_by_label("OTHER COMMANDS");

    type_query(&mut harness, "s");

    assert!(harness.query_by_label("RECENT").is_none());
    assert!(harness.query_by_label("OTHER COMMANDS").is_none());
}

#[test]
fn escape_leaves_the_screen_before_closing_the_palette() {
    let mut harness = harness(servers_screen(), vec![server("api", "main", 3000)]);
    harness.run();
    harness.get_by_label("Commands");

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert_eq!(harness.state().palette.screen(), Screen::Commands);
    assert!(!harness.state().dismissed);

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().dismissed, "Esc at the root closes");
}

#[test]
fn backspace_goes_back_only_once_the_query_is_empty() {
    let mut harness = harness(servers_screen(), vec![server("api", "main", 3000)]);
    harness.run();
    type_query(&mut harness, "a");

    harness.key_press(egui::Key::Backspace);
    harness.run();
    assert_eq!(
        harness.state().palette.screen(),
        Screen::RunningServers,
        "the first Backspace edits the query"
    );

    harness.key_press(egui::Key::Backspace);
    harness.run();
    assert_eq!(harness.state().palette.screen(), Screen::Commands);
}

#[test]
fn the_cross_stops_its_row_without_selecting_it() {
    let rows = vec![
        server("api", "main", 3000),
        server("web", "feat/login", 3001),
    ];
    let mut harness = harness(servers_screen(), rows);
    harness.run();

    harness
        .get_all_by_role_and_label(egui::accesskit::Role::Button, "Stop server")
        .nth(1)
        .unwrap()
        .click();
    harness.run();

    assert_eq!(harness.state().removed, Some(1));
    assert_eq!(harness.state().activated, None);
}

#[test]
fn cmd_backspace_stops_the_highlighted_row() {
    let rows = vec![
        server("api", "main", 3000),
        server("web", "feat/login", 3001),
    ];
    let mut harness = harness(servers_screen(), rows);
    harness.run();

    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    harness.key_press_modifiers(
        Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        },
        egui::Key::Backspace,
    );
    harness.run();

    assert_eq!(harness.state().removed, Some(1));
}

#[test]
fn escape_from_a_screen_scrolls_back_to_the_row_it_was_entered_from() {
    let mut harness = harness(CommandPalette::default(), numbered_commands(20));
    harness.run();
    let list_top = harness.get_by_label("Command 0").rect().min.y;
    for _ in 0..16 {
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
    }
    harness.state_mut().palette.enter(Screen::RunningServers);
    harness.state_mut().rows = vec![server("api", "main", 3000)];
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.state_mut().rows = numbered_commands(20);
    harness.run();

    let entered_row = harness.get_by_label("Command 16").rect();
    assert!(
        entered_row.max.y <= list_top + 360.0,
        "{entered_row:?} lies below the list"
    );
}
