use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use helm::files::file_type::FileType;
use helm::files::tab::{SidebarTab, TabEdit, TabState};
use helm::files::tint::StatusTints;
use helm::files::tree::{EntryKind, FolderListing, Listings, TreeEntry};
use helm::git::status::{ChangeKind, FileEntry, RepoStatus};
use helm::theme::Palette;
use helm::ui::file_tree::{file_tree, FileTreeState};
use helm::ui::git_panel::GitIntent;

/// The tree as the app drives it: rows projected from the listings and the tab
/// state each frame, the frame's intents applied back to the tab state.
struct World {
    listings: Listings,
    tints: StatusTints,
    tab: TabState,
    tree: FileTreeState,
    intents: Vec<GitIntent>,
}

impl World {
    fn apply(&mut self, intent: &GitIntent) {
        let edit = match intent {
            GitIntent::SelectEntry(path) => TabEdit::Select(path.clone()),
            GitIntent::FoldFolder(folder) => TabEdit::Fold(folder.clone()),
            GitIntent::UnfoldFolder(folder) => TabEdit::Unfold(folder.clone()),
            _ => return,
        };
        self.tab.apply(edit);
    }
}

fn folder(path: &str, entries: &[(&str, EntryKind, bool)]) -> (String, FolderListing) {
    let entries: Vec<TreeEntry> = entries
        .iter()
        .map(|(name, kind, ignored)| TreeEntry {
            name: (*name).to_owned(),
            kind: *kind,
            ignored: *ignored,
        })
        .collect();
    let total = entries.len();
    (path.to_owned(), FolderListing { entries, total })
}

/// `src/` (`lib.rs`, `main.rs` modified), `target/` ignored (`debug/`), `link`,
/// `README.md` untracked.
fn sample_world(unfolded: &[&str]) -> World {
    let mut listings = Listings::default();
    listings.store(vec![
        folder(
            "",
            &[
                ("src", EntryKind::Folder, false),
                ("target", EntryKind::Folder, true),
                ("link", EntryKind::Symlink, false),
                ("README.md", EntryKind::File, false),
            ],
        ),
        folder(
            "src",
            &[
                ("lib.rs", EntryKind::File, false),
                ("main.rs", EntryKind::File, false),
            ],
        ),
        folder("target", &[("debug", EntryKind::Folder, true)]),
    ]);
    let change = |path: &str, kind| FileEntry {
        path: path.to_owned(),
        kind,
        additions: 0,
        deletions: 0,
    };
    let status = RepoStatus {
        staged: Vec::new(),
        unstaged: vec![
            change("src/main.rs", ChangeKind::Modified),
            change("README.md", ChangeKind::Untracked),
        ],
    };
    World {
        listings,
        tints: StatusTints::of(&status),
        tab: TabState {
            tab: SidebarTab::Files,
            unfolded: unfolded.iter().map(|folder| (*folder).to_owned()).collect(),
            selected: None,
        },
        tree: FileTreeState::default(),
        intents: Vec::new(),
    }
}

fn tree_harness(world: World) -> Harness<'static, World> {
    let palette = Palette::dark();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 400.0))
        .build_ui_state(
            move |ui, world: &mut World| {
                world.tree.rows = world.listings.rows(&world.tab.unfolded, &world.tints);
                world.tree.selected = world.tab.selected.clone();
                let intents = file_tree(ui, &palette, &mut world.tree);
                for intent in &intents {
                    world.apply(intent);
                }
                world.intents.extend(intents);
            },
            world,
        );
    harness.run();
    harness
}

fn selected(harness: &Harness<'_, World>) -> Option<String> {
    harness.state().tab.selected.clone()
}

fn text_color(harness: &Harness<'_, World>, text: &str) -> egui::Color32 {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.job.text == text => {
                Some(shape.galley.job.sections[0].format.color)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {text:?} painted"))
}

fn dot_colors(harness: &Harness<'_, World>) -> Vec<egui::Color32> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Circle(circle) => Some(circle.fill),
            _ => None,
        })
        .collect()
}

/// Ink of the icon painted as `glyph` — a Nerd Font glyph paints in its fallback color.
fn glyph_color(harness: &Harness<'_, World>, glyph: char) -> egui::Color32 {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.job.text == glyph.to_string() => {
                Some(shape.fallback_color)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no glyph U+{:04X} painted", glyph as u32))
}

#[test]
fn a_known_file_type_shows_its_glyph_in_its_color_muted_when_ignored() {
    let palette = Palette::dark();
    let mut world = sample_world(&[]);
    world.listings.store(vec![folder(
        "",
        &[
            ("Cargo.toml", EntryKind::File, false),
            ("build.log", EntryKind::File, true),
            ("notes.xyz", EntryKind::File, false),
        ],
    )]);
    let harness = tree_harness(world);

    assert_eq!(
        glyph_color(&harness, FileType::Cargo.glyph()),
        palette.file_type_color(FileType::Cargo)
    );
    assert_eq!(
        glyph_color(&harness, FileType::Log.glyph()),
        palette.text_muted
    );
    assert_eq!(
        text_color(&harness, &lucide_icons::Icon::File.unicode().to_string()),
        palette.text_secondary,
        "an unknown type keeps the plain file icon"
    );
}

