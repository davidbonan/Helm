//! The browser sidebar (specs/browser.md): each worktree's tabs, their native
//! views placed on the panel, and the favorites, history and imports they share.

use std::collections::HashMap;
use std::path::Path;

use crossbeam_channel::Receiver;

use crate::browser::freeze::{Cover, FreezeStep, PageFreeze};
use crate::browser::import::{ImportError, ImportReport, ImportSource, ImportStatus, Imported};
use crate::browser::page_view::ZoomChange;
use crate::browser::pip::PipFrame;
use crate::browser::split::{Side, SplitView, TabId};
use crate::browser::{url, Navigation, PageSnapshot, PageState, ViewRect, WebEngine, WebView};
use crate::ui::browser_full_window::{browser_full_window, FullWindowStage, FullWindowView};
use crate::ui::browser_panel::{
    browser_panel, AddressBar, BrowserChrome, BrowserIntent, BrowserPanelOutput, BrowserPanelView,
    BrowserTabState, PageCorners, TabStrip, TabSummary,
};
use crate::ui::browser_pip::{browser_pip, panel_rect, BrowserPipView, PipPlacement};
use crate::ui::browser_split::SplitPartner;

use super::git_session::RepoKey;
use super::keys::{consume_action, consume_zoom_commands, ZoomCommand};
use super::{HelmApp, Page, RunIntent, TerminalState};
use crate::keybindings::Action;
use crate::persistence::Prefs;
use crate::tab_drop::TabDrop;
use crate::ui::run_panel::RunPanelAction;

/// Lines of the Run strip scanned for the server's URL (browser.md §5).
const SERVER_OUTPUT_LINES: usize = 400;
const DISK_ACCESS_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// One worktree's browser: its tabs, shown in the browser sidebar.
pub(crate) struct WorktreeBrowser {
    tabs: Vec<BrowserTab>,
    /// The tab shown, its page on the panel.
    active: usize,
    /// The sidebar opened, the toolbar was pressed or another tab came to show:
    /// the shown page takes the keyboard once its view exists.
    takes_keyboard: bool,
    /// The page area drawn this frame, in egui points.
    placed: Option<egui::Rect>,
    /// How the drawn page view's corners round.
    placed_corners: PageCorners,
    /// The sidebar's resize grab reaches up to this x this frame, in egui points.
    grab_edge: Option<f32>,
    /// The panel's own suggestions fell over the page this frame.
    overlaid: bool,
    /// The shown page's dialog was drawn over it this frame.
    blocked: bool,
    /// A field of the panel, or the page's dialog, held the keyboard when last drawn.
    field_focused: bool,
    /// Drawn this frame.
    drawn: bool,
    /// Over the whole window (browser.md §15); not kept across launches.
    pub(crate) full_window: bool,
    /// Where the sidebar or the floating panel last drew it: what full window
    /// grows out of and back into.
    home: Option<egui::Rect>,
    /// Two tabs side by side (browser.md §16); not kept across launches.
    split: Option<SplitView>,
    /// The partner's page area drawn this frame.
    placed_partner: Option<egui::Rect>,
    /// The partner's dialog was drawn over it this frame.
    partner_blocked: bool,
    next_tab_id: TabId,
}

/// One page of a worktree's browser.
struct BrowserTab {
    id: TabId,
    view: Option<Box<dyn WebView>>,
    /// Loaded once the view exists.
    pending: Option<String>,
    state: BrowserTabState,
    freeze: PageFreeze,
    /// The last page counted in the history (browser.md §8).
    visited: Option<String>,
    /// The last icon the page named, handed to the site icons.
    icon_seen: Option<String>,
    /// The zoom the view was given; a new view starts at 100 %.
    applied_zoom: f64,
    /// A find was sent and the engine has not answered yet.
    finding: bool,
}

impl BrowserTab {
    /// A tab opening on `url`; none ⇒ the start page, the URL field focused
    /// (browser.md §5, §7).
    fn opening(id: TabId, url: Option<String>) -> Self {
        let state = BrowserTabState {
            address: AddressBar {
                focus_requested: url.is_none(),
                ..AddressBar::default()
            },
            ..BrowserTabState::default()
        };
        Self {
            pending: url,
            state,
            ..Self::new(id)
        }
    }

    /// A tab on a page another one opened, already loading (browser.md §2).
    fn adopting(id: TabId, view: Box<dyn WebView>) -> Self {
        Self {
            view: Some(view),
            ..Self::new(id)
        }
    }

    fn new(id: TabId) -> Self {
        Self {
            id,
            view: None,
            state: BrowserTabState::default(),
            pending: None,
            freeze: PageFreeze::default(),
            visited: None,
            icon_seen: None,
            applied_zoom: 1.0,
            finding: false,
        }
    }

    fn has_native_focus(&self) -> bool {
        self.view.as_ref().is_some_and(|view| view.is_focused())
    }

    fn page(&self) -> PageState {
        match &self.view {
            Some(view) => view.page(),
            None => PageState {
                url: self.pending.clone().unwrap_or_default(),
                ..PageState::default()
            },
        }
    }

