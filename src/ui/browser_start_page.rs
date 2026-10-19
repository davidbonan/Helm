//! Start page of a browser pane (specs/browser.md §7, §9): a sidebar of sections
//! — This project, Starred, Frequently visited, each import source's folders —
//! and the shown section's tiles; a section picker instead of the sidebar in a
//! narrow pane.

use std::sync::Arc;

use lucide_icons::Icon;

use crate::browser::import::{ImportError, ImportSource, ImportStatus, FOLDER_SEPARATOR};
use crate::browser::sections::{Entry, FolderRow, Section};
use crate::browser::url;
use crate::theme::Palette;
use crate::ui::browser_pane::{BrowserChrome, BrowserIntent};
use crate::ui::with_alpha;

const SIDEBAR_WIDTH: f32 = 220.0;
const SIDEBAR_PAD: f32 = 10.0;
const SIDEBAR_FOOT_HEIGHT: f32 = 58.0;
/// Under this page-area width the sidebar folds into a section picker.
const NARROW_BELOW: f32 = 560.0;
const ROW_HEIGHT: f32 = 28.0;
const ROW_RADIUS: u8 = 6;
const ROW_ICON_SIZE: f32 = 14.0;
const ROW_INDENT: f32 = 16.0;
const FIELD_HEIGHT: f32 = 28.0;
const CONTENT_PAD_X: f32 = 24.0;
const CONTENT_PAD_TOP: f32 = 18.0;
const TILE_MIN_WIDTH: f32 = 180.0;
const TILE_HEIGHT: f32 = 64.0;
const TILE_GAP: f32 = 12.0;
const TILE_RADIUS: u8 = 10;
const TILE_PAD: f32 = 12.0;
const BADGE_SIZE: f32 = 32.0;
const BADGE_RADIUS: u8 = 8;
const BADGE_INITIAL_SCALE: f32 = 15.0 / 32.0;
/// Room around a site icon in its tile badge, per side, of the badge size.
const SITE_ICON_INSET: f32 = 0.18;
const TILE_TITLE_SIZE: f32 = 13.0;
const TILE_LOCATION_SIZE: f32 = 11.0;
const TITLE_SIZE: f32 = 18.0;
const BANNER_RADIUS: u8 = 10;
const DISMISS_SIZE: f32 = 24.0;
const FILTER_HINT: &str = "Filter favorites";
const DEV_SERVER: &str = "Dev server";

/// What the start page keeps per tab, besides the library (browser.md §7).
#[derive(Debug, Default)]
pub struct FavoritesView {
    /// The section shown; `None` until the page first picks one.
    pub section: Option<Section>,
    pub filter: String,
    pub renaming: Option<Renaming>,
}

/// A favorite whose title is edited in place.
#[derive(Debug)]
pub struct Renaming {
    pub url: String,
    pub text: String,
    started: bool,
}

/// One tile: a favorite, a history page or the Dev server.
struct Tile<'a> {
    title: &'a str,
    url: &'a str,
    kind: TileKind,
}

#[derive(Clone, Copy, PartialEq)]
enum TileKind {
    Server,
    Favorite,
    Visit,
}

impl<'a> Tile<'a> {
    fn of(entry: &Entry<'a>) -> Self {
        Self {
            title: entry.title,
            url: entry.url,
            kind: if entry.favorite {
                TileKind::Favorite
            } else {
                TileKind::Visit
            },
        }
    }

    /// Its title, else its host.
    fn name(&self) -> &'a str {
        if self.title.trim().is_empty() {
            url::display_parts(self.url).0
        } else {
            self.title
        }
    }
}

/// A sidebar row: a section with its icon, name and count.
struct SectionRow {
    section: Section,
    icon: Icon,
    name: String,
    count: usize,
    depth: usize,
}

/// The sidebar's content: the library's sections, then each import source's
/// folders under its heading.
struct Sidebar {
    library: Vec<SectionRow>,
    groups: Vec<(String, Vec<SectionRow>)>,
}

