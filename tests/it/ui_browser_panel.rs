//! The browser panel, headless (specs/browser.md §4, §7, §8).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::browser::import::{ImportSource, ImportStatus};
use helm::browser::library::{Favorite, Library, Visit};
use helm::browser::{Navigation, PageState};
use helm::theme::Palette;
use helm::ui::browser_panel::{
    browser_panel, BrowserChrome, BrowserIntent, BrowserPanelView, BrowserTabState, TabStrip,
    TabSummary,
};

/// What the pane reported on its last frame.
#[derive(Default)]
struct Shown {
    intents: Vec<BrowserIntent>,
    page_rect: Option<egui::Rect>,
    overlaid: bool,
    /// A frame reported a press in the toolbar.
    toolbar_pressed: bool,
}

type Seen = Rc<RefCell<Shown>>;

const PROJECT: &str = "/dev/helm";

struct Fixture {
    page: Rc<RefCell<PageState>>,
    library: Library,
    palette: Palette,
    /// The other tabs, after the shown one.
    others: Vec<TabSummary>,
    width: f32,
}

impl Fixture {
    fn new(page: PageState, library: Library) -> Self {
        Self {
            page: Rc::new(RefCell::new(page)),
            library,
            palette: Palette::dark(),
            others: Vec::new(),
            width: 800.0,
        }
    }
}

fn mount(fixture: Fixture) -> (Harness<'static, ()>, Seen) {
    let seen = Seen::default();
    let sink = seen.clone();
    let Fixture {
        page,
        library,
        palette,
        others,
        width,
    } = fixture;
    let mut tab = BrowserTabState::default();
    let import = ImportStatus::Idle;
    let icons = HashMap::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, 500.0))
        .build_ui(move |ui| {
            let chrome = BrowserChrome {
                palette: &palette,
                server: None,
                project: PROJECT,
                library: &library,
                import: &import,
                icons: &icons,
            };
            let page = page.borrow();
            let strip = TabStrip {
                tabs: std::iter::once(tab_summary(&page.title, &page.url))
                    .chain(others.iter().cloned())
                    .collect(),
                active: 0,
            };
            let view = BrowserPanelView {
                page: &page,
                tab: &mut tab,
                strip: &strip,
            };
            let out = browser_panel(ui, &chrome, view);
            let mut seen = sink.borrow_mut();
            seen.intents.extend(out.intents);
            seen.page_rect = out.page_rect;
            seen.overlaid = out.page_overlaid;
            seen.toolbar_pressed |= out.clicked;
        });
    harness.run();
    (harness, seen)
}

fn pane(page: PageState, library: Library) -> (Harness<'static, ()>, Seen) {
    mount(Fixture::new(page, library))
}

fn tab_summary(title: &str, url: &str) -> TabSummary {
    TabSummary {
        title: title.to_owned(),
        url: url.to_owned(),
        loading: false,
    }
}

/// The shown page, then Docs, Jira, Vite and a pull request.
fn five_tabs(width: f32) -> Fixture {
    Fixture {
        others: vec![
            tab_summary("Docs", "https://docs.rs/"),
            tab_summary("HELM-142", "https://jira.dev/browse/HELM-142"),
            tab_summary("Vite + React", "http://localhost:5173/"),
            tab_summary("Pull request #88", "https://github.com/helm/pull/88"),
        ],
        width,
        ..Fixture::new(page_at("https://kiln.studio/pricing"), Library::default())
    }
}

fn page_at(url: &str) -> PageState {
    PageState {
        url: url.to_owned(),
        title: "Admin".to_owned(),
        ..PageState::default()
    }
}

fn library_with_docs() -> Library {
    let mut library = Library::default();
    library.add_favorites(vec![Favorite {
        title: "Docs".to_owned(),
        url: "https://docs.rs/".to_owned(),
        ..Favorite::default()
    }]);
    library.merge_history(vec![Visit {
        url: "https://doc.rust-lang.org/std/".to_owned(),
        title: "std".to_owned(),
        count: 3,
        last: 1,
    }]);
    library
}

/// The address pill's field: the first text input, before the start page's filter.
fn type_in_address(harness: &mut Harness<'static, ()>, text: &str) {
    let address = || {
        harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .next()
    };
    address().expect("address field").click();
    harness.run();
    let address = || {
        harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .next()
    };
    address().expect("address field").type_text(text);
    harness.run();
}

#[test]
fn enter_in_the_url_field_loads_what_was_typed() {
    let (mut harness, seen) = pane(PageState::default(), Library::default());
    type_in_address(&mut harness, "localhost:5173");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(seen
        .borrow()
        .intents
        .contains(&BrowserIntent::Load("localhost:5173".into())));
}

#[test]
fn back_shows_only_when_there_is_history() {
    let (harness, _) = pane(page_at("http://localhost:5173/"), Library::default());
    assert!(harness.query_by_label("Back").is_none());

    let history = PageState {
        can_go_back: true,
        ..page_at("http://localhost:5173/b")
    };
    let (mut harness, seen) = pane(history, Library::default());
    harness.get_by_label("Back").click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::Navigate(Navigation::Back)]
    );
}