    fn load(&mut self, url: String) {
        self.state.start_page = false;
        match self.view.as_mut() {
            Some(view) => {
                view.load(&url);
                view.focus();
            }
            None => self.pending = Some(url),
        }
    }

    /// The page this frame, once loaded, when the history has not counted it yet.
    fn new_visit(&mut self, page: &PageState) -> Option<(String, String)> {
        if self.view.is_none() || page.loading || page.url.is_empty() {
            return None;
        }
        if self.visited.as_deref() == Some(page.url.as_str()) {
            return None;
        }
        self.visited = Some(page.url.clone());
        Some((page.url.clone(), page.title.clone()))
    }

    /// The page and the icon it names, when it names a new one.
    fn new_icon(&mut self, page: &PageState) -> Option<(String, String)> {
        let icon = page.icon_url.as_ref()?;
        if self.icon_seen.as_ref() == Some(icon) {
            return None;
        }
        self.icon_seen = Some(icon.clone());
        Some((page.url.clone(), icon.clone()))
    }

    fn summary(&self, page: PageState) -> TabSummary {
        TabSummary {
            title: page.title,
            url: page.url,
            loading: page.loading,
        }
    }

    /// Shows the view on `placed`, or hides it behind its snapshot while egui
    /// draws over it (browser.md §4); the view is made on the first frame it has
    /// a page to show.
    fn sync_view(&mut self, engine: Option<&dyn WebEngine>, sync: &ViewSync<'_>) {
        if self.view.is_none() {
            let (Some(engine), Cover::None, Some(_)) = (engine, sync.cover, self.pending.as_ref())
            else {
                return;
            };
            self.view = Some(engine.open());
        }
        let Some(view) = self.view.as_mut() else {
            return;
        };
        if let Some(url) = self.pending.take() {
            view.load(&url);
            if sync.focused {
                view.focus();
            }
        }
        let ctx = sync.ctx;
        let zoom = self.state.page_view.zoom;
        if zoom != self.applied_zoom {
            view.set_zoom(zoom);
            self.applied_zoom = zoom;
        }
        if self.finding {
            match view.take_find_result() {
                Some(matched) => {
                    self.finding = false;
                    if let Some(find) = self.state.find.as_mut() {
                        find.no_match = !matched;
                    }
                }
                None => ctx.request_repaint(),
            }
        }
        let snapshot = view.take_snapshot();
        let snapshot_in = snapshot.is_some();
        if let Some(snapshot) = snapshot {
            self.state.backdrop = Some(backdrop_texture(ctx, snapshot));
        }
        let rect = sync.placed.map(|rect| view_rect(rect, sync));
        match self
            .freeze
            .step(sync.cover, snapshot_in, std::time::Instant::now())
        {
            FreezeStep::Show => view.place(rect),
            FreezeStep::Snapshot => {
                view.request_snapshot();
                view.place(rect);
                ctx.request_repaint();
            }
            FreezeStep::Wait => {
                view.place(rect);
                ctx.request_repaint();
            }
            FreezeStep::Hide { pictured } => {
                if !pictured {
                    self.state.backdrop = None;
                }
                view.place(None);
            }
            FreezeStep::Leave => {
                view.request_snapshot();
                view.place(None);
                ctx.request_repaint();
            }
        }
    }
}

/// What a tab's view follows this frame.
struct ViewSync<'a> {
    cover: Cover,
    placed: Option<egui::Rect>,
    corners: PageCorners,
    grab_edge: Option<f32>,
    /// A first load takes the keyboard.
    focused: bool,
    ctx: &'a egui::Context,
}

impl WorktreeBrowser {
    /// A browser opening on `url` (browser.md §5), taking the keyboard.
    pub(crate) fn opening(url: Option<String>) -> Self {
        Self {
            tabs: vec![BrowserTab::opening(0, url)],
            next_tab_id: 1,
            split: None,
            placed_partner: None,
            partner_blocked: false,
            active: 0,
            takes_keyboard: true,
            placed: None,
            placed_corners: PageCorners::default(),
            grab_edge: None,
            overlaid: false,
            blocked: false,
            field_focused: false,
            drawn: false,
            full_window: false,
            home: None,
        }
    }

    fn shown(&mut self) -> &mut BrowserTab {
        &mut self.tabs[self.active]
    }

    fn new_tab_id(&mut self) -> TabId {
        self.next_tab_id += 1;
        self.next_tab_id - 1
    }

    /// The shown tab's split partner: its index and the side it shows on.
    fn partner(&self) -> Option<(usize, Side)> {
        let (id, side) = self.split?.partner_of(self.tabs[self.active].id)?;
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        Some((index, side))
    }

    /// The tabs in order, the split's two marked.
    fn tab_strip(&self, pages: &[PageState]) -> TabStrip {
        let index_of = |id: TabId| self.tabs.iter().position(|tab| tab.id == id);
        TabStrip {
            tabs: self
                .tabs
                .iter()
                .zip(pages.iter().cloned())
                .map(|(tab, page)| tab.summary(page))
                .collect(),
            active: self.active,
            pair: self
                .split
                .and_then(|split| Some((index_of(split.left)?, index_of(split.right)?))),
        }
    }