impl Sidebar {
    fn of(chrome: &BrowserChrome<'_>) -> Self {
        let library = chrome.library;
        let count = |section: &Section| library.section(section, chrome.project).len();
        let server = usize::from(chrome.server.is_some());
        let row = |section: Section, icon, name: &str, extra: usize| SectionRow {
            count: count(&section) + extra,
            section,
            icon,
            name: name.to_owned(),
            depth: 0,
        };
        let groups = library
            .imported_groups()
            .into_iter()
            .map(|group| {
                let rows = group.rows.into_iter().map(folder_row).collect();
                (source_heading(group.source), rows)
            })
            .collect();
        Self {
            library: vec![
                row(
                    Section::Project,
                    Icon::SquareTerminal,
                    "This project",
                    server,
                ),
                row(Section::Starred, Icon::Star, "Starred", 0),
                row(Section::Frequent, Icon::Clock, "Frequently visited", 0),
            ],
            groups,
        }
    }

    fn rows(&self) -> impl Iterator<Item = &SectionRow> {
        self.library
            .iter()
            .chain(self.groups.iter().flat_map(|(_, rows)| rows))
    }
}

fn folder_row(row: FolderRow) -> SectionRow {
    SectionRow {
        name: row.name.unwrap_or_else(|| "Unsorted".to_owned()),
        section: row.section,
        icon: Icon::Folder,
        count: row.count,
        depth: row.depth,
    }
}

fn source_heading(source: Option<ImportSource>) -> String {
    match source {
        Some(source) => format!("From {}", source.name()),
        None => "Imported".to_owned(),
    }
}

/// The start page over `area`; what the user asked of it.
pub(crate) fn start_page(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: &mut FavoritesView,
    area: egui::Rect,
) -> Vec<BrowserIntent> {
    let sidebar = Sidebar::of(chrome);
    let section = view
        .section
        .get_or_insert_with(|| first_section(&sidebar))
        .clone();
    let mut intents = Vec::new();
    let narrow = area.width() < NARROW_BELOW;
    let content = if narrow {
        area
    } else {
        let (side, content) = area.split_left_right_at_x(area.left() + SIDEBAR_WIDTH);
        sidebar_panel(ui, chrome, view, &sidebar, side, &mut intents);
        content
    };
    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("start_page"))
        .auto_shrink([false, false])
        .show(&mut content_ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: CONTENT_PAD_X as i8,
                    right: CONTENT_PAD_X as i8,
                    top: CONTENT_PAD_TOP as i8,
                    bottom: CONTENT_PAD_X as i8,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if narrow {
                        section_picker(ui, chrome.palette, view, &sidebar);
                        ui.add_space(12.0);
                    }
                    if let Some(intent) = import_banner(ui, chrome.palette, chrome.import) {
                        intents.push(intent);
                    }
                    section_view(ui, chrome, view, &section, &mut intents);
                });
        });
    intents
}

/// This project when it has a tile, else Starred.
fn first_section(sidebar: &Sidebar) -> Section {
    let project = sidebar
        .library
        .iter()
        .any(|row| row.section == Section::Project && row.count > 0);
    if project {
        Section::Project
    } else {
        Section::Starred
    }
}

fn sidebar_panel(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: &mut FavoritesView,
    sidebar: &Sidebar,
    rect: egui::Rect,
    intents: &mut Vec<BrowserIntent>,
) {
    let palette = chrome.palette;
    ui.painter().rect_filled(rect, 0.0, palette.bg_sidebar);
    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    let inner = rect.shrink(SIDEBAR_PAD);
    let (rows_rect, foot) = inner.split_top_bottom_at_y(inner.bottom() - SIDEBAR_FOOT_HEIGHT);
    let mut rows_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rows_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    filter_field(&mut rows_ui, palette, &mut view.filter, rows_rect.width());
    rows_ui.add_space(10.0);
    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("favorites_sidebar"))
        .auto_shrink([false, false])
        .show(&mut rows_ui, |ui| {
            section_rows(ui, palette, view, sidebar);
        });
    ui.painter().hline(
        foot.x_range(),
        foot.top() + 4.0,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    let mut foot_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(foot.shrink2(egui::vec2(0.0, 10.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    foot_ui.spacing_mut().item_spacing.y = 2.0;
    import_links(&mut foot_ui, palette, chrome.import, intents);
}

/// The library's rows, then each source's heading and folders; a click shows
/// its section and clears the filter.
fn section_rows(ui: &mut egui::Ui, palette: &Palette, view: &mut FavoritesView, sidebar: &Sidebar) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let shown = view
        .filter
        .trim()
        .is_empty()
        .then_some(view.section.as_ref())
        .flatten();
    let mut picked = None;
    for row in &sidebar.library {
        if section_row(ui, palette, row, shown == Some(&row.section)).clicked() {
            picked = Some(row.section.clone());
        }
    }
    for (heading, rows) in &sidebar.groups {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(crate::ui::section_label(palette, heading));
        });
        ui.add_space(4.0);
        for row in rows {
            if section_row(ui, palette, row, shown == Some(&row.section)).clicked() {
                picked = Some(row.section.clone());
            }
        }
    }
    if let Some(section) = picked {
        view.section = Some(section);
        view.filter.clear();
    }
}