#[test]
fn reload_sits_in_the_pill() {
    let (mut harness, seen) = pane(page_at("http://localhost:3000/admin"), Library::default());
    harness.get_by_label("Reload (⇧ ignores the cache)").click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::Navigate(Navigation::Reload)]
    );
}

fn press_and_release(harness: &mut Harness<'static, ()>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
        harness.run();
    }
}

#[test]
fn a_press_on_the_url_field_keeps_the_keyboard_off_the_page() {
    let (mut harness, seen) = pane(page_at("http://localhost:3000/admin"), Library::default());
    let address = harness
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .next()
        .expect("address field")
        .rect()
        .center();
    press_and_release(&mut harness, address);
    assert!(!seen.borrow().toolbar_pressed);
}

#[test]
fn a_press_on_reload_gives_the_page_the_keyboard() {
    let (mut harness, seen) = pane(page_at("http://localhost:3000/admin"), Library::default());
    let reload = harness
        .get_by_label("Reload (⇧ ignores the cache)")
        .rect()
        .center();
    press_and_release(&mut harness, reload);
    assert!(seen.borrow().toolbar_pressed);
}

#[test]
fn the_star_toggles_the_page_as_a_favorite() {
    let (mut harness, seen) = pane(page_at("http://localhost:3000/admin"), Library::default());
    harness.get_by_label("Add to favorites").click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::ToggleFavorite {
            url: "http://localhost:3000/admin".into(),
            title: "Admin".into(),
        }]
    );
}

#[test]
fn a_blank_pane_shows_the_favorites_and_the_import() {
    let (mut harness, seen) = pane(PageState::default(), library_with_docs());
    harness.get_by_label("Docs").click();
    harness.run();
    harness.get_by_label("Import from Safari").click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![
            BrowserIntent::Open("https://docs.rs/".into()),
            BrowserIntent::Import(ImportSource::Safari),
        ]
    );
}

#[test]
fn arrow_down_then_enter_opens_the_picked_suggestion() {
    let (mut harness, seen) = pane(PageState::default(), library_with_docs());
    type_in_address(&mut harness, "doc");
    assert!(harness
        .query_by_label("https://doc.rust-lang.org/std/")
        .is_some());
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::Open("https://doc.rust-lang.org/std/".into())]
    );
}

#[test]
fn clicking_a_suggestion_opens_it() {
    let (mut harness, seen) = pane(PageState::default(), library_with_docs());
    type_in_address(&mut harness, "doc");
    harness
        .get_by_label("https://doc.rust-lang.org/std/")
        .click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::Open("https://doc.rust-lang.org/std/".into())]
    );
}

#[test]
fn imported_favorites_sit_under_their_folder() {
    let mut library = Library::default();
    library.add_favorites(vec![Favorite {
        title: "Jira".to_owned(),
        url: "https://jira.dev/".to_owned(),
        folder: Some("Bookmarks Bar › Work".to_owned()),
        ..Favorite::default()
    }]);
    let (mut harness, seen) = pane(PageState::default(), library);
    harness.get_by_label("Bookmarks Bar");
    harness.get_by_label("Work").click();
    harness.run();
    harness.get_by_label("Jira").click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::Open("https://jira.dev/".into())]
    );
}

#[test]
fn scrolling_down_folds_the_toolbar_to_the_host_and_a_click_unfolds_it() {
    let fixture = Fixture::new(page_at("https://kiln.studio/pricing"), Library::default());
    let page = fixture.page.clone();
    let (mut harness, _) = mount(fixture);
    page.borrow_mut().scroll_y = 240.0;
    harness.run();
    assert!(harness.query_by_label("Favorites").is_none());

    harness.get_by_label("kiln.studio").click();
    harness.run();
    assert!(harness.query_by_label("Favorites").is_some());
}

#[test]
fn suggestions_fall_over_the_page_without_taking_its_place() {
    let fixture = Fixture {
        palette: Palette::light(),
        ..Fixture::new(page_at("https://kiln.studio/pricing"), library_with_docs())
    };
    let (mut harness, seen) = mount(fixture);
    type_in_address(&mut harness, "doc");
    harness.get_by_label("https://docs.rs/");
    let seen = seen.borrow();
    assert!(seen.page_rect.is_some() && seen.overlaid);
}

#[test]
fn a_click_on_a_tab_shows_it() {
    let (mut harness, seen) = mount(five_tabs(1280.0));
    harness.get_by_label("HELM-142").click();
    harness.run();
    assert_eq!(seen.borrow().intents, vec![BrowserIntent::SelectTab(2)]);
}

#[test]
fn a_tab_closes_from_its_cross() {
    let (mut harness, seen) = mount(five_tabs(1280.0));
    harness.get_by_label("Docs").hover();
    harness.run();
    harness.get_by_label("Close Docs").click();
    harness.run();
    assert_eq!(seen.borrow().intents, vec![BrowserIntent::CloseTab(1)]);
}