    /// The shown tab's state, and its split partner's beside it.
    fn shown_and_partner<'a>(
        &'a mut self,
        pages: &'a [PageState],
    ) -> (&'a mut BrowserTabState, Option<SplitPartner<'a>>) {
        let active = self.active;
        let share = self.split.map_or(0.5, |split| split.share);
        let Some((index, side)) = self.partner() else {
            return (&mut self.tabs[active].state, None);
        };
        let (shown, partner) = pair_mut(&mut self.tabs, active, index);
        let partner = SplitPartner {
            page: &pages[index],
            tab: &mut partner.state,
            side,
            share,
        };
        (&mut shown.state, Some(partner))
    }

    /// Tab `index` joins the shown one, on its right and next to it in the row.
    fn split_with(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active {
            return;
        }
        let joining = self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.split = Some(SplitView::new(self.tabs[self.active].id, joining.id));
        self.tabs.insert(self.active + 1, joining);
        self.takes_keyboard = true;
    }

    pub(crate) fn load(&mut self, url: String) {
        self.shown().load(url);
    }

    #[cfg(test)]
    pub(crate) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    #[cfg(test)]
    pub(crate) fn shown_title(&self) -> String {
        self.tabs[self.active].page().title
    }

    /// A tab on `url` (the start page without one), after the shown one, shown in
    /// its place.
    fn open_tab(&mut self, url: Option<String>) {
        let tab = BrowserTab::opening(self.new_tab_id(), url);
        self.active += 1;
        self.tabs.insert(self.active, tab);
        self.takes_keyboard = true;
    }

    /// A tab on `url` after the shown one, left in the background: its page
    /// loads once shown.
    fn open_background_tab(&mut self, url: String) {
        let tab = BrowserTab::opening(self.new_tab_id(), Some(url));
        self.tabs.insert(self.active + 1, tab);
    }

    /// Closes tab `index`, ending its split; the one after it is shown in its
    /// place, else the one before. The last one gives way to a tab on the start
    /// page.
    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if self
            .split
            .is_some_and(|split| split.holds(self.tabs[index].id))
        {
            self.split = None;
        }
        if self.tabs.len() == 1 {
            self.tabs[0] = BrowserTab::opening(self.new_tab_id(), None);
            return;
        }
        self.tabs.remove(index);
        if index < self.active || self.active == self.tabs.len() {
            self.active -= 1;
        }
        self.takes_keyboard = true;
    }

    fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() && index != self.active {
            self.active = index;
            self.takes_keyboard = true;
        }
    }

    /// The pages the tabs' pages opened, each in a tab shown after its opener;
    /// a page that closed itself closes its tab (browser.md §2). The last tab
    /// closing comes back for the app.
    fn follow_pages(&mut self) -> Option<BrowserIntent> {
        let mut index = 0;
        while index < self.tabs.len() {
            let opened = self.tabs[index]
                .view
                .as_mut()
                .and_then(|view| view.take_opened());
            let Some(view) = opened else {
                index += 1;
                continue;
            };
            let tab = BrowserTab::adopting(self.new_tab_id(), view);
            self.tabs.insert(index + 1, tab);
            self.active = index + 1;
            self.takes_keyboard = true;
        }
        let closed = self
            .tabs
            .iter()
            .position(|tab| tab.view.as_ref().is_some_and(|view| view.page().closed))?;
        self.apply(BrowserIntent::CloseTab(closed))
    }

    /// The shown tab stays shown wherever it goes.
    fn move_tab(&mut self, drop: TabDrop) {
        if let Some(active) = drop.apply(&mut self.tabs, self.active) {
            self.active = active;
        }
    }

    fn has_native_focus(&self) -> bool {
        self.tabs[self.active].has_native_focus()
    }

    /// The page's focus is read live: a click on it runs no egui frame.
    fn has_keyboard(&self) -> bool {
        self.field_focused || self.has_native_focus()
    }

    /// Applies the browser's own intent; the shared ones (favorites, import, server,
    /// closing the last tab) come back for the app.
    fn apply(&mut self, intent: BrowserIntent) -> Option<BrowserIntent> {
        self.apply_on(self.active, intent)
    }

    /// `intent` from tab `index`'s page: the shown tab's, or its partner's.
    fn apply_on(&mut self, index: usize, intent: BrowserIntent) -> Option<BrowserIntent> {
        let loads = matches!(intent, BrowserIntent::Load(_) | BrowserIntent::Open(_));
        let last_tab = self.tabs.len() == 1;
        let tab = &mut self.tabs[index];
        match intent {
            BrowserIntent::Load(typed) => {
                if let Some(url) = url::typed_url(&typed) {
                    tab.load(url);
                }
            }
            BrowserIntent::Open(url) => tab.load(url),
            BrowserIntent::ToggleStartPage => tab.state.start_page = !tab.state.start_page,
            BrowserIntent::Navigate(navigation) => {
                tab.state.start_page = false;
                if let Some(view) = tab.view.as_mut() {
                    view.navigate(navigation);
                }
            }
            BrowserIntent::Inspect => {
                if let Some(view) = tab.view.as_mut() {
                    view.show_inspector();
                }
            }
            BrowserIntent::Find { text, backwards } => {
                if let Some(view) = tab.view.as_mut() {
                    view.find(&text, backwards);
                    tab.finding = true;
                }
            }
            BrowserIntent::FocusPage => {
                if let Some(view) = tab.view.as_mut() {
                    view.focus();
                }
            }
            BrowserIntent::OpenInChrome => open_in_chrome(&tab.page().url),
            BrowserIntent::AnswerDialog(answer) => {
                if let Some(view) = tab.view.as_mut() {
                    view.answer_dialog(answer);
                }
                self.takes_keyboard = true;
            }
            BrowserIntent::RevealDownload(path) => super::reveal_in_finder(Some(&path)),
            BrowserIntent::NewTab => self.open_tab(None),
            BrowserIntent::OpenInNewTab(url) => self.open_background_tab(url),
            BrowserIntent::SelectTab(index) => self.select_tab(index),
            BrowserIntent::MoveTab(drop) => self.move_tab(drop),
            BrowserIntent::CloseTab(index) if last_tab => {
                self.close_tab(index);
                return Some(intent);
            }
            BrowserIntent::CloseTab(index) => self.close_tab(index),
            BrowserIntent::SplitWith(index) => self.split_with(index),
            BrowserIntent::Unsplit => self.split = None,
            BrowserIntent::ResizeSplit(share) => {
                if let Some(split) = self.split.as_mut() {
                    split.share = share;
                }
            }
            BrowserIntent::ShowPartner => {
                if let Some((partner, _)) = self.partner() {
                    self.select_tab(partner);
                }
            }
            shared => return Some(shared),
        }
        self.takes_keyboard |= loads;
        None
    }

    /// Places the shown tab's view on this frame's page area, frozen while egui
    /// draws over it; the other tabs' views stay hidden (browser.md §4).
    fn sync_views(&mut self, engine: Option<&dyn WebEngine>, covered: bool, ctx: &egui::Context) {
        if !std::mem::take(&mut self.drawn) {
            self.field_focused = false;
        }
        let covered = covered || std::mem::take(&mut self.overlaid);
        let grab_edge = self.grab_edge.take();
        let shown = Placement {
            rect: self.placed.take(),
            blocked: std::mem::take(&mut self.blocked),
        };
        let partner = Placement {
            rect: self.placed_partner.take(),
            blocked: std::mem::take(&mut self.partner_blocked),
        };
        let partner_index = self.partner().map(|(index, _)| index);
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let placement = match index {
                _ if index == self.active => shown,
                _ if Some(index) == partner_index => partner,
                _ => Placement::AWAY,
            };
            let sync = ViewSync {
                cover: placement.cover(covered),
                placed: placement.rect,
                corners: self.placed_corners,
                grab_edge,
                focused: index == self.active && self.takes_keyboard,
                ctx,
            };
            tab.sync_view(engine, &sync);
        }
        if self.tabs[self.active].view.is_some() {
            self.takes_keyboard = false;
        }
    }
}

