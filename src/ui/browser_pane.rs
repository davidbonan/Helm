//! Browser pane of the split tree (specs/browser.md §3, §4, §8): toolbar, tab row,
//! address suggestions, page area (where the native view is placed, over the
//! page's last snapshot, else the start page) and the Browser offer of a fresh
//! terminal.

use std::collections::HashMap;

use lucide_icons::Icon;

use crate::browser::favicons;
use crate::browser::fold::ToolbarFold;
use crate::browser::import::{ImportSource, ImportStatus};
use crate::browser::library::{Library, Suggestion};
use crate::browser::page_view::{PageView, Viewport, ZoomChange};
use crate::browser::{url, Navigation, PageState};
use crate::keybindings::Shortcut;
use crate::theme::Palette;
use crate::ui::browser_start_page::{elided, paint_badge, Badge, FavoritesView};
use crate::ui::terminal_view::GRIP_RESERVE;
use crate::ui::with_alpha;

const BAR_HEIGHT: f32 = 40.0;
const FOLDED_BAR_HEIGHT: f32 = 26.0;
const FOLDED_TEXT_SIZE: f32 = 11.5;
const FOLD_ANIMATION: f32 = 0.18;
const BAR_PAD_X: f32 = 8.0;
const GROUP_GAP: f32 = 8.0;
const BUTTON_SIZE: f32 = 28.0;
const BUTTON_GAP: f32 = 2.0;
const ICON_SIZE: f32 = 15.0;
const PILL_HEIGHT: f32 = 28.0;
/// A capsule: half the pill's height.
const PILL_RADIUS: u8 = 14;
const PILL_MAX_WIDTH: f32 = 640.0;
const PILL_PAD_X: f32 = 10.0;
const PILL_ICON_SIZE: f32 = 13.0;
const PILL_ICON_GAP: f32 = 7.0;
const SCHEME_ICON_SIZE: f32 = 11.0;
const SCHEME_ICON_GAP: f32 = 5.0;
const STAR_SIZE: f32 = 22.0;
const MIN_PILL_WIDTH: f32 = 60.0;
const URL_TEXT_SIZE: f32 = 13.0;
const PROGRESS_GLOW_ALPHA: u8 = 90;
/// How fast the glow closes on WebKit's progress, per second.
const PROGRESS_EASE: f32 = 6.0;
/// Where the glow creeps while WebKit's progress stalls.
const PROGRESS_CREEP_CEILING: f32 = 0.9;
const PROGRESS_CREEP_RATE: f32 = 0.25;
const PROGRESS_FADE: f32 = 0.25;
/// Seconds the toolbar takes to follow a new page's tint.
const TINT_FADE: f32 = 0.2;
const ADDRESS_HINT: &str = "Search or enter address";
const SUGGESTION_HEIGHT: f32 = 32.0;
const SUGGESTION_RADIUS: u8 = 7;
/// Between the pill and the suggestions card.
const SUGGESTIONS_GAP: f32 = 6.0;
const SUGGESTIONS_PAD: f32 = 6.0;
/// Kept from the page area's edges.
const SUGGESTIONS_MARGIN: f32 = 8.0;
const SUGGESTIONS_MIN_WIDTH: f32 = 360.0;
const SUGGESTIONS_RADIUS: u8 = 12;
const TAB_ROW_HEIGHT: f32 = 34.0;
const TAB_HEIGHT: f32 = 26.0;
const TAB_RADIUS: u8 = 8;
const TAB_GAP: f32 = 4.0;
const TAB_TITLED_MIN_WIDTH: f32 = 80.0;
const TAB_MAX_WIDTH: f32 = 200.0;
const TAB_SHOWN_WIDTH: f32 = 150.0;
const TAB_BADGE_WIDTH: f32 = 30.0;
const TAB_BADGE_SIZE: f32 = 16.0;
const TAB_TITLE_SIZE: f32 = 12.5;
const TAB_CLOSE_SIZE: f32 = 18.0;
const UNTITLED_TAB: &str = "New tab";
const MORE_MENU_WIDTH: f32 = 260.0;
const FIND_ROW_HEIGHT: f32 = 34.0;
const FIND_FIELD_WIDTH: f32 = 240.0;
const FIND_HINT: &str = "Find in page";

const OFFER_SIZE: egui::Vec2 = egui::vec2(208.0, 124.0);
const OFFER_RADIUS: u8 = 12;
const OFFER_ICON_SIZE: f32 = 20.0;
const OFFER_TITLE_SIZE: f32 = 13.5;
const OFFER_DETAIL_SIZE: f32 = 11.5;
const OFFER_REST_OPACITY: f32 = 0.55;
const OFFER_SHORTCUT: Shortcut = Shortcut::cmd(egui::Key::Enter);

/// The URL field of one pane.
#[derive(Debug, Default)]
pub struct AddressBar {
    pub text: String,
    /// Set to give the field focus, its text selected, on the next frame.
    pub focus_requested: bool,
    /// The suggestion `↑`/`↓` picked (browser.md §8).
    pub selected: Option<usize>,
}

/// What one tab keeps between frames, besides its page.
#[derive(Default)]
pub struct BrowserTabState {
    pub address: AddressBar,
    /// The start page covers the page (browser.md §7).
    pub start_page: bool,
    pub fold: ToolbarFold,
    pub favorites: FavoritesView,
    pub page_view: PageView,
    /// Open under the toolbar (browser.md §4 *Find bar*).
    pub find: Option<FindBar>,
    /// The page's last snapshot, drawn under the native view: what shows while
    /// egui draws over the page (browser.md §4).
    pub backdrop: Option<egui::TextureHandle>,
}

impl BrowserTabState {
    /// Opens the find bar over the page, its field focused.
    pub fn open_find(&mut self) {
        self.start_page = false;
        self.find
            .get_or_insert_with(FindBar::default)
            .focus_requested = true;
    }
}

#[derive(Debug, Default)]
pub struct FindBar {
    pub text: String,
    pub focus_requested: bool,
    /// The engine found nothing for `text`.
    pub no_match: bool,
}

/// One tab as the tab row shows it (browser.md §4 *Tabs*).
#[derive(Clone)]
pub struct TabSummary {
    pub title: String,
    pub url: String,
}

impl TabSummary {
    fn label(&self) -> &str {
        if !self.title.is_empty() {
            return &self.title;
        }
        match url::display_parts(&self.url).0 {
            "" => UNTITLED_TAB,
            host => host,
        }
    }
}

/// The pane's tabs, in order; the shown one is the address pill's.
pub struct TabStrip {
    pub tabs: Vec<TabSummary>,
    pub active: usize,
}

/// What one browser pane draws: the shown tab's page and state, and its tabs.
pub struct BrowserPaneView<'a> {
    pub page: &'a PageState,
    pub tab: &'a mut BrowserTabState,
    pub strip: &'a TabStrip,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrowserIntent {
    /// What the URL field held on `Enter`.
    Load(String),
    /// An exact URL: a tile or a suggestion.
    Open(String),
    /// A tile `Cmd`-clicked: a tab after the shown one, in the background.
    OpenInNewTab(String),
    Navigate(Navigation),
    LoadServer,
    /// The toolbar's Favorites: shows the start page, or the page again.
    ToggleStartPage,
    ToggleFavorite {
        url: String,
        title: String,
    },
    RemoveFavorite(String),
    /// The next match of `text`, the previous when `backwards`.
    Find {
        text: String,
        backwards: bool,
    },
    /// The find bar closed: the page takes the keyboard back.
    FocusPage,
    RenameFavorite {
        url: String,
        title: String,
    },
    /// Hides the import's outcome.
    DismissImport,
    Import(ImportSource),
    OpenDiskAccessSettings,
    Inspect,
    OpenInChrome,
    NewTab,
    SelectTab(usize),
    CloseTab(usize),
}