fn section_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &SectionRow,
    shown: bool,
) -> egui::Response {
    let size = egui::vec2(ui.available_width(), ROW_HEIGHT);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, true);
    let radius = egui::CornerRadius::same(ROW_RADIUS);
    if shown {
        let shadow = egui::Shadow {
            offset: [0, 1],
            blur: 3,
            spread: 0,
            color: with_alpha(egui::Color32::BLACK, 24),
        };
        ui.painter().add(shadow.as_shape(rect, radius));
        ui.painter().rect_filled(rect, radius, palette.bg_canvas);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, radius, with_alpha(palette.text_primary, 14));
    }
    let icon_x = rect.left() + 8.0 + row.depth as f32 * ROW_INDENT + ROW_ICON_SIZE / 2.0;
    let icon_tint = if shown {
        palette.accent
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(icon_x, rect.center().y),
        ROW_ICON_SIZE,
        row.icon,
        icon_tint,
    );
    let count = ui.painter().layout_no_wrap(
        row.count.to_string(),
        egui::FontId::proportional(11.0),
        palette.text_muted,
    );
    let count_left = rect.right() - 8.0 - count.size().x;
    let text_left = icon_x + ROW_ICON_SIZE / 2.0 + 8.0;
    let name = elided(
        ui.painter(),
        &row.name,
        13.0,
        palette.text_primary,
        (count_left - 8.0 - text_left).max(0.0),
    );
    paint_left_centered(ui.painter(), name, egui::pos2(text_left, rect.center().y));
    paint_left_centered(ui.painter(), count, egui::pos2(count_left, rect.center().y));
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, shown, &row.name)
    });
    response
}

fn filter_field(ui: &mut egui::Ui, palette: &Palette, filter: &mut String, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, FIELD_HEIGHT), egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(7),
        palette.bg_canvas,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let icon_center = egui::pos2(rect.left() + 9.0 + 6.5, rect.center().y);
    crate::ui::paint_icon(
        ui.painter(),
        icon_center,
        13.0,
        Icon::Search,
        palette.text_muted,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(icon_center.x + 12.0, rect.top()),
        egui::pos2(rect.right() - 8.0, rect.bottom()),
    );
    let mut field_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    egui::TextEdit::singleline(filter)
        .id(ui.id().with("favorites_filter"))
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .hint_text(FILTER_HINT)
        .font(egui::FontId::proportional(12.0))
        .text_color(palette.text_primary)
        .desired_width(f32::INFINITY)
        .show(&mut field_ui);
}

/// The narrow pane's header: a picker of the sidebar's sections, and the filter.
fn section_picker(
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &mut FavoritesView,
    sidebar: &Sidebar,
) {
    let width = ui.available_width();
    let shown = view.section.as_ref();
    let label = sidebar
        .rows()
        .find(|row| Some(&row.section) == shown)
        .map_or("Favorites", |row| row.name.as_str());
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(width, FIELD_HEIGHT + 4.0), true);
    let fill = if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_canvas
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        fill,
        egui::Stroke::new(1.0_f32, palette.border_input),
        egui::StrokeKind::Inside,
    );
    let text = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(13.0),
        palette.text_primary,
    );
    paint_left_centered(
        ui.painter(),
        text,
        egui::pos2(rect.left() + 10.0, rect.center().y),
    );
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(rect.right() - 16.0, rect.center().y),
        14.0,
        Icon::ChevronDown,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    egui::Popup::menu(&response)
        .style(crate::theme::menu_style)
        .width(width.min(280.0))
        .show(|ui| {
            section_rows(ui, palette, view, sidebar);
        });
    ui.add_space(8.0);
    filter_field(ui, palette, &mut view.filter, width);
}