/// Where a tab's page was drawn this frame, if anywhere.
#[derive(Clone, Copy)]
struct Placement {
    rect: Option<egui::Rect>,
    /// Its dialog lies over it.
    blocked: bool,
}

impl Placement {
    const AWAY: Self = Self {
        rect: None,
        blocked: false,
    };

    fn cover(self, covered: bool) -> Cover {
        match self.rect {
            None => Cover::Away,
            Some(_) if self.blocked => Cover::Blocked,
            Some(_) if covered => Cover::Overlay,
            Some(_) => Cover::None,
        }
    }
}

/// Two distinct items of `items`, mutably.
fn pair_mut<T>(items: &mut [T], first: usize, second: usize) -> (&mut T, &mut T) {
    if first < second {
        let (head, tail) = items.split_at_mut(second);
        (&mut head[first], &mut tail[0])
    } else {
        let (head, tail) = items.split_at_mut(first);
        (&mut tail[0], &mut head[second])
    }
}

fn backdrop_texture(ctx: &egui::Context, snapshot: PageSnapshot) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_premultiplied(
        [snapshot.width, snapshot.height],
        &snapshot.rgba_premultiplied,
    );
    ctx.load_texture("browser-snapshot", image, egui::TextureOptions::LINEAR)
}

fn view_rect(rect: egui::Rect, sync: &ViewSync<'_>) -> ViewRect {
    let zoom = f64::from(sync.ctx.zoom_factor());
    let corners = sync.corners;
    let pass_through = sync
        .grab_edge
        .map_or(0.0, |edge| (edge - rect.left()).clamp(0.0, rect.width()));
    ViewRect {
        x: f64::from(rect.left()) * zoom,
        y: f64::from(rect.top()) * zoom,
        width: f64::from(rect.width()) * zoom,
        height: f64::from(rect.height()) * zoom,
        radius: f64::from(corners.radius) * zoom,
        rounds_top: corners.rounds_top,
        pass_through_left: f64::from(pass_through) * zoom,
    }
}