#[derive(Debug)]
pub struct BrowserPaneOutput {
    pub intents: Vec<BrowserIntent>,
    /// Where the page view goes, in egui points; `None` while the start page
    /// stands in its place.
    pub page_rect: Option<egui::Rect>,
    /// The suggestions fall over the page: it shows frozen beneath them.
    pub page_overlaid: bool,
    /// A press in the toolbar (the page's own clicks are the engine's).
    pub clicked: bool,
    /// The URL or find field holds the keyboard.
    pub field_focused: bool,
}

/// What every browser pane of the tab shares this frame.
pub struct BrowserChrome<'a> {
    pub palette: &'a Palette,
    /// The worktree's server URL (browser.md §5), if known.
    pub server: Option<&'a str>,
    /// The project (group root) local favorites belong to (browser.md §7).
    pub project: &'a str,
    pub library: &'a Library,
    pub import: &'a ImportStatus,
    /// The site icons in, by site (browser.md §10).
    pub icons: &'a HashMap<String, egui::TextureHandle>,
}

impl BrowserChrome<'_> {
    pub(crate) fn site_icon(&self, page_url: &str) -> Option<egui::TextureId> {
        let site = favicons::site_of(page_url)?;
        self.icons.get(site).map(egui::TextureHandle::id)
    }
}

/// The toolbar's colors: on a page, its tint and the ink that reads best on it;
/// else the palette's own (browser.md §4).
struct BarColors {
    fill: egui::Color32,
    ink: egui::Color32,
    secondary: egui::Color32,
    muted: egui::Color32,
    disabled: egui::Color32,
    button_hover: egui::Color32,
    pill: RaisedPill,
    accent: egui::Color32,
    accent_subtle: egui::Color32,
    /// A tinted bar runs into the page; the palette's keeps its hairline.
    tinted: bool,
}

/// The address pill stands off the bar like Safari's capsule, by a wide soft
/// shadow alone: white on a light bar, a lighter shade on a dark one.
struct RaisedPill {
    fill: egui::Color32,
    hover: egui::Color32,
    shadow: egui::Color32,
}

impl RaisedPill {
    fn on(bar: egui::Color32, light_bar: bool) -> Self {
        if light_bar {
            return Self {
                fill: egui::Color32::WHITE,
                hover: egui::Color32::WHITE,
                shadow: with_alpha(egui::Color32::BLACK, 22),
            };
        }
        Self {
            fill: bar.lerp_to_gamma(egui::Color32::WHITE, 0.07),
            hover: bar.lerp_to_gamma(egui::Color32::WHITE, 0.11),
            shadow: with_alpha(egui::Color32::BLACK, 80),
        }
    }

    fn paint(&self, painter: &egui::Painter, rect: egui::Rect, radius: egui::CornerRadius) {
        let shadow = egui::Shadow {
            offset: [0, 1],
            blur: 10,
            spread: 0,
            color: self.shadow,
        };
        painter.add(shadow.as_shape(rect, radius));
        painter.rect_filled(rect, radius, self.fill);
    }
}

impl BarColors {
    fn new(ui: &egui::Ui, palette: &Palette, tint: Option<[u8; 3]>) -> Self {
        let Some([red, green, blue]) = tint else {
            return Self::untinted(palette);
        };
        let fill = faded_tint(ui, egui::Color32::from_rgb(red, green, blue));
        let ink = readable_ink(fill);
        Self {
            fill,
            ink,
            secondary: with_alpha(ink, 210),
            muted: with_alpha(ink, 140),
            disabled: with_alpha(ink, 80),
            button_hover: fill.lerp_to_gamma(ink, 0.08),
            pill: RaisedPill::on(fill, ink == Palette::light().text_primary),
            accent: palette.accent,
            accent_subtle: palette.accent_subtle,
            tinted: true,
        }
    }

    fn untinted(palette: &Palette) -> Self {
        Self {
            fill: palette.bg_canvas,
            ink: palette.text_primary,
            secondary: palette.text_secondary,
            muted: palette.text_muted,
            disabled: palette.state_disabled,
            button_hover: palette.bg_surface_hover,
            pill: RaisedPill::on(palette.bg_canvas, !palette.dark),
            accent: palette.accent,
            accent_subtle: palette.accent_subtle,
            tinted: false,
        }
    }
}

/// `target`, reached over `TINT_FADE` from the tint the bar showed before.
fn faded_tint(ui: &egui::Ui, target: egui::Color32) -> egui::Color32 {
    let id = ui.id().with("bar_tint");
    let [red, green, blue, _] = target.to_array();
    let channel = |at: usize, value: u8| {
        ui.ctx()
            .animate_value_with_time(id.with(at), f32::from(value), TINT_FADE)
            .round() as u8
    };
    egui::Color32::from_rgb(channel(0, red), channel(1, green), channel(2, blue))
}

/// The light or the dark theme's primary text, whichever contrasts more on `fill`.
fn readable_ink(fill: egui::Color32) -> egui::Color32 {
    let dark_ink = Palette::light().text_primary;
    let light_ink = Palette::dark().text_primary;
    if contrast(fill, dark_ink) >= contrast(fill, light_ink) {
        dark_ink
    } else {
        light_ink
    }
}

/// WCAG contrast ratio.
fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn relative_luminance(color: egui::Color32) -> f32 {
    let [red, green, blue, _] = color.to_srgba_unmultiplied();
    let linear = |channel: u8| {
        let channel = f32::from(channel) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
}

/// How the address pill is drawn this frame.
struct Pill<'a> {
    colors: &'a BarColors,
    width: f32,
}