#[test]
fn a_quarter_pane_still_reaches_every_tab() {
    let (mut harness, seen) = mount(five_tabs(400.0));
    harness.get_by_label("Pull request #88").click();
    harness.run();
    assert_eq!(seen.borrow().intents, vec![BrowserIntent::SelectTab(4)]);
}

#[test]
fn the_plus_opens_a_tab() {
    let (mut harness, seen) = pane(page_at("https://kiln.studio/pricing"), Library::default());
    harness.get_by_label("New tab").click();
    harness.run();
    assert_eq!(seen.borrow().intents, vec![BrowserIntent::NewTab]);
}

#[test]
fn with_several_tabs_new_tab_ends_the_tab_row() {
    let (mut harness, seen) = mount(five_tabs(1280.0));
    harness.get_by_label("New tab").click();
    harness.run();
    assert_eq!(seen.borrow().intents, vec![BrowserIntent::NewTab]);
}

fn open_find_bar(harness: &mut Harness<'static, ()>) {
    harness.get_by_label("More").click();
    harness.run();
    harness.get_by_label("Find in Page ⌘F").click();
    harness.run();
}

fn find(text: &str, backwards: bool) -> BrowserIntent {
    BrowserIntent::Find {
        text: text.to_owned(),
        backwards,
    }
}

#[test]
fn the_find_bar_finds_as_typed_then_next_and_previous() {
    let (mut harness, seen) = pane(page_at("https://kiln.studio/pricing"), Library::default());
    open_find_bar(&mut harness);
    harness
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .nth(1)
        .expect("find field after the address")
        .type_text("plan");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![find("plan", false), find("plan", false), find("plan", true)]
    );
}

#[test]
fn esc_closes_the_find_bar_and_gives_the_page_the_keyboard() {
    let (mut harness, seen) = pane(page_at("https://kiln.studio/pricing"), Library::default());
    open_find_bar(&mut harness);
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert_eq!(seen.borrow().intents, vec![BrowserIntent::FocusPage]);
    assert!(harness.query_by_label("Close find bar").is_none());
}

#[test]
fn a_mobile_viewport_centers_a_390_pt_page() {
    let (mut harness, seen) = pane(page_at("https://kiln.studio/pricing"), Library::default());
    harness.get_by_label("More").click();
    harness.run();
    harness.get_by_label("Mobile").click();
    harness.run();
    let page = seen.borrow().page_rect.expect("page shown");
    assert_eq!(page.width(), 390.0);
    assert_eq!(page.center().x, 400.0);
}

fn library_with_work_folder() -> Library {
    let mut library = library_with_docs();
    library.add_favorites(vec![Favorite {
        title: "Jira docs".to_owned(),
        url: "https://jira.dev/docs".to_owned(),
        folder: Some("Bookmarks Bar › Work".to_owned()),
        source: Some(ImportSource::Chrome),
        ..Favorite::default()
    }]);
    library
}

#[test]
fn the_filter_finds_favorites_of_every_section() {
    let (mut harness, seen) = pane(PageState::default(), library_with_work_folder());
    let filter = || {
        harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .nth(1)
    };
    filter().expect("filter field").click();
    harness.run();
    let filter = || {
        harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .nth(1)
    };
    filter().expect("filter field").type_text("DOCS");
    harness.run();
    harness.get_by_label("Docs");
    harness.get_by_label("Jira docs").click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::Open("https://jira.dev/docs".into())]
    );
}

#[test]
fn a_cmd_click_opens_the_tile_in_a_new_tab() {
    let (mut harness, seen) = pane(PageState::default(), library_with_docs());
    harness
        .get_by_label("Docs")
        .click_modifiers(egui::Modifiers::COMMAND);
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::OpenInNewTab("https://docs.rs/".into())]
    );
}

#[test]
fn rename_from_the_tile_menu_reports_the_new_title() {
    let (mut harness, seen) = pane(PageState::default(), library_with_docs());
    harness.get_by_label("Docs").click_secondary();
    harness.run();
    harness.get_by_label("Rename…").click();
    harness.run();
    harness.run();
    harness
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|field| field.value().as_deref() == Some("Docs"))
        .expect("rename field")
        .type_text("Rust docs");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::RenameFavorite {
            url: "https://docs.rs/".into(),
            title: "Rust docs".into(),
        }]
    );
}

#[test]
fn a_narrow_pane_picks_its_section_from_a_menu() {
    let (mut harness, seen) = mount(Fixture {
        width: 460.0,
        ..Fixture::new(PageState::default(), library_with_work_folder())
    });
    assert!(harness.query_by_label("Frequently visited").is_none());
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Starred")
        .click();
    harness.run();
    harness.get_by_label("Work").click();
    harness.run();
    harness.get_by_label("Jira docs").click();
    harness.run();
    assert_eq!(
        seen.borrow().intents,
        vec![BrowserIntent::Open("https://jira.dev/docs".into())]
    );
}