fn open_in_chrome(url: &str) {
    if url.is_empty() {
        return;
    }
    let in_chrome = std::process::Command::new("open")
        .args(["-a", "Google Chrome", url])
        .status()
        .is_ok_and(|status| status.success());
    if !in_chrome {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

/// What the browser sidebar reported this frame.
pub(crate) struct BrowserOutput {
    /// For the app: favorites, import, Dev server.
    pub(crate) intents: Vec<BrowserIntent>,
    /// Pages to count in the history: URL, title.
    pub(crate) visits: Vec<(String, String)>,
    /// Icons the pages named: page URL, icon URL.
    pub(crate) icons: Vec<(String, String)>,
}

/// The worktree's browser in its sidebar (browser.md §4).
pub(crate) fn render_browser_sidebar(
    ui: &mut egui::Ui,
    browser: &mut WorktreeBrowser,
    chrome: &BrowserChrome<'_>,
) -> BrowserOutput {
    let panel = ui.available_rect_before_wrap();
    browser.home = Some(panel);
    let closing = browser.follow_pages();
    let reports = PageReports::of(browser);
    let strip = browser.tab_strip(&reports.pages);
    let page = &reports.pages[browser.active];
    let (tab, partner) = browser.shown_and_partner(&reports.pages);
    let view = BrowserPanelView {
        page,
        tab,
        strip: &strip,
        partner,
    };
    let output = browser_panel(ui, chrome, view);
    let mut intents = browser.take_in(ui, panel, output);
    browser.grab_edge = Some(panel.left() + crate::ui::BROWSER_RESIZE_GRAB);
    intents.extend(closing);
    reports.with(intents)
}

/// The worktree's browser floating over the window (browser.md §11); where its
/// panel now floats.
pub(crate) fn render_browser_pip(
    ui: &mut egui::Ui,
    browser: &mut WorktreeBrowser,
    chrome: &BrowserChrome<'_>,
    placement: PipPlacement,
) -> (BrowserOutput, PipFrame) {
    let closing = browser.follow_pages();
    let reports = PageReports::of(browser);
    let active = browser.active;
    let view = BrowserPipView {
        page: &reports.pages[active],
        tab: &mut browser.tabs[active].state,
        placement,
    };
    let output = browser_pip(ui, chrome, view);
    browser.home = Some(panel_rect(output.frame));
    let mut intents = browser.take_in(ui, panel_rect(output.frame), output.panel);
    intents.extend(closing);
    (reports.with(intents), output.frame)
}

/// The worktree's browser over the whole window (browser.md §15), and how far
/// it has grown.
pub(crate) struct FullWindowDraw<'a, 'b> {
    pub(crate) browser: &'a mut WorktreeBrowser,
    pub(crate) stage: FullWindowStage<'b>,
}

impl WorktreeBrowser {
    /// Where full window grows out of: where the browser sat, else `fallback`.
    pub(crate) fn home_or(&self, fallback: egui::Rect) -> egui::Rect {
        self.home.unwrap_or(fallback)
    }
}

pub(crate) fn render_browser_full_window(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    draw: FullWindowDraw<'_, '_>,
) -> BrowserOutput {
    let FullWindowDraw { browser, stage } = draw;
    let closing = browser.follow_pages();
    let reports = PageReports::of(browser);
    let strip = browser.tab_strip(&reports.pages);
    let page = &reports.pages[browser.active];
    let (tab, partner) = browser.shown_and_partner(&reports.pages);
    let view = FullWindowView {
        panel: BrowserPanelView {
            page,
            tab,
            strip: &strip,
            partner,
        },
        stage,
    };
    let output = browser_full_window(ui, chrome, view);
    let mut intents = browser.take_in(ui, output.frame, output.panel);
    intents.extend(closing);
    reports.with(intents)
}

/// What the tabs' pages tell this frame.
struct PageReports {
    pages: Vec<PageState>,
    visits: Vec<(String, String)>,
    icons: Vec<(String, String)>,
}

impl PageReports {
    fn of(browser: &mut WorktreeBrowser) -> Self {
        let pages: Vec<PageState> = browser.tabs.iter().map(BrowserTab::page).collect();
        let visits = browser
            .tabs
            .iter_mut()
            .zip(&pages)
            .filter_map(|(tab, page)| tab.new_visit(page))
            .collect();
        let icons = browser
            .tabs
            .iter_mut()
            .zip(&pages)
            .filter_map(|(tab, page)| tab.new_icon(page))
            .collect();
        Self {
            pages,
            visits,
            icons,
        }
    }

    fn with(self, intents: Vec<BrowserIntent>) -> BrowserOutput {
        BrowserOutput {
            intents,
            visits: self.visits,
            icons: self.icons,
        }
    }
}

impl WorktreeBrowser {
    /// Where the page goes and who holds the keyboard, from what the panel drew;
    /// its own intents applied, the shared ones returned.
    fn take_in(
        &mut self,
        ui: &egui::Ui,
        panel: egui::Rect,
        output: BrowserPanelOutput,
    ) -> Vec<BrowserIntent> {
        let partner = self.partner().map(|(index, _)| index);
        let partner_output = output.partner.filter(|_| partner.is_some());
        self.placed = output.page_rect;
        self.placed_partner = partner_output.as_ref().and_then(|half| half.page_rect);
        self.partner_blocked = partner_output.as_ref().is_some_and(|half| half.blocked);
        self.placed_corners = output.page_corners;
        self.overlaid = output.page_overlaid;
        self.blocked = output.page_blocked;
        if let Some(index) = partner.filter(|&index| self.tabs[index].has_native_focus()) {
            self.active = index;
            std::mem::swap(&mut self.placed, &mut self.placed_partner);
            std::mem::swap(&mut self.blocked, &mut self.partner_blocked);
        }
        let native_focus = self.has_native_focus();
        let dialog_keys = output.page_blocked && !output.field_focused;
        hold_keyboard_for_page(ui, panel, native_focus || dialog_keys);
        if output.clicked {
            self.takes_keyboard = true;
        }
        if output.field_focused {
            self.takes_keyboard = false;
        }
        if self.takes_keyboard && !native_focus {
            if let Some(view) = self.shown().view.as_mut() {
                view.focus();
            }
        }
        let partner_focused = partner_output
            .as_ref()
            .is_some_and(|half| half.field_focused || half.blocked);
        self.field_focused = output.field_focused || output.page_blocked || partner_focused;
        self.drawn = true;
        let mut shared: Vec<BrowserIntent> = output
            .intents
            .into_iter()
            .filter_map(|intent| self.apply(intent))
            .collect();
        if let (Some(index), Some(half)) = (partner, partner_output) {
            let partner_shared = half
                .intents
                .into_iter()
                .filter_map(|intent| self.apply_on(index, intent));
            shared.extend(partner_shared.collect::<Vec<_>>());
        }
        shared
    }
}

/// While the page (or its dialog) holds the keyboard, egui's focus rests on the
/// panel: no field keeps a caret behind the page and the shell does not claim
/// the keyboard.
fn hold_keyboard_for_page(ui: &egui::Ui, panel: egui::Rect, holds: bool) {
    let holder = ui.interact(
        panel,
        ui.id().with("browser_page_keyboard"),
        egui::Sense::focusable_noninteractive(),
    );
    if holds {
        holder.request_focus();
    } else if holder.has_focus() {
        holder.surrender_focus();
    }
}

/// The worktree's default page (browser.md §5), from its Run strip and port.
pub(crate) fn server_url_from(
    run_panes: &HashMap<RepoKey, TerminalState>,
    key: &RepoKey,
    port: Option<u16>,
) -> Option<String> {
    let printed = match run_panes.get(key) {
        Some(TerminalState::Live(pane)) => url::server_url_in(&crate::terminal::emu::tail_text(
            &pane.grid().lock(),
            SERVER_OUTPUT_LINES,
        )),
        _ => None,
    };
    url::server_url(printed, port)
}

impl HelmApp {
    /// The project local favorites belong to (browser.md §7): the active entry's
    /// group root, as the render shows it.
    fn browser_project(&self) -> String {
        self.workspace
            .active()
            .and_then(|index| self.project_root_for_index(index))
            .map(|root| root.display().to_string())
            .unwrap_or_default()
    }

    pub(crate) fn server_url_of(&self, key: &RepoKey) -> Option<String> {
        let port = self.run_target_for(key).and_then(|t| t.port);
        server_url_from(&self.caches.run_panes, key, port)
    }

    /// The active worktree's browser, opened on its server page the first time
    /// (browser.md §3, §5).
    pub(crate) fn open_active_browser(&mut self) -> Option<&mut WorktreeBrowser> {
        let key = self.active_repo_key()?;
        if !self.caches.browsers.contains_key(&key) {
            let url = self.server_url_of(&key);
            self.caches
                .browsers
                .insert(key.clone(), WorktreeBrowser::opening(url));
        }
        self.caches.browsers.get_mut(&key)
    }

    pub(super) fn shows_browser(&self) -> bool {
        self.workspace
            .active_repo()
            .is_some_and(|repo| self.prefs.shows_browser(&repo.path))
    }

    /// The active worktree's browser floats over the window (browser.md §11).
    pub(super) fn browser_floats(&self) -> bool {
        self.workspace
            .active_repo()
            .is_some_and(|repo| self.prefs.browser_pip(&repo.path).floating)
    }

    /// `⌘⇧B` or the globe toggle, on the active worktree only; opening gives its
    /// page the keyboard (browser.md §3).
    pub(super) fn toggle_browser(&mut self) {
        let Some(worktree) = self.workspace.active_repo().map(|repo| repo.path.clone()) else {
            return;
        };
        self.persist(|mut prefs| {
            prefs.toggle_browser(&worktree);
            prefs
        });
        if !self.prefs.shows_browser(&worktree) {
            self.leave_browser_full_window();
            return;
        }
        if let Some(browser) = self.open_active_browser() {
            browser.takes_keyboard = true;
        }
    }

    /// A hidden browser shows again where it sat, not over the window.
    fn leave_browser_full_window(&mut self) {
        let browser = self
            .active_repo_key()
            .and_then(|key| self.caches.browsers.get_mut(&key));
        if let Some(browser) = browser {
            browser.full_window = false;
        }
    }

    /// A terminal Cmd+clicked URL (terminal.md §12): a new tab of the active
    /// worktree's browser, shown — on its own first tab if the browser had none.
    /// `false` without an active worktree.
    pub(super) fn open_in_browser(&mut self, url: String) -> bool {
        let Some(worktree) = self.workspace.active_repo().map(|repo| repo.path.clone()) else {
            return false;
        };
        let Some(key) = self.active_repo_key() else {
            return false;
        };
        self.persist(|mut prefs| {
            prefs.show_browser(&worktree);
            prefs
        });
        match self.caches.browsers.get_mut(&key) {
            Some(browser) => browser.open_tab(Some(url)),
            None => {
                self.caches
                    .browsers
                    .insert(key, WorktreeBrowser::opening(Some(url)));
            }
        }
        true
    }

    /// `⌘⇧↩` or the toolbar's Full window: the active worktree's browser over
    /// the whole window, shown first if hidden; or back (browser.md §15).
    pub(super) fn toggle_browser_full_window(&mut self) {
        let Some(worktree) = self.workspace.active_repo().map(|repo| repo.path.clone()) else {
            return;
        };
        if !self.prefs.shows_browser(&worktree) {
            self.persist(|mut prefs| {
                prefs.show_browser(&worktree);
                prefs
            });
        }
        if let Some(browser) = self.open_active_browser() {
            browser.full_window = !browser.full_window;
            browser.takes_keyboard = true;
        }
    }

    fn hide_browser(&mut self) {
        let Some(worktree) = self.workspace.active_repo().map(|repo| repo.path.clone()) else {
            return;
        };
        self.persist(|mut prefs| {
            prefs.hide_browser(&worktree);
            prefs
        });
        self.leave_browser_full_window();
    }

    /// The active worktree's browser onto the floating panel, or back into the
    /// sidebar (browser.md §11).
    fn edit_browser_pip(&mut self, edit: impl FnOnce(&mut Prefs, &Path)) {
        let Some(worktree) = self.workspace.active_repo().map(|repo| repo.path.clone()) else {
            return;
        };
        self.persist(|mut prefs| {
            edit(&mut prefs, &worktree);
            prefs
        });
    }

    pub(crate) fn move_browser_pip(&mut self, frame: PipFrame) {
        self.edit_browser_pip(|prefs, worktree| prefs.set_browser_pip_frame(worktree, frame));
    }

    /// The shared intents of the worktree's browser this frame.
    pub(crate) fn apply_browser_intents(
        &mut self,
        key: &RepoKey,
        intents: Vec<BrowserIntent>,
        ctx: &egui::Context,
    ) {
        let project = self.browser_project();
        for intent in intents {
            match intent {
                BrowserIntent::LoadServer => self.load_server_page(key),
                BrowserIntent::RunServer => self.run_server(key, ctx),
                BrowserIntent::ToggleFavorite { url, title } => {
                    self.browser_library
                        .edit(|library| library.toggle_favorite(&url, &title, &project));
                    self.browser_library.save();
                }
                BrowserIntent::RemoveFavorite(url) => {
                    self.browser_library
                        .edit(|library| library.remove_favorite(&url, &project));
                    self.browser_library.save();
                }
                BrowserIntent::RenameFavorite { url, title } => {
                    self.browser_library
                        .edit(|library| library.rename_favorite(&url, &project, &title));
                    self.browser_library.save();
                }
                BrowserIntent::CloseTab(_) => self.hide_browser(),
                BrowserIntent::Float => self.edit_browser_pip(Prefs::float_browser),
                BrowserIntent::ToggleFullWindow => self.toggle_browser_full_window(),
                BrowserIntent::Dock => self.edit_browser_pip(Prefs::dock_browser),
                BrowserIntent::DismissImport => self.browser_import.dismiss(),
                BrowserIntent::Import(source) => self.browser_import.start(source, ctx),
                BrowserIntent::OpenDiskAccessSettings => {
                    let _ = std::process::Command::new("open")
                        .arg(DISK_ACCESS_SETTINGS)
                        .spawn();
                }
                _ => {}
            }
        }
    }

    pub(crate) fn fetch_site_icons(&mut self, icons: Vec<(String, String)>, ctx: &egui::Context) {
        for (page_url, icon_url) in icons {
            self.site_icons.fetch(&page_url, &icon_url, ctx);
        }
    }

    /// Kept in memory: eframe's periodic `save` writes them.
    pub(crate) fn record_browser_visits(&mut self, visits: Vec<(String, String)>) {
        let now = unix_now();
        for (url, title) in visits {
            self.browser_library
                .edit(|library| library.record_visit(&url, &title, now));
        }
    }

    /// Takes in a finished import: favorites and history into the library,
    /// cookies into the engine (browser.md §9).
    pub(crate) fn finish_browser_import(&mut self, ctx: &egui::Context) {
        let Some((source, result)) = self.browser_import.finished() else {
            return;
        };
        self.browser_import.status = match result {
            Ok(imported) => ImportStatus::Done(self.take_in(source, imported, ctx)),
            Err(err) => ImportStatus::Failed(source, err),
        };
        self.browser_library.save();
    }

    fn take_in(
        &mut self,
        source: ImportSource,
        imported: Imported,
        ctx: &egui::Context,
    ) -> ImportReport {
        let history = imported.history.len();
        self.site_icons.take_in(imported.icons, ctx);
        let favorites = self.browser_library.edit(|library| {
            library.merge_history(imported.history);
            library.add_favorites(imported.favorites)
        });
        let cookies = self
            .web_engine
            .as_deref()
            .map_or(0, |engine| engine.import_cookies(&imported.cookies));
        ImportReport {
            source,
            favorites,
            history,
            cookies,
            cookies_skipped: imported.cookies_skipped,
        }
    }

    /// The failure page's Run server: the worktree's Run strip runs, shown as
    /// `⌘R` shows it; no command yet ⇒ its editor opens (git.md §3).
    fn run_server(&mut self, key: &RepoKey, ctx: &egui::Context) {
        let Some(target) = self.run_target_for(key) else {
            return;
        };
        self.sidebars.git = true;
        self.run_collapsed.insert(key.clone(), false);
        let action = if target.command.trim().is_empty() {
            RunPanelAction {
                begin_edit: true,
                ..RunPanelAction::default()
            }
        } else {
            RunPanelAction {
                run: true,
                ..RunPanelAction::default()
            }
        };
        let intent = RunIntent {
            key: target.key,
            cwd: target.path,
            root: target.root,
            command: target.command,
            launch_command: target.launch_command,
            port: target.port,
            action,
        };
        self.apply_run_intent(intent, ctx);
    }

    /// The toolbar's Server button: the default page again.
    fn load_server_page(&mut self, key: &RepoKey) {
        let Some(url) = self.server_url_of(key) else {
            return;
        };
        if let Some(browser) = self.caches.browsers.get_mut(key) {
            browser.load(url);
        }
    }

    /// While the browser holds the keyboard (browser.md §6), `Cmd+T` and `Cmd+W`
    /// act on its tabs, zoom on its page and `Cmd+F` opens its find bar (none on
    /// the floating panel), taken before helm sees them; `Cmd+W` on its last tab
    /// hides the browser.
    pub(crate) fn route_browser_tab_keys(&mut self, ctx: &egui::Context) {
        if !self.shows_browser() {
            return;
        }
        let floats = self.browser_floats();
        let Some(key) = self.active_repo_key() else {
            return;
        };
        let Some(browser) = self
            .caches
            .browsers
            .get_mut(&key)
            .filter(|browser| browser.has_keyboard())
        else {
            return;
        };
        if consume_action(ctx, &self.keymap, Action::NewTab) {
            browser.open_tab(None);
        }
        if consume_action(ctx, &self.keymap, Action::Run) {
            browser.apply(BrowserIntent::Navigate(Navigation::Reload));
        }
        let bypass_cache = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        if ctx.input_mut(|i| i.consume_key(bypass_cache, egui::Key::R)) {
            browser.apply(BrowserIntent::Navigate(Navigation::ReloadFromOrigin));
        }
        if consume_action(ctx, &self.keymap, Action::ClosePane)
            && browser
                .apply(BrowserIntent::CloseTab(browser.active))
                .is_some()
        {
            self.hide_browser();
            return;
        }
        let page_view = &mut browser.shown().state.page_view;
        for command in consume_zoom_commands(ctx, &self.keymap) {
            let change = match command {
                ZoomCommand::In => ZoomChange::In,
                ZoomCommand::Out => ZoomChange::Out,
                ZoomCommand::Reset => ZoomChange::Reset,
            };
            page_view.zoom = page_view.zoomed(change);
        }
        if !floats && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            browser.shown().state.open_find();
        }
    }

    /// Places every page view for this frame, frozen wherever egui draws above
    /// the sidebar (browser.md §4). Called once per frame, after every overlay.
    pub(crate) fn sync_browser_views(&mut self, ctx: &egui::Context) {
        let covered = self.page == Page::Preferences
            || self.modal.is_some()
            || egui::Popup::is_any_open(ctx)
            || egui::DragAndDrop::has_any_payload(ctx);
        let engine = self.web_engine.as_deref();
        for browser in self.caches.browsers.values_mut() {
            browser.sync_views(engine, covered, ctx);
        }
    }
}

/// The import running in the background, and what the start page tells of it.
#[derive(Default)]
pub(crate) struct BrowserImport {
    pub(crate) status: ImportStatus,
    running: Option<(ImportSource, Receiver<Result<Imported, ImportError>>)>,
}

impl BrowserImport {
    fn start(&mut self, source: ImportSource, ctx: &egui::Context) {
        self.start_with(source, ctx, move || crate::browser::import::import(source));
    }

    /// Runs `job` off the UI thread: the Keychain prompt blocks it.
    pub(crate) fn start_with(
        &mut self,
        source: ImportSource,
        ctx: &egui::Context,
        job: impl FnOnce() -> Result<Imported, ImportError> + Send + 'static,
    ) {
        if self.running.is_some() {
            return;
        }
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(job());
            ctx.request_repaint();
        });
        self.running = Some((source, receiver));
        self.status = ImportStatus::Running(source);
    }

    /// Forgets the last import's outcome; one under way stays told.
    fn dismiss(&mut self) {
        if self.running.is_none() {
            self.status = ImportStatus::Idle;
        }
    }

    fn finished(&mut self) -> Option<(ImportSource, Result<Imported, ImportError>)> {
        let (source, receiver) = self.running.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return None,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                Err(ImportError::Failed("the import stopped".to_owned()))
            }
        };
        let source = *source;
        self.running = None;
        Some((source, result))
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