pub fn browser_pane(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: BrowserPaneView<'_>,
) -> BrowserPaneOutput {
    let BrowserPaneView { page, tab, strip } = view;
    let palette = chrome.palette;
    let pane = ui.available_rect_before_wrap();
    let shows_page = !(tab.start_page || page.url.is_empty());
    let folded = shows_page && toolbar_folded(ui, page, tab, pane);
    let fold =
        ui.ctx()
            .animate_bool_with_time(ui.id().with("toolbar_fold"), folded, FOLD_ANIMATION);
    let has_row = strip.tabs.len() > 1;
    let finding = shows_page && tab.find.is_some();
    let row_height = if has_row { TAB_ROW_HEIGHT } else { 0.0 };
    let find_height = if finding { FIND_ROW_HEIGHT } else { 0.0 };
    let full_height = BAR_HEIGHT + row_height + find_height;
    let bar_height = egui::lerp(full_height..=FOLDED_BAR_HEIGHT, fold);
    let (bar_rect, page_area) = pane.split_top_bottom_at_y(pane.top() + bar_height);
    let viewport = viewport_rect(page_area, tab.page_view.viewport);
    let colors = BarColors::new(ui, palette, page.tint.filter(|_| shows_page));
    ui.painter().rect_filled(bar_rect, 0.0, colors.fill);
    if !colors.tinted {
        ui.painter().hline(
            bar_rect.x_range(),
            bar_rect.bottom() - 0.5,
            egui::Stroke::new(1.0_f32, palette.border_subtle),
        );
    }
    let page_fill = if colors.tinted {
        colors.fill
    } else {
        palette.bg_surface
    };
    ui.painter().rect_filled(page_area, 0.0, page_fill);
    if shows_page {
        if viewport != page_area {
            ui.painter().rect_filled(page_area, 0.0, palette.bg_sidebar);
            ui.painter().rect_filled(viewport, 0.0, page_fill);
        }
        paint_backdrop(ui, tab.backdrop.as_ref(), viewport);
    }
    if folded {
        if folded_toolbar(ui, &colors, bar_rect, &page.url) {
            tab.fold.unfold();
            tab.address.focus_requested = true;
        }
        ui.advance_cursor_after_rect(pane);
        return BrowserPaneOutput {
            intents: Vec::new(),
            page_rect: Some(viewport),
            page_overlaid: false,
            clicked: pressed_in(ui, bar_rect),
            field_focused: false,
        };
    }
    // The pane's drag grip owns the top-right corner on hover.
    let mut bar_ui_rect = bar_rect;
    bar_ui_rect.max.y = bar_ui_rect.max.y.min(bar_rect.top() + BAR_HEIGHT);
    bar_ui_rect.min.x += BAR_PAD_X;
    bar_ui_rect.max.x -= GRIP_RESERVE;
    let mut bar_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_ui_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar_ui.spacing_mut().item_spacing = egui::vec2(BUTTON_GAP, 0.0);
    let mut intents = Vec::new();
    history_buttons(&mut bar_ui, &colors, page, &mut intents);
    bar_ui.add_space(GROUP_GAP);
    let actions = page_actions(chrome, page, tab.start_page);
    let trailing = actions.len() + 1 + usize::from(!has_row);
    let actions_width = trailing as f32 * (BUTTON_SIZE + BUTTON_GAP) + 2.0 * GROUP_GAP;
    let slot = (bar_ui.available_width() - actions_width).max(MIN_PILL_WIDTH);
    let pill = Pill {
        colors: &colors,
        width: slot.clamp(MIN_PILL_WIDTH, PILL_MAX_WIDTH),
    };
    let centering = ((slot - pill.width) / 2.0).max(0.0);
    bar_ui.add_space(centering);
    let address = address_bar(&mut bar_ui, chrome, page, &mut tab.address, &pill);
    intents.extend(address.intent);
    bar_ui.add_space(centering + GROUP_GAP);
    if !has_row && new_tab_button(&mut bar_ui, &colors) {
        intents.push(BrowserIntent::NewTab);
    }
    bar_ui.add_space(GROUP_GAP);
    for action in actions {
        if toolbar_button(&mut bar_ui, &colors, &action.button).clicked() {
            intents.push(action.intent);
        }
    }
    let more = MoreMenu {
        colors: &colors,
        page,
    };
    intents.extend(more_menu(&mut bar_ui, &more, tab));
    let page_rect = if shows_page {
        Some(viewport)
    } else {
        intents.extend(crate::ui::browser_start_page::start_page(
            ui,
            chrome,
            &mut tab.favorites,
            page_area,
        ));
        None
    };
    let row_top = bar_rect.top() + BAR_HEIGHT;
    let row_area = bar_band(bar_rect, row_top, row_height);
    if has_row && row_area.height() > 0.0 {
        let row = TabRow {
            strip,
            chrome,
            colors: &colors,
        };
        intents.extend(tab_row(ui, &row, row_area));
    }
    let find_area = bar_band(bar_rect, row_top + row_height, find_height);
    let mut find_focused = false;
    if finding && find_area.height() > 0.0 {
        let found = find_bar(ui, &colors, &mut tab.find, find_area);
        find_focused = found.has_focus;
        intents.extend(found.intent);
    }
    let page_overlaid = !address.suggestions.is_empty();
    if page_overlaid {
        let list = SuggestionList {
            suggestions: &address.suggestions,
            selected: tab.address.selected,
            pill: address.pill_rect,
            area: page_area,
        };
        intents.extend(suggestion_list(ui, &colors, &list).map(BrowserIntent::Open));
    }
    ui.advance_cursor_after_rect(pane);
    BrowserPaneOutput {
        intents,
        page_rect,
        page_overlaid,
        clicked: pressed_in(ui, bar_rect),
        field_focused: address.has_focus || find_focused,
    }
}

/// The band of the bar from `top`, `height` high, clipped to the bar while it
/// folds.
fn bar_band(bar: egui::Rect, top: f32, height: f32) -> egui::Rect {
    egui::Rect::from_x_y_ranges(bar.x_range(), top..=(top + height).min(bar.bottom()))
}

/// Where the page goes in `area` (browser.md §4 *Viewport*): all of it, or the
/// viewport's width centered.
fn viewport_rect(area: egui::Rect, viewport: Viewport) -> egui::Rect {
    match viewport.width() {
        Some(width) if width < area.width() => {
            egui::Rect::from_center_size(area.center(), egui::vec2(width, area.height()))
        }
        _ => area,
    }
}

fn paint_backdrop(ui: &egui::Ui, backdrop: Option<&egui::TextureHandle>, area: egui::Rect) {
    let Some(texture) = backdrop else {
        return;
    };
    let rect =
        egui::Rect::from_min_size(area.min, texture.size_vec2() / ui.ctx().pixels_per_point());
    let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter()
        .with_clip_rect(area)
        .image(texture.id(), rect, whole, egui::Color32::WHITE);
}

fn pressed_in(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|i| {
        i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|at| rect.contains(at))
    })
}

/// Whether the toolbar folds this frame (browser.md §4): the page scrolled down,
/// unless the URL field is in use or the pointer moves over the bar. The pointer
/// must move: over the page view egui keeps its last position, stale.
fn toolbar_folded(
    ui: &egui::Ui,
    page: &PageState,
    state: &mut BrowserTabState,
    pane: egui::Rect,
) -> bool {
    let folded = state.fold.follow(&page.url, page.scroll_y);
    let bar = egui::Rect::from_min_size(pane.min, egui::vec2(pane.width(), FOLDED_BAR_HEIGHT));
    let editing = state.address.focus_requested
        || state.find.is_some()
        || ui.ctx().memory(|m| m.has_focus(address_id(ui)));
    let reached = ui.input(|i| {
        i.pointer.is_moving() && i.pointer.hover_pos().is_some_and(|at| bar.contains(at))
    });
    if folded && (editing || reached) {
        state.fold.unfold();
        return false;
    }
    folded
}

/// The folded toolbar: the page's host, small and centered. `true` when clicked.
fn folded_toolbar(ui: &mut egui::Ui, colors: &BarColors, bar: egui::Rect, page_url: &str) -> bool {
    let (host, _) = url::display_parts(page_url);
    let response = ui
        .interact(bar, ui.id().with("folded_toolbar"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut job = egui::text::LayoutJob::simple_singleline(
        host.to_owned(),
        egui::FontId::proportional(FOLDED_TEXT_SIZE),
        colors.ink,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(bar.width() - 2.0 * BAR_PAD_X);
    let galley = ui.painter().layout_job(job);
    let at = bar.center() - galley.size() / 2.0;
    ui.painter().galley(at, galley, colors.ink);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, host));
    response.clicked()
}

/// Back · forward, each only while it leads somewhere.
fn history_buttons(
    ui: &mut egui::Ui,
    colors: &BarColors,
    page: &PageState,
    intents: &mut Vec<BrowserIntent>,
) {
    let history = [
        (Icon::ArrowLeft, "Back", page.can_go_back, Navigation::Back),
        (
            Icon::ArrowRight,
            "Forward",
            page.can_go_forward,
            Navigation::Forward,
        ),
    ];
    for (icon, label, enabled, navigation) in history {
        if !enabled {
            continue;
        }
        let button = ToolButton::new(icon, label, true);
        if toolbar_button(ui, colors, &button).clicked() {
            intents.push(BrowserIntent::Navigate(navigation));
        }
    }
}

struct ToolButton<'a> {
    icon: Icon,
    label: std::borrow::Cow<'a, str>,
    enabled: bool,
    /// Pressed look: what it shows is on screen.
    active: bool,
}