/// The section's path, name and count, then its tiles; *Results* while the
/// filter holds text.
fn section_view(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: &mut FavoritesView,
    section: &Section,
    intents: &mut Vec<BrowserIntent>,
) {
    let palette = chrome.palette;
    let filter = view.filter.trim().to_owned();
    let (path, title, entries) = if filter.is_empty() {
        let (path, title) = section_heading(chrome, section);
        (path, title, chrome.library.section(section, chrome.project))
    } else {
        let results = chrome.library.search(&filter, chrome.project);
        ("Every favorite".to_owned(), "Results".to_owned(), results)
    };
    let server = chrome
        .server
        .filter(|_| filter.is_empty() && *section == Section::Project)
        .map(|server| Tile {
            title: DEV_SERVER,
            url: server,
            kind: TileKind::Server,
        });
    let tiles: Vec<Tile<'_>> = server
        .into_iter()
        .chain(entries.iter().map(Tile::of))
        .collect();
    ui.label(
        egui::RichText::new(path)
            .size(11.0)
            .color(palette.text_muted),
    );
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            egui::RichText::new(&title)
                .size(TITLE_SIZE)
                .color(palette.text_primary),
        );
        ui.label(
            egui::RichText::new(tiles.len().to_string())
                .size(13.0)
                .color(palette.text_muted),
        );
    });
    ui.add_space(14.0);
    if tiles.is_empty() {
        let hint = if filter.is_empty() {
            empty_hint(section).to_owned()
        } else {
            format!("No favorite matches “{filter}”.")
        };
        ui.label(egui::RichText::new(hint).color(palette.text_muted));
        return;
    }
    tile_grid(ui, chrome, view, &tiles, intents);
}

/// The muted line above a section's name, and its name.
fn section_heading(chrome: &BrowserChrome<'_>, section: &Section) -> (String, String) {
    match section {
        Section::Project => {
            let name = std::path::Path::new(chrome.project)
                .file_name()
                .map_or_else(
                    || chrome.project.to_owned(),
                    |name| name.to_string_lossy().into_owned(),
                );
            (name, "This project".to_owned())
        }
        Section::Starred => ("Every project".to_owned(), "Starred".to_owned()),
        Section::Frequent => ("From history".to_owned(), "Frequently visited".to_owned()),
        Section::Folder { source, path } => {
            let heading = source_heading(*source);
            let Some(path) = path else {
                return (heading, "Unsorted".to_owned());
            };
            match path.rsplit_once(FOLDER_SEPARATOR) {
                Some((parent, name)) => (
                    format!("{heading}{FOLDER_SEPARATOR}{parent}"),
                    name.to_owned(),
                ),
                None => (heading, path.clone()),
            }
        }
    }
}

fn empty_hint(section: &Section) -> &'static str {
    match section {
        Section::Project => "Star a local page to keep it with this project.",
        Section::Starred => "No favorites yet. Star a page, or import them from your browser.",
        Section::Frequent => "The pages you visit most will show here.",
        Section::Folder { .. } => "This folder holds only folders.",
    }
}

/// Equal columns of at least `TILE_MIN_WIDTH`, filling the width.
fn tile_grid(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: &mut FavoritesView,
    tiles: &[Tile<'_>],
    intents: &mut Vec<BrowserIntent>,
) {
    let width = ui.available_width();
    let columns = ((width + TILE_GAP) / (TILE_MIN_WIDTH + TILE_GAP))
        .floor()
        .max(1.0);
    let tile_width = ((width - (columns - 1.0) * TILE_GAP) / columns).floor();
    for (at, row) in tiles.chunks(columns as usize).enumerate() {
        if at > 0 {
            ui.add_space(TILE_GAP);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = TILE_GAP;
            for tile in row {
                intents.extend(tile_widget(ui, chrome, view, tile, tile_width));
            }
        });
    }
}

