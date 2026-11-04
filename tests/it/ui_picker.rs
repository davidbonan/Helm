use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme::Palette;
use helm::ui::picker::{Choice, PickMany, PickOne, Toggle, Trigger};

const FRUITS: [&str; 3] = ["Apple", "Banana", "Cherry"];
const MANY: [&str; 9] = [
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
];

#[derive(Default)]
struct Picked {
    selected: usize,
    picks: Vec<usize>,
}

fn pick_one(labels: &'static [&'static str]) -> Harness<'static, Picked> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 500.0))
        .build_ui_state(
            move |ui, state: &mut Picked| {
                let palette = Palette::dark();
                let response = Trigger::new(labels[state.selected]).show(ui, &palette);
                let choices: Vec<Choice> = labels.iter().map(|l| Choice::new(l)).collect();
                if let Some(index) =
                    PickOne::new(&choices, Some(state.selected)).show(&response, &palette)
                {
                    state.selected = index;
                    state.picks.push(index);
                }
            },
            Picked::default(),
        );
    harness.run();
    harness
}

fn open(harness: &mut Harness<'static, Picked>, trigger: &str) {
    harness.get_by_label(trigger).click();
    harness.run();
    harness.run();
}

#[test]
fn picking_another_choice_reports_it_and_closes_the_menu() {
    let mut harness = pick_one(&FRUITS);
    open(&mut harness, "Apple");
    harness.get_by_label("Cherry").click();
    harness.run();

    assert_eq!(harness.state().picks, [2]);
    assert!(
        harness.query_by_label("Banana").is_none(),
        "the menu closed"
    );
}

#[test]
fn picking_the_current_choice_reports_nothing() {
    let mut harness = pick_one(&FRUITS);
    open(&mut harness, "Apple");
    harness.get_all_by_label("Apple").last().unwrap().click();
    harness.run();

    assert!(harness.state().picks.is_empty());
}

#[test]
fn arrows_and_enter_pick_from_the_keyboard() {
    let mut harness = pick_one(&FRUITS);
    open(&mut harness, "Apple");
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    assert_eq!(harness.state().picks, [1]);
}

#[test]
fn a_long_list_filters_as_you_type_and_enter_takes_the_first_match() {
    let mut harness = pick_one(&MANY);
    open(&mut harness, "Alpha");
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .type_text("ot");
    harness.run();
    assert!(harness.query_by_label("Delta").is_none(), "filtered out");
    harness.get_by_label("Foxtrot");

    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().picks, [5]);
}

#[test]
fn pick_many_reports_each_flip_and_stays_open() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, flipped: &mut Vec<usize>| {
                let palette = Palette::dark();
                let response = Trigger::new("Projects").show(ui, &palette);
                let toggles = [
                    Toggle {
                        label: "alpha",
                        on: true,
                    },
                    Toggle {
                        label: "beta",
                        on: false,
                    },
                ];
                flipped.extend(PickMany::new(&toggles).show(&response, &palette));
            },
            Vec::new(),
        );
    harness.run();
    harness.get_by_label("Projects").click();
    harness.run();
    harness.get_by_label("beta").click();
    harness.run();
    harness.get_by_label("alpha").click();
    harness.run();

    assert_eq!(*harness.state(), [1, 0]);
}