impl<'a> ToolButton<'a> {
    fn new(icon: Icon, label: impl Into<std::borrow::Cow<'a, str>>, enabled: bool) -> Self {
        Self {
            icon,
            label: label.into(),
            enabled,
            active: false,
        }
    }
}

struct PageAction {
    button: ToolButton<'static>,
    intent: BrowserIntent,
}

/// The toolbar's trailing actions: dev server, favorites.
fn page_actions(chrome: &BrowserChrome<'_>, page: &PageState, start_page: bool) -> [PageAction; 2] {
    let server_label = match chrome.server {
        Some(server) => {
            let (host, path) = url::display_parts(server);
            format!("Dev server · {host}{path}")
        }
        None => "Dev server".to_owned(),
    };
    let has_page = !page.url.is_empty();
    let action = |icon, label: String, enabled, intent| PageAction {
        button: ToolButton::new(icon, label, enabled),
        intent,
    };
    let mut favorites = action(
        Icon::Star,
        "Favorites".to_owned(),
        has_page,
        BrowserIntent::ToggleStartPage,
    );
    favorites.button.active = has_page && start_page;
    [
        action(
            Icon::Server,
            server_label,
            chrome.server.is_some(),
            BrowserIntent::LoadServer,
        ),
        favorites,
    ]
}

/// `true` when clicked.
fn new_tab_button(ui: &mut egui::Ui, colors: &BarColors) -> bool {
    let new_tab = ToolButton::new(Icon::Plus, UNTITLED_TAB, true);
    toolbar_button(ui, colors, &new_tab).clicked()
}

struct MoreMenu<'a> {
    colors: &'a BarColors,
    page: &'a PageState,
}

/// The toolbar's `⋯` (browser.md §4 *More*): find, zoom and viewport change the
/// tab's state in place; the rest come back as intents.
fn more_menu(
    ui: &mut egui::Ui,
    more: &MoreMenu<'_>,
    tab: &mut BrowserTabState,
) -> Option<BrowserIntent> {
    let button = ToolButton::new(Icon::Ellipsis, "More", true);
    let response = toolbar_button(ui, more.colors, &button);
    let has_page = !more.page.url.is_empty();
    let mut intent = None;
    egui::Popup::menu(&response)
        .style(crate::theme::menu_style)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(MORE_MENU_WIDTH);
            ui.add_enabled_ui(has_page, |ui| {
                let find = egui::Button::new("Find in Page").shortcut_text("⌘F");
                if ui.add(find).clicked() {
                    tab.open_find();
                    ui.close();
                }
                zoom_row(ui, &mut tab.page_view);
                viewport_row(ui, &mut tab.page_view.viewport);
                ui.separator();
                for (label, action) in [
                    ("Web Inspector", BrowserIntent::Inspect),
                    ("Open in Chrome", BrowserIntent::OpenInChrome),
                ] {
                    if ui.button(label).clicked() {
                        intent = Some(action);
                        ui.close();
                    }
                }
                if ui.button("Copy URL").clicked() {
                    ui.ctx().copy_text(more.page.url.clone());
                    ui.close();
                }
            });
        });
    intent
}

/// `−` · percent · `+`; a click on the percent resets it.
fn zoom_row(ui: &mut egui::Ui, view: &mut PageView) {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().button_padding.x);
        ui.label("Zoom");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let changes = [
                (ui.button("+").on_hover_text("Zoom In"), ZoomChange::In),
                (
                    ui.button(format!("{:.0} %", view.zoom * 100.0))
                        .on_hover_text("Actual Size"),
                    ZoomChange::Reset,
                ),
                (ui.button("−").on_hover_text("Zoom Out"), ZoomChange::Out),
            ];
            for (response, change) in changes {
                if response.clicked() {
                    view.zoom = view.zoomed(change);
                }
            }
        });
    });
}

fn viewport_row(ui: &mut egui::Ui, viewport: &mut Viewport) {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().button_padding.x);
        ui.label("Viewport");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for choice in Viewport::ALL.into_iter().rev() {
                ui.selectable_value(viewport, choice, choice.name());
            }
        });
    });
}

struct FindOutput {
    has_focus: bool,
    intent: Option<BrowserIntent>,
}

/// The find bar (browser.md §4): typing finds, `Enter` the next match,
/// `Shift+Enter` the previous; `Esc` or `×` closes it.
fn find_bar(
    ui: &mut egui::Ui,
    colors: &BarColors,
    slot: &mut Option<FindBar>,
    area: egui::Rect,
) -> FindOutput {
    let Some(find) = slot.as_mut() else {
        return FindOutput {
            has_focus: false,
            intent: None,
        };
    };
    let inner = area.shrink2(egui::vec2(BAR_PAD_X, 0.0));
    let mut bar_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar_ui.shrink_clip_rect(area);
    bar_ui.spacing_mut().item_spacing = egui::vec2(BUTTON_GAP, 0.0);
    let id = bar_ui.id().with("browser_find");
    let output = egui::TextEdit::singleline(&mut find.text)
        .id(id)
        .hint_text(FIND_HINT)
        .margin(egui::Margin::symmetric(8, 4))
        .font(egui::FontId::proportional(URL_TEXT_SIZE))
        .text_color(colors.ink)
        .desired_width(FIND_FIELD_WIDTH)
        .show(&mut bar_ui);
    let field = output.response;
    if std::mem::take(&mut find.focus_requested) {
        field.request_focus();
    }
    let (enter, escape, shift) = bar_ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
            i.modifiers.shift,
        )
    });
    let mut intent = None;
    if field.changed() {
        find.no_match = false;
        intent = (!find.text.is_empty()).then(|| BrowserIntent::Find {
            text: find.text.clone(),
            backwards: false,
        });
    }
    if field.lost_focus() && enter {
        find.focus_requested = true;
        intent = Some(BrowserIntent::Find {
            text: find.text.clone(),
            backwards: shift,
        });
    }
    bar_ui.add_space(GROUP_GAP);
    let has_text = !find.text.is_empty();
    for (icon, label, backwards) in [
        (Icon::ChevronUp, "Previous match", true),
        (Icon::ChevronDown, "Next match", false),
    ] {
        let button = ToolButton::new(icon, label, has_text);
        if toolbar_button(&mut bar_ui, colors, &button).clicked() {
            intent = Some(BrowserIntent::Find {
                text: find.text.clone(),
                backwards,
            });
        }
    }
    if find.no_match && has_text {
        bar_ui.add_space(GROUP_GAP);
        bar_ui.label(
            egui::RichText::new("No matches")
                .size(TAB_TITLE_SIZE)
                .color(colors.muted),
        );
    }
    let closed = bar_ui
        .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ToolButton::new(Icon::X, "Close find bar", true);
            toolbar_button(ui, colors, &close).clicked()
        })
        .inner;
    let has_focus = field.has_focus();
    if closed || (field.lost_focus() && escape) {
        *slot = None;
        return FindOutput {
            has_focus: false,
            intent: Some(BrowserIntent::FocusPage),
        };
    }
    FindOutput { has_focus, intent }
}