/// One tile: a click opens it, `Cmd`-click or a middle click in a new tab; its
/// right-click menu; its title edited in place while renamed.
fn tile_widget(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: &mut FavoritesView,
    tile: &Tile<'_>,
    width: f32,
) -> Option<BrowserIntent> {
    let palette = chrome.palette;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, TILE_HEIGHT), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = ui.rect_contains_pointer(rect);
    let (fill, border) = if hovered {
        (palette.bg_surface_hover, palette.border_input)
    } else {
        (palette.bg_canvas, palette.border_subtle)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(TILE_RADIUS),
        fill,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );
    let badge = Badge {
        center: egui::pos2(rect.left() + TILE_PAD + BADGE_SIZE / 2.0, rect.center().y),
        size: BADGE_SIZE,
        site: url::display_parts(tile.url).0,
        name: tile.name(),
        server: tile.kind == TileKind::Server,
        icon: chrome.site_icon(tile.url),
    };
    paint_badge(painter, palette, &badge);
    let text_left = badge.center.x + BADGE_SIZE / 2.0 + 10.0;
    let text_width = rect.right() - TILE_PAD - text_left;
    let renamed = view
        .renaming
        .as_ref()
        .is_some_and(|renaming| renaming.url == tile.url);
    let name = tile.name();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if renamed {
        let field = egui::Rect::from_min_max(
            egui::pos2(text_left - 4.0, rect.top() + 8.0),
            egui::pos2(rect.right() - TILE_PAD, rect.bottom() - 8.0),
        );
        return rename_field(ui, palette, view, field);
    }
    let title = wrapped(painter, name, text_width);
    let location = elided(
        painter,
        &url::tile_location(tile.url),
        TILE_LOCATION_SIZE,
        palette.text_muted,
        text_width,
    );
    let top = rect.center().y - (title.size().y + 2.0 + location.size().y) / 2.0;
    let location_top = top + title.size().y + 2.0;
    painter.galley(egui::pos2(text_left, top), title, palette.text_primary);
    painter.galley(
        egui::pos2(text_left, location_top),
        location,
        palette.text_muted,
    );
    let menu = tile_menu(&response, view, tile);
    if menu.is_some() {
        return menu;
    }
    let new_tab =
        response.middle_clicked() || (response.clicked() && ui.input(|i| i.modifiers.command));
    if new_tab {
        return Some(BrowserIntent::OpenInNewTab(tile.url.to_owned()));
    }
    if !response.clicked() {
        return None;
    }
    Some(match tile.kind {
        TileKind::Server => BrowserIntent::LoadServer,
        TileKind::Favorite | TileKind::Visit => BrowserIntent::Open(tile.url.to_owned()),
    })
}

/// Open, Open in New Tab, Copy URL, and for a favorite Rename… and Remove.
fn tile_menu(
    response: &egui::Response,
    view: &mut FavoritesView,
    tile: &Tile<'_>,
) -> Option<BrowserIntent> {
    let mut intent = None;
    egui::Popup::context_menu(response)
        .style(crate::theme::menu_style)
        .show(|ui| {
            if ui.button("Open").clicked() {
                intent = Some(match tile.kind {
                    TileKind::Server => BrowserIntent::LoadServer,
                    TileKind::Favorite | TileKind::Visit => {
                        BrowserIntent::Open(tile.url.to_owned())
                    }
                });
                ui.close();
            }
            if ui.button("Open in New Tab").clicked() {
                intent = Some(BrowserIntent::OpenInNewTab(tile.url.to_owned()));
                ui.close();
            }
            if ui.button("Copy URL").clicked() {
                ui.ctx().copy_text(tile.url.to_owned());
                ui.close();
            }
            if tile.kind != TileKind::Favorite {
                return;
            }
            ui.separator();
            if ui.button("Rename…").clicked() {
                view.renaming = Some(Renaming {
                    url: tile.url.to_owned(),
                    text: tile.name().to_owned(),
                    started: false,
                });
                ui.close();
            }
            if ui.button("Remove from Favorites").clicked() {
                intent = Some(BrowserIntent::RemoveFavorite(tile.url.to_owned()));
                ui.close();
            }
        });
    intent
}