#[test]
fn a_folder_click_unfolds_it_and_shows_its_children() {
    let mut harness = tree_harness(sample_world(&[]));
    assert!(harness.query_by_label("src/main.rs").is_none());

    harness.get_by_label("src").click();
    harness.run();

    assert_eq!(
        harness.state().intents,
        [
            GitIntent::SelectEntry("src".to_owned()),
            GitIntent::UnfoldFolder("src".to_owned())
        ]
    );
    harness.get_by_label("src/main.rs");
    harness.get_by_label("src/lib.rs");
}

#[test]
fn a_file_click_selects_it_and_asks_the_viewer_to_open_it() {
    let mut harness = tree_harness(sample_world(&["src"]));

    harness.get_by_label("src/main.rs").click();
    harness.run();

    assert_eq!(
        harness.state().intents,
        [
            GitIntent::SelectEntry("src/main.rs".to_owned()),
            GitIntent::OpenFile("src/main.rs".to_owned())
        ]
    );
    assert_eq!(
        format!(
            "{:?}",
            harness
                .get_by_label("src/main.rs")
                .accesskit_node()
                .toggled()
        ),
        "Some(True)"
    );
}

#[test]
fn ignored_entries_are_muted_and_changes_take_their_git_color() {
    let palette = Palette::dark();
    let harness = tree_harness(sample_world(&["src", "target"]));

    assert_eq!(text_color(&harness, "target"), palette.text_muted);
    assert_eq!(text_color(&harness, "debug"), palette.text_muted);
    assert_eq!(text_color(&harness, "lib.rs"), palette.text_primary);
    assert_eq!(text_color(&harness, "main.rs"), palette.git_modified);
    assert_eq!(text_color(&harness, "README.md"), palette.git_added);
    assert_eq!(
        text_color(&harness, "src"),
        palette.text_primary,
        "a folder shows its change as a dot, not a color"
    );
    assert_eq!(dot_colors(&harness), [palette.git_modified]);
}

#[test]
fn the_arrows_walk_fold_and_unfold_once_a_row_was_clicked() {
    let mut harness = tree_harness(sample_world(&[]));
    harness.get_by_label("src").click();
    harness.run();
    let mut press = |key| {
        harness.key_press(key);
        harness.run();
        selected(&harness)
    };

    assert_eq!(press(egui::Key::ArrowDown).as_deref(), Some("src/lib.rs"));
    assert_eq!(press(egui::Key::ArrowDown).as_deref(), Some("src/main.rs"));
    assert_eq!(press(egui::Key::ArrowLeft).as_deref(), Some("src"));
    assert_eq!(press(egui::Key::ArrowLeft).as_deref(), Some("src"));
    assert!(harness.query_by_label("src/lib.rs").is_none(), "← folds");
    harness.key_press(egui::Key::ArrowRight);
    harness.run();
    harness.get_by_label("src/lib.rs");
    harness.key_press(egui::Key::ArrowRight);
    harness.run();
    assert_eq!(selected(&harness).as_deref(), Some("src/lib.rs"));
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert_eq!(selected(&harness).as_deref(), Some("src"));
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(
        harness.query_by_label("src/lib.rs").is_none(),
        "Enter folds"
    );

    let opened: Vec<&GitIntent> = harness
        .state()
        .intents
        .iter()
        .filter(|intent| matches!(intent, GitIntent::OpenFile(_)))
        .collect();
    assert_eq!(
        opened,
        [
            &GitIntent::OpenFile("src/lib.rs".to_owned()),
            &GitIntent::OpenFile("src/main.rs".to_owned())
        ],
        "↓ onto a file opens it, → onto one only selects it"
    );
}

#[test]
fn the_arrows_do_nothing_until_a_row_is_clicked() {
    let mut world = sample_world(&[]);
    world.tab.selected = Some("src".to_owned());
    let mut harness = tree_harness(world);

    harness.key_press(egui::Key::ArrowDown);
    harness.key_press(egui::Key::ArrowRight);
    harness.run();

    assert!(harness.state().intents.is_empty());
}

#[test]
fn an_empty_worktree_says_so_and_a_capped_folder_counts_what_it_leaves_out() {
    let mut empty = sample_world(&[]);
    empty.listings = Listings::default();
    empty.listings.store(vec![folder("", &[])]);
    tree_harness(empty).get_by_label("No files");

    let mut capped = sample_world(&[]);
    let (root, mut listing) = folder("", &[("a.txt", EntryKind::File, false)]);
    listing.total = 2_400;
    capped.listings.store(vec![(root, listing)]);
    tree_harness(capped).get_by_label("2399 more not shown");
}

#[test]
fn nothing_shows_until_the_root_is_read() {
    let mut world = sample_world(&[]);
    world.listings = Listings::default();
    let harness = tree_harness(world);

    assert!(harness.query_by_label("No files").is_none());
}