struct AddressOutput {
    has_focus: bool,
    intent: Option<BrowserIntent>,
    /// The list to show under the pill (browser.md §8).
    suggestions: Vec<Suggestion>,
    pill_rect: egui::Rect,
}

/// The address pill and its suggestions: `↑`/`↓` pick one, `Enter` opens it, or
/// loads the typed text when none is picked.
fn address_bar(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    page: &PageState,
    bar: &mut AddressBar,
    pill: &Pill<'_>,
) -> AddressOutput {
    let id = address_id(ui);
    let editing = ui.ctx().memory(|m| m.has_focus(id));
    let listed = if editing {
        listed_suggestions(chrome, page, bar)
    } else {
        Vec::new()
    };
    if !listed.is_empty() {
        move_selection(ui, bar, listed.len());
    }
    let typed_before = bar.text.clone();
    let field = address_field(ui, chrome, page, bar, pill);
    let retyped = bar.text != typed_before;
    if retyped {
        bar.selected = None;
    }
    let intent = if let Some(typed) = field.entered {
        let picked = bar.selected.take().and_then(|at| listed.get(at));
        Some(match picked {
            Some(suggestion) => BrowserIntent::Open(suggestion.url.clone()),
            None => BrowserIntent::Load(typed),
        })
    } else if field.star_clicked {
        Some(BrowserIntent::ToggleFavorite {
            url: page.url.clone(),
            title: page.title.clone(),
        })
    } else {
        field.navigated.map(BrowserIntent::Navigate)
    };
    // A click on a suggestion takes the field's focus on release: the list stays
    // that frame so the click lands.
    let released = editing && ui.input(|i| i.pointer.any_released());
    let suggestions = match (field.has_focus || released, retyped) {
        (false, _) => {
            bar.selected = None;
            Vec::new()
        }
        (true, true) => listed_suggestions(chrome, page, bar),
        (true, false) => listed,
    };
    AddressOutput {
        has_focus: field.has_focus,
        intent,
        suggestions,
        pill_rect: field.rect,
    }
}

fn address_id(ui: &egui::Ui) -> egui::Id {
    ui.id().with("browser_address")
}

/// Nothing while the field still shows the page's own URL.
fn listed_suggestions(
    chrome: &BrowserChrome<'_>,
    page: &PageState,
    bar: &AddressBar,
) -> Vec<Suggestion> {
    if bar.text == page.url {
        return Vec::new();
    }
    chrome.library.suggestions(&bar.text, chrome.project)
}

fn move_selection(ui: &egui::Ui, bar: &mut AddressBar, count: usize) {
    let (down, up) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
        )
    });
    bar.selected = match (down, up, bar.selected) {
        (true, _, None) => Some(0),
        (true, _, Some(at)) => Some((at + 1).min(count - 1)),
        (_, true, Some(0)) => None,
        (_, true, Some(at)) => Some(at - 1),
        (_, _, selected) => selected.filter(|&at| at < count),
    };
}

struct FieldOutput {
    rect: egui::Rect,
    has_focus: bool,
    /// What the field held on `Enter`.
    entered: Option<String>,
    star_clicked: bool,
    navigated: Option<Navigation>,
}

/// The address pill: the page's host centered at rest, ended by its favorite star
/// and reload; the full URL (selected) once clicked.
fn address_field(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    page: &PageState,
    bar: &mut AddressBar,
    pill: &Pill<'_>,
) -> FieldOutput {
    let colors = pill.colors;
    let id = address_id(ui);
    let editing = ui.ctx().memory(|m| m.has_focus(id));
    if !editing {
        bar.text.clone_from(&page.url);
    }
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(pill.width, PILL_HEIGHT), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::Text);
    if response.clicked() {
        bar.focus_requested = true;
    }
    let hovered = response.contains_pointer();
    paint_pill(ui, colors, rect, page, editing || hovered);
    if !(editing || bar.focus_requested || page.url.is_empty()) {
        paint_host(ui, colors, rect, &page.url);
        response.widget_info(|| egui::WidgetInfo::text_edit(true, "", &page.url, ADDRESS_HINT));
        let reload_center = egui::pos2(
            rect.right() - PILL_PAD_X / 2.0 - STAR_SIZE / 2.0,
            rect.center().y,
        );
        let star = Star {
            center: reload_center - egui::vec2(STAR_SIZE, 0.0),
            favorite: chrome.library.is_favorite(&page.url, chrome.project),
            shown: hovered,
        };
        let star_clicked = favorite_star(ui, colors, &star);
        return FieldOutput {
            rect,
            has_focus: false,
            entered: None,
            star_clicked,
            navigated: reload_button(ui, colors, reload_center, page.loading),
        };
    }
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(PILL_RADIUS),
        egui::Stroke::new(1.0_f32, colors.accent),
        egui::StrokeKind::Inside,
    );
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(
            rect.left() + PILL_PAD_X + PILL_ICON_SIZE / 2.0,
            rect.center().y,
        ),
        PILL_ICON_SIZE,
        scheme_icon(&page.url),
        colors.muted,
    );
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + PILL_PAD_X + PILL_ICON_SIZE + PILL_ICON_GAP,
            rect.top(),
        ),
        egui::pos2(rect.right() - PILL_PAD_X, rect.bottom()),
    );
    let mut edit_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let output = egui::TextEdit::singleline(&mut bar.text)
        .id(id)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .hint_text(ADDRESS_HINT)
        .font(egui::FontId::proportional(URL_TEXT_SIZE))
        .text_color(colors.ink)
        .desired_width(f32::INFINITY)
        .show(&mut edit_ui);
    if std::mem::take(&mut bar.focus_requested) {
        output.response.request_focus();
        let mut state = output.state;
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(bar.text.chars().count()),
            )));
        state.store(ui.ctx(), id);
    }
    let entered = output.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    FieldOutput {
        rect,
        has_focus: output.response.has_focus(),
        entered: entered.then(|| bar.text.clone()),
        star_clicked: false,
        navigated: None,
    }
}