/// The renamed title: focused with its text selected; `Enter` keeps it, `Esc`
/// or clicking away drops it.
fn rename_field(
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &mut FavoritesView,
    field: egui::Rect,
) -> Option<BrowserIntent> {
    let renaming = view.renaming.as_mut()?;
    let mut field_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let output = egui::TextEdit::singleline(&mut renaming.text)
        .id(ui.id().with("rename_favorite"))
        .font(egui::FontId::proportional(TILE_TITLE_SIZE))
        .text_color(palette.text_primary)
        .desired_width(f32::INFINITY)
        .margin(egui::Margin::symmetric(4, 2))
        .show(&mut field_ui);
    if !std::mem::replace(&mut renaming.started, true) {
        output.response.request_focus();
        let mut state = output.state;
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(renaming.text.chars().count()),
            )));
        state.store(ui.ctx(), output.response.id);
        return None;
    }
    if !output.response.lost_focus() {
        return None;
    }
    let kept = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let renaming = view.renaming.take()?;
    kept.then_some(BrowserIntent::RenameFavorite {
        url: renaming.url,
        title: renaming.text,
    })
}

/// A badge standing for a page.
pub(crate) struct Badge<'a> {
    pub(crate) center: egui::Pos2,
    pub(crate) size: f32,
    /// The page's host and port: its hue.
    pub(crate) site: &'a str,
    /// Its title, else host: its initial.
    pub(crate) name: &'a str,
    /// The Dev server's.
    pub(crate) server: bool,
    /// The site's icon (browser.md §10), drawn instead of the initial.
    pub(crate) icon: Option<egui::TextureId>,
}

/// The name's initial on a hue drawn from the site; the Dev server's icon on
/// the accent.
pub(crate) fn paint_badge(painter: &egui::Painter, palette: &Palette, badge: &Badge<'_>) {
    let rect = egui::Rect::from_center_size(badge.center, egui::vec2(badge.size, badge.size));
    let radius =
        egui::CornerRadius::same((f32::from(BADGE_RADIUS) * badge.size / BADGE_SIZE) as u8);
    if badge.server {
        painter.rect_filled(rect, radius, palette.accent);
        crate::ui::paint_icon(
            painter,
            badge.center,
            badge.size / 2.0,
            Icon::Server,
            egui::Color32::WHITE,
        );
        return;
    }
    if let Some(icon) = badge.icon {
        paint_site_icon(painter, palette, badge, icon);
        return;
    }
    painter.rect_filled(rect, radius, site_hue(palette, badge.site));
    let initial = badge
        .name
        .trim_start_matches("www.")
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(String::new, |initial| initial.to_uppercase().to_string());
    painter.text(
        badge.center,
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(badge.size * BADGE_INITIAL_SCALE),
        egui::Color32::WHITE,
    );
}