/// The pill's reload, stop while loading; `Shift` bypasses the cache.
fn reload_button(
    ui: &mut egui::Ui,
    colors: &BarColors,
    center: egui::Pos2,
    loading: bool,
) -> Option<Navigation> {
    let (icon, label) = if loading {
        (Icon::X, "Stop")
    } else {
        (Icon::RotateCw, "Reload (⇧ ignores the cache)")
    };
    let rect = egui::Rect::from_center_size(center, egui::vec2(STAR_SIZE, STAR_SIZE));
    let response = ui
        .interact(rect, ui.id().with("reload"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label);
    let tint = if response.hovered() {
        colors.ink
    } else {
        colors.muted
    };
    crate::ui::paint_icon(ui.painter(), center, PILL_ICON_SIZE, icon, tint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if !response.clicked() {
        return None;
    }
    Some(match (loading, ui.input(|i| i.modifiers.shift)) {
        (true, _) => Navigation::Stop,
        (false, true) => Navigation::ReloadFromOrigin,
        (false, false) => Navigation::Reload,
    })
}

struct Star {
    center: egui::Pos2,
    favorite: bool,
    /// Under the pointer: a page that is not a favorite shows its star then.
    shown: bool,
}

/// The pill's star: accent when the page is a favorite. `true` when clicked.
fn favorite_star(ui: &mut egui::Ui, colors: &BarColors, star: &Star) -> bool {
    let rect = egui::Rect::from_center_size(star.center, egui::vec2(STAR_SIZE, STAR_SIZE));
    let label = if star.favorite {
        "Remove from favorites"
    } else {
        "Add to favorites"
    };
    let response = ui
        .interact(rect, ui.id().with("favorite_star"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label);
    let tint = match (star.favorite, response.hovered(), star.shown) {
        (true, _, _) => Some(colors.accent),
        (false, true, _) => Some(colors.ink),
        (false, false, true) => Some(colors.muted),
        (false, false, false) => None,
    };
    if let Some(tint) = tint {
        crate::ui::paint_icon(ui.painter(), star.center, PILL_ICON_SIZE, Icon::Star, tint);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.clicked()
}

/// The pill's raised surface and the load progress, a glow filling it from the
/// left.
fn paint_pill(ui: &egui::Ui, colors: &BarColors, rect: egui::Rect, page: &PageState, lit: bool) {
    let pill = &colors.pill;
    let radius = egui::CornerRadius::same(PILL_RADIUS);
    let painter = ui.painter();
    pill.paint(painter, rect, radius);
    if lit {
        painter.rect_filled(rect, radius, pill.hover);
    }
    let Some(glow) = load_glow(ui, page) else {
        return;
    };
    let track = rect.shrink2(egui::vec2(f32::from(PILL_RADIUS), 0.0));
    let head = track.left() + track.width() * glow.progress;
    let span = egui::Rect::from_x_y_ranges(rect.left()..=head, rect.y_range());
    let clear = with_alpha(colors.accent, 0);
    let bright = with_alpha(
        colors.accent,
        (f32::from(PROGRESS_GLOW_ALPHA) * glow.opacity) as u8,
    );
    let mut mesh = egui::Mesh::default();
    for (corner, color) in [
        (span.left_top(), clear),
        (span.right_top(), bright),
        (span.right_bottom(), bright),
        (span.left_bottom(), clear),
    ] {
        mesh.colored_vertex(corner, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

struct LoadGlow {
    progress: f32,
    opacity: f32,
}

/// What the glow showed last frame, and whether the page was loading then.
#[derive(Clone, Copy)]
struct GlowMemory {
    progress: f32,
    loading: bool,
}

/// The glow drawn this frame: it eases toward WebKit's coarse progress steps,
/// creeps on while they stall, runs to full then fades once the load ends, and
/// restarts from empty on a new load. Keeps frames coming meanwhile.
fn load_glow(ui: &egui::Ui, page: &PageState) -> Option<LoadGlow> {
    let id = ui.id().with("load_glow");
    let target = if page.loading {
        page.progress.clamp(0.1, 1.0)
    } else {
        1.0
    };
    let before: Option<GlowMemory> = ui.data(|data| data.get_temp(id));
    let shown = match before {
        Some(before) if page.loading && !before.loading => 0.0,
        Some(before) => before.progress,
        None if page.loading => 0.0,
        None => return None,
    };
    let dt = ui.input(|input| input.stable_dt).min(0.1);
    let eased = shown + (target - shown).max(0.0) * (1.0 - (-PROGRESS_EASE * dt).exp());
    let crept = if page.loading && eased < PROGRESS_CREEP_CEILING {
        eased + (PROGRESS_CREEP_CEILING - eased) * PROGRESS_CREEP_RATE * dt
    } else {
        eased
    };
    let progress = crept.min(1.0);
    let showing = page.loading || progress < 0.995;
    let opacity = ui
        .ctx()
        .animate_bool_with_time(id.with("opacity"), showing, PROGRESS_FADE);
    if opacity <= 0.0 {
        ui.data_mut(|data| data.remove::<GlowMemory>(id));
        return None;
    }
    let memory = GlowMemory {
        progress,
        loading: page.loading,
    };
    ui.data_mut(|data| data.insert_temp(id, memory));
    ui.ctx().request_repaint();
    Some(LoadGlow { progress, opacity })
}

fn scheme_icon(page_url: &str) -> Icon {
    if page_url.starts_with("https://") {
        Icon::Lock
    } else {
        Icon::Globe
    }
}

/// The pill at rest: the scheme icon and the page's host, centered between the
/// star's room on both sides.
fn paint_host(ui: &egui::Ui, colors: &BarColors, rect: egui::Rect, page_url: &str) {
    let (host, _) = url::display_parts(page_url);
    let room =
        rect.width() - 2.0 * (PILL_PAD_X + 2.0 * STAR_SIZE) - SCHEME_ICON_SIZE - SCHEME_ICON_GAP;
    let mut job = egui::text::LayoutJob::simple_singleline(
        host.to_owned(),
        egui::FontId::proportional(URL_TEXT_SIZE),
        colors.ink,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(0.0));
    let galley = ui.painter().layout_job(job);
    let left = rect.center().x - (SCHEME_ICON_SIZE + SCHEME_ICON_GAP + galley.size().x) / 2.0;
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(left + SCHEME_ICON_SIZE / 2.0, rect.center().y),
        SCHEME_ICON_SIZE,
        scheme_icon(page_url),
        colors.muted,
    );
    let at = egui::pos2(
        left + SCHEME_ICON_SIZE + SCHEME_ICON_GAP,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(at, galley, colors.ink);
}

fn url_galley(
    ui: &egui::Ui,
    colors: &BarColors,
    lead: &str,
    rest: &str,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let font = egui::FontId::proportional(URL_TEXT_SIZE);
    let mut job = egui::text::LayoutJob::default();
    for (text, color) in [(lead, colors.ink), (rest, colors.muted)] {
        job.append(text, 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.painter().layout_job(job)
}

struct SuggestionList<'a> {
    suggestions: &'a [Suggestion],
    selected: Option<usize>,
    /// The address pill the card hangs from.
    pill: egui::Rect,
    area: egui::Rect,
}

/// The card under the address pill (browser.md §8), as many rows as fit in the
/// page area; `None` when none does.
fn suggestions_card(list: &SuggestionList<'_>) -> Option<(egui::Rect, usize)> {
    let area = list.area;
    let width = list
        .pill
        .width()
        .max(SUGGESTIONS_MIN_WIDTH)
        .min(area.width() - 2.0 * SUGGESTIONS_MARGIN);
    let top = list.pill.bottom() + SUGGESTIONS_GAP;
    let room = area.bottom() - SUGGESTIONS_MARGIN - top - 2.0 * SUGGESTIONS_PAD;
    let rows = ((room / SUGGESTION_HEIGHT).floor().max(0.0) as usize).min(list.suggestions.len());
    if width <= 0.0 || rows == 0 {
        return None;
    }
    let left = (list.pill.center().x - width / 2.0)
        .min(area.right() - SUGGESTIONS_MARGIN - width)
        .max(area.left() + SUGGESTIONS_MARGIN);
    let height = rows as f32 * SUGGESTION_HEIGHT + 2.0 * SUGGESTIONS_PAD;
    let card = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, height));
    Some((card, rows))
}

/// The suggestions card, in the toolbar's colors; the URL clicked, if any.
fn suggestion_list(
    ui: &mut egui::Ui,
    colors: &BarColors,
    list: &SuggestionList<'_>,
) -> Option<String> {
    let (card, rows) = suggestions_card(list)?;
    // Swallows the clicks between rows: nothing under the card takes them.
    let _ = ui.interact(card, ui.id().with("suggestions"), egui::Sense::click());
    let radius = egui::CornerRadius::same(SUGGESTIONS_RADIUS);
    let shadow = egui::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: colors.pill.shadow,
    };
    ui.painter().add(shadow.as_shape(card, radius));
    ui.painter().rect(
        card,
        radius,
        colors.pill.fill,
        egui::Stroke::new(1.0_f32, with_alpha(colors.ink, 18)),
        egui::StrokeKind::Inside,
    );
    let lit = lit_row(colors);
    let mut picked = None;
    for (at, suggestion) in list.suggestions.iter().take(rows).enumerate() {
        let row = egui::Rect::from_min_size(
            card.min
                + egui::vec2(
                    SUGGESTIONS_PAD,
                    SUGGESTIONS_PAD + at as f32 * SUGGESTION_HEIGHT,
                ),
            egui::vec2(card.width() - 2.0 * SUGGESTIONS_PAD, SUGGESTION_HEIGHT),
        );
        let response = ui
            .interact(row, ui.id().with(("suggestion", at)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() || list.selected == Some(at) {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(SUGGESTION_RADIUS), lit);
        }
        suggestion_row(ui, colors, row, suggestion);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &suggestion.url)
        });
        if response.clicked() {
            picked = Some(suggestion.url.clone());
        }
    }
    picked
}

fn suggestion_row(ui: &egui::Ui, colors: &BarColors, row: egui::Rect, suggestion: &Suggestion) {
    let (icon, tint) = if suggestion.favorite {
        (Icon::Star, colors.accent)
    } else {
        (Icon::Clock, colors.muted)
    };
    let icon_center = egui::pos2(row.left() + 10.0 + PILL_ICON_SIZE / 2.0, row.center().y);
    crate::ui::paint_icon(ui.painter(), icon_center, PILL_ICON_SIZE, icon, tint);
    let (host, path) = url::display_parts(&suggestion.url);
    let title = if suggestion.title.is_empty() {
        host
    } else {
        suggestion.title.as_str()
    };
    let text_left = icon_center.x + PILL_ICON_SIZE / 2.0 + 10.0;
    let galley = url_galley(
        ui,
        colors,
        title,
        &format!("  {host}{path}"),
        row.right() - 10.0 - text_left,
    );
    let at_text = egui::pos2(text_left, row.center().y - galley.size().y / 2.0);
    ui.painter().galley(at_text, galley, colors.ink);
}

/// How wide each tab is in the tab row (browser.md §4 *Tabs*).
struct TabWidths {
    shown: f32,
    other: f32,
}

impl TabWidths {
    /// Equal shares while each keeps 80 pt, else the shown tab titled and the
    /// others badges.
    fn fitting(room: f32, count: usize) -> Self {
        let count = count as f32;
        let share = ((room - TAB_GAP * (count - 1.0)) / count).floor();
        if share >= TAB_TITLED_MIN_WIDTH {
            let width = share.min(TAB_MAX_WIDTH);
            return Self {
                shown: width,
                other: width,
            };
        }
        Self {
            shown: TAB_SHOWN_WIDTH,
            other: TAB_BADGE_WIDTH,
        }
    }
}

/// What the tab row draws with.
struct TabRow<'a> {
    strip: &'a TabStrip,
    chrome: &'a BrowserChrome<'a>,
    colors: &'a BarColors,
}

/// The pane's tabs under the toolbar, in order, the shown one raised, then New
/// tab; they scroll sideways when they do not fit, the shown one kept in view.
fn tab_row(ui: &mut egui::Ui, row: &TabRow<'_>, area: egui::Rect) -> Option<BrowserIntent> {
    let strip = row.strip;
    let tabs = egui::Rect::from_min_size(
        area.min + egui::vec2(BAR_PAD_X, 0.0),
        egui::vec2(area.width() - 2.0 * BAR_PAD_X, TAB_HEIGHT),
    );
    let new_tab_room = TAB_GAP + BUTTON_SIZE;
    let widths = TabWidths::fitting(tabs.width() - new_tab_room, strip.tabs.len());
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(tabs)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row_ui.shrink_clip_rect(area);
    // Its fade takes the panel's color, not the bar's tint.
    row_ui.spacing_mut().scroll.fade.strength = 0.0;
    let shown_id = row_ui.id().with("shown_tab");
    let newly_shown = row_ui.data_mut(|data| {
        let last = data.get_temp::<usize>(shown_id);
        data.insert_temp(shown_id, strip.active);
        last != Some(strip.active)
    });
    let picked = egui::ScrollArea::horizontal()
        .id_salt("tab_row")
        .max_width(tabs.width() - new_tab_room)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut row_ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(TAB_GAP, 0.0);
            let mut intent = None;
            for (index, summary) in strip.tabs.iter().enumerate() {
                let shown = index == strip.active;
                let width = if shown { widths.shown } else { widths.other };
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::hover());
                if shown && newly_shown {
                    ui.scroll_to_rect(rect, None);
                }
                let tab = Tab {
                    index,
                    summary,
                    rect,
                    shown,
                };
                intent = intent.or_else(|| tab_chip(ui, row, &tab));
            }
            intent
        })
        .inner;
    row_ui.add_space(TAB_GAP);
    let new_tab = new_tab_button(&mut row_ui, row.colors).then_some(BrowserIntent::NewTab);
    picked.or(new_tab)
}

struct Tab<'a> {
    index: usize,
    summary: &'a TabSummary,
    rect: egui::Rect,
    shown: bool,
}

/// One tab of the row: a click shows it; its `×` (titled: on hover, always on
/// the shown tab) or a middle click closes it.
fn tab_chip(ui: &mut egui::Ui, row: &TabRow<'_>, tab: &Tab<'_>) -> Option<BrowserIntent> {
    let (rect, colors) = (tab.rect, row.colors);
    let label = tab.summary.label();
    let titled = rect.width() >= TAB_TITLED_MIN_WIDTH;
    let response = ui
        .interact(rect, ui.id().with(("tab", tab.index)), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label);
    let hovered = ui.rect_contains_pointer(rect);
    let radius = egui::CornerRadius::same(TAB_RADIUS);
    if tab.shown {
        colors.pill.paint(ui.painter(), rect, radius);
    } else if hovered {
        ui.painter().rect_filled(rect, radius, colors.button_hover);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, tab.shown, label)
    });
    let badge_x = if titled {
        rect.left() + 8.0 + TAB_BADGE_SIZE / 2.0
    } else {
        rect.center().x
    };
    paint_tab_badge(
        ui.painter(),
        row.chrome.palette,
        &tab_badge(
            row.chrome,
            egui::pos2(badge_x, rect.center().y),
            tab.summary,
        ),
    );
    if titled {
        let crossed = hovered || tab.shown;
        let close_room = if crossed { TAB_CLOSE_SIZE } else { 0.0 };
        let text_left = badge_x + TAB_BADGE_SIZE / 2.0 + 7.0;
        let ink = if tab.shown || hovered {
            colors.ink
        } else {
            colors.secondary
        };
        let width = (rect.right() - 8.0 - close_room - text_left).max(0.0);
        let title = elided(ui.painter(), label, TAB_TITLE_SIZE, ink, width);
        paint_from_left(ui.painter(), title, egui::pos2(text_left, rect.center().y));
        let cross = TabCross {
            rect: egui::Rect::from_center_size(
                egui::pos2(rect.right() - 4.0 - TAB_CLOSE_SIZE / 2.0, rect.center().y),
                egui::vec2(TAB_CLOSE_SIZE, TAB_CLOSE_SIZE),
            ),
            label: &format!("Close {label}"),
        };
        let closed = crossed
            && ui
                .push_id(("tab_close", tab.index), |ui| {
                    close_button(ui, colors, &cross)
                })
                .inner;
        if closed {
            return Some(BrowserIntent::CloseTab(tab.index));
        }
    }
    if response.middle_clicked() {
        return Some(BrowserIntent::CloseTab(tab.index));
    }
    (response.clicked() && !tab.shown).then_some(BrowserIntent::SelectTab(tab.index))
}