/// A tile's icon sits on a light square, as the page shows it; a tab's is
/// drawn bare.
fn paint_site_icon(
    painter: &egui::Painter,
    palette: &Palette,
    badge: &Badge<'_>,
    icon: egui::TextureId,
) {
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let rect = egui::Rect::from_center_size(badge.center, egui::vec2(badge.size, badge.size));
    if badge.size < BADGE_SIZE {
        painter.image(icon, rect, uv, egui::Color32::WHITE);
        return;
    }
    let radius =
        egui::CornerRadius::same((f32::from(BADGE_RADIUS) * badge.size / BADGE_SIZE) as u8);
    painter.rect(
        rect,
        radius,
        egui::Color32::WHITE,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let glyph = rect.shrink(badge.size * SITE_ICON_INSET);
    painter.image(icon, glyph, uv, egui::Color32::WHITE);
}

fn site_hue(palette: &Palette, site: &str) -> egui::Color32 {
    let hash = site.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    palette.lane_colors[hash % palette.lane_colors.len()]
}

/// The title over at most two lines, elided after.
fn wrapped(painter: &egui::Painter, text: &str, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontId::proportional(TILE_TITLE_SIZE),
        egui::Color32::PLACEHOLDER,
        width,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

pub(crate) fn elided(
    painter: &egui::Painter,
    text: &str,
    size: f32,
    color: egui::Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        egui::FontId::proportional(size),
        color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    painter.layout_job(job)
}

fn paint_left_centered(
    painter: &egui::Painter,
    galley: Arc<egui::Galley>,
    left_center: egui::Pos2,
) {
    let at = egui::pos2(left_center.x, left_center.y - galley.size().y / 2.0);
    painter.galley(at, galley, egui::Color32::PLACEHOLDER);
}

/// The import under way or its outcome atop the section; `×` once it is over.
fn import_banner(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &ImportStatus,
) -> Option<BrowserIntent> {
    let line = import_status_line(status)?;
    let mut intent = None;
    egui::Frame::NONE
        .fill(palette.accent_subtle)
        .stroke(egui::Stroke::new(1.0_f32, with_alpha(palette.accent, 60)))
        .corner_radius(egui::CornerRadius::same(BANNER_RADIUS))
        .inner_margin(egui::Margin {
            left: 14,
            right: 8,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let icon = match status {
                    ImportStatus::Failed(..) => Icon::AlertCircle,
                    ImportStatus::Running(_) => Icon::Download,
                    _ => Icon::Check,
                };
                ui.label(
                    egui::RichText::new(icon.unicode().to_string())
                        .size(14.0)
                        .color(palette.accent),
                );
                let ended = !matches!(status, ImportStatus::Running(_));
                let room = ui.available_width() - if ended { DISMISS_SIZE + 8.0 } else { 0.0 };
                ui.allocate_ui_with_layout(
                    egui::vec2(room.max(0.0), DISMISS_SIZE),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(line)
                                    .size(13.0)
                                    .color(palette.text_primary),
                            )
                            .wrap(),
                        );
                    },
                );
                if ended && dismiss_button(ui, palette).clicked() {
                    intent = Some(BrowserIntent::DismissImport);
                }
            });
            if matches!(
                status,
                ImportStatus::Failed(_, ImportError::NeedsFullDiskAccess)
            ) {
                ui.add_space(6.0);
                if ui.button("Open Privacy Settings").clicked() {
                    intent = Some(BrowserIntent::OpenDiskAccessSettings);
                }
            }
        });
    ui.add_space(16.0);
    intent
}

fn dismiss_button(ui: &mut egui::Ui, palette: &Palette) -> egui::Response {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(DISMISS_SIZE, DISMISS_SIZE), true);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            with_alpha(palette.text_primary, 16),
        );
    }
    crate::ui::paint_icon(
        ui.painter(),
        rect.center(),
        13.0,
        Icon::X,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Dismiss"));
    response
}

fn import_links(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &ImportStatus,
    intents: &mut Vec<BrowserIntent>,
) {
    let running = matches!(status, ImportStatus::Running(_));
    for source in ImportSource::ALL {
        let text = egui::RichText::new(format!("Import from {}", source.name()))
            .size(12.0)
            .color(palette.text_secondary);
        let link = ui
            .add_enabled(!running, egui::Button::new(text).frame(false))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if link.clicked() {
            intents.push(BrowserIntent::Import(source));
        }
    }
}

fn import_status_line(status: &ImportStatus) -> Option<String> {
    Some(match status {
        ImportStatus::Idle => return None,
        ImportStatus::Running(source) => format!("Importing from {}…", source.name()),
        ImportStatus::Done(report) => {
            let mut line = format!(
                "Imported from {}: {} favorites, {} pages of history, {} cookies.",
                report.source.name(),
                report.favorites,
                report.history,
                report.cookies
            );
            if let Some(reason) = &report.cookies_skipped {
                line.push_str(&format!(" Cookies skipped: {reason}."));
            }
            line
        }
        ImportStatus::Failed(source, ImportError::NotFound) => {
            format!("No {} data found.", source.name())
        }
        ImportStatus::Failed(source, ImportError::NeedsFullDiskAccess) => format!(
            "{}'s data needs Full Disk Access for helm (System Settings › Privacy & Security).",
            source.name()
        ),
        ImportStatus::Failed(source, ImportError::Failed(reason)) => {
            format!("{} import failed: {reason}", source.name())
        }
    })
}