fn tab_badge<'a>(
    chrome: &BrowserChrome<'_>,
    center: egui::Pos2,
    summary: &'a TabSummary,
) -> Badge<'a> {
    let site = url::display_parts(&summary.url).0;
    Badge {
        center,
        size: TAB_BADGE_SIZE,
        site,
        name: if summary.title.trim().is_empty() {
            site
        } else {
            &summary.title
        },
        server: false,
        icon: chrome.site_icon(&summary.url),
    }
}

/// A tab's badge: its host's, or a globe in a grey badge while it has no page.
fn paint_tab_badge(painter: &egui::Painter, palette: &Palette, badge: &Badge<'_>) {
    if badge.site.is_empty() {
        painter.circle_filled(badge.center, badge.size / 2.0, palette.text_muted);
        crate::ui::paint_icon(
            painter,
            badge.center,
            badge.size * 0.6,
            Icon::Globe,
            egui::Color32::WHITE,
        );
        return;
    }
    paint_badge(painter, palette, badge);
}

struct TabCross<'a> {
    rect: egui::Rect,
    label: &'a str,
}

/// A `×` closing a tab; `true` when clicked.
fn close_button(ui: &mut egui::Ui, colors: &BarColors, cross: &TabCross<'_>) -> bool {
    let rect = cross.rect;
    let response = ui
        .interact(rect, ui.id().with("close_tab"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Close tab");
    let tint = if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(5), colors.button_hover);
        colors.ink
    } else {
        colors.muted
    };
    crate::ui::paint_icon(ui.painter(), rect.center(), 12.0, Icon::X, tint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, cross.label));
    response.clicked()
}

/// `galley` from `left_center`, vertically centered on it.
fn paint_from_left(
    painter: &egui::Painter,
    galley: std::sync::Arc<egui::Galley>,
    left_center: egui::Pos2,
) {
    let at = egui::pos2(left_center.x, left_center.y - galley.size().y / 2.0);
    painter.galley(at, galley, egui::Color32::PLACEHOLDER);
}

fn lit_row(colors: &BarColors) -> egui::Color32 {
    colors.pill.fill.lerp_to_gamma(colors.ink, 0.07)
}

fn toolbar_button(
    ui: &mut egui::Ui,
    colors: &BarColors,
    button: &ToolButton<'_>,
) -> egui::Response {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(BUTTON_SIZE, BUTTON_SIZE), button.enabled);
    let fill = match (button.active, hovered) {
        (true, _) => Some(colors.accent_subtle),
        (false, true) => Some(colors.button_hover),
        (false, false) => None,
    };
    if let Some(fill) = fill {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), fill);
    }
    let tint = match (button.enabled, button.active, hovered) {
        (false, _, _) => colors.disabled,
        (true, true, _) => colors.accent,
        (true, false, true) => colors.ink,
        (true, false, false) => colors.secondary,
    };
    crate::ui::paint_icon(ui.painter(), rect.center(), ICON_SIZE, button.icon, tint);
    let label = button.label.as_ref();
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            button.enabled,
            button.active,
            label,
        )
    });
    response.on_hover_text(label)
}

/// The ghost Browser button centered over a fresh terminal (browser.md §3): what
/// it opens and its shortcut. `None` when the pane is too small to show it.
pub fn browser_offer(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    pane_rect: egui::Rect,
) -> Option<egui::Response> {
    if pane_rect.width() < OFFER_SIZE.x || pane_rect.height() < OFFER_SIZE.y {
        return None;
    }
    let palette = chrome.palette;
    let rect = egui::Rect::from_center_size(pane_rect.center(), OFFER_SIZE);
    let response = ui
        .interact(rect, ui.id().with("browser_offer"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    let ink = |color: egui::Color32| {
        if hovered {
            color
        } else {
            color.gamma_multiply(OFFER_REST_OPACITY)
        }
    };
    let painter = ui.painter();
    let fill = if hovered {
        with_alpha(palette.bg_surface, 230)
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect(
        rect,
        egui::CornerRadius::same(OFFER_RADIUS),
        fill,
        egui::Stroke::new(1.0_f32, ink(palette.border_input)),
        egui::StrokeKind::Inside,
    );
    let icon_center = egui::pos2(rect.center().x, rect.top() + 18.0 + OFFER_ICON_SIZE / 2.0);
    let glyph = if hovered {
        palette.accent
    } else {
        ink(palette.text_muted)
    };
    crate::ui::paint_icon(painter, icon_center, OFFER_ICON_SIZE, Icon::Globe, glyph);
    let mut y = icon_center.y + OFFER_ICON_SIZE / 2.0 + 10.0;
    let title = painter.layout_no_wrap(
        "Open browser".to_owned(),
        egui::FontId::proportional(OFFER_TITLE_SIZE),
        ink(palette.text_secondary),
    );
    y += paint_centered(painter, title, rect.center().x, y) + 3.0;
    let detail = match chrome.server {
        Some(server) => {
            let (host, path) = url::display_parts(server);
            painter.layout_no_wrap(
                format!("{host}{path}"),
                egui::FontId::monospace(OFFER_DETAIL_SIZE),
                ink(palette.text_muted),
            )
        }
        None => painter.layout_no_wrap(
            "No dev server yet".to_owned(),
            egui::FontId::proportional(OFFER_DETAIL_SIZE),
            ink(palette.text_muted),
        ),
    };
    y += paint_centered(painter, detail, rect.center().x, y) + 8.0;
    let shortcut = painter.layout_no_wrap(
        OFFER_SHORTCUT.display(),
        egui::FontId::proportional(OFFER_DETAIL_SIZE),
        ink(palette.text_muted),
    );
    paint_centered(painter, shortcut, rect.center().x, y);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open browser"));
    Some(response)
}

/// `⌘↩` pressed this frame, consumed so nothing else acts on it — read only while
/// the shell under the Browser button has the keyboard (browser.md §3).
pub fn offer_shortcut_pressed(ui: &egui::Ui) -> bool {
    ui.input_mut(|input| {
        let mut pressed = false;
        input.events.retain(|event| {
            let hit = matches!(
                event,
                egui::Event::Key { key, pressed: true, modifiers, .. }
                    if OFFER_SHORTCUT.matches(*key, *modifiers)
            );
            pressed |= hit;
            !hit
        });
        pressed
    })
}

/// Paints `galley` centered on `center_x` from `top`; its height.
fn paint_centered(
    painter: &egui::Painter,
    galley: std::sync::Arc<egui::Galley>,
    center_x: f32,
    top: f32,
) -> f32 {
    let size = galley.size();
    painter.galley(
        egui::pos2(center_x - size.x / 2.0, top),
        galley,
        egui::Color32::PLACEHOLDER,
    );
    size.y
}
