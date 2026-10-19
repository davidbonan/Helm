//! Browser panes of the split tree (specs/browser.md): their pages, the Browser
//! offer of fresh terminals, the native views kept on their panes, and the
//! favorites, history and imports they share.

use std::collections::{HashMap, HashSet};

use crossbeam_channel::Receiver;

use crate::browser::freeze::{Cover, FreezeStep, PageFreeze};
use crate::browser::import::{ImportError, ImportReport, ImportSource, ImportStatus, Imported};
use crate::browser::page_view::ZoomChange;
use crate::browser::{url, PageSnapshot, PageState, ViewRect, WebEngine, WebView};
use crate::terminal::layout::PaneId;
use crate::ui::browser_pane::{
    browser_pane, AddressBar, BrowserChrome, BrowserIntent, BrowserPaneView, BrowserTabState,
    TabStrip, TabSummary,
};

use super::git_session::{PaneKey, RepoKey};
use super::keys::{consume_action, consume_zoom_commands, ZoomCommand};
use super::{HelmApp, Page, TerminalState};
use crate::keybindings::Action;

/// Lines of the Run strip scanned for the server's URL (browser.md §5).
const SERVER_OUTPUT_LINES: usize = 400;
const DISK_ACCESS_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// The browser state of one tab.
#[derive(Default)]
pub(crate) struct TabBrowsers {
    pub(crate) pages: HashMap<PaneId, BrowserPane>,
    /// Fresh shells still showing the Browser button (browser.md §3).
    pub(crate) offers: HashSet<PaneId>,
}

impl TabBrowsers {
    pub(crate) fn is_empty(&self) -> bool {
        self.pages.is_empty() && self.offers.is_empty()
    }

    pub(crate) fn retain_panes(&mut self, live: &HashSet<PaneId>) {
        self.pages.retain(|id, _| live.contains(id));
        self.offers.retain(|id| live.contains(id));
    }
}

pub(crate) struct BrowserPane {
    tabs: Vec<BrowserTab>,
    /// The tab shown, its page on the pane.
    active: usize,
    /// Another tab came to show: it takes the keyboard when the pane has it.
    shown_changed: bool,
    /// The page area drawn this frame, in egui points.
    placed: Option<egui::Rect>,
    /// The pane's own suggestions fell over the page this frame.
    overlaid: bool,
    was_focused: bool,
    /// The pane held the keyboard when last drawn: its page, its URL field, or the
    /// tree's focus with no other widget holding it.
    has_keyboard: bool,
    /// Drawn this frame.
    drawn: bool,
}

/// One page of a browser pane.
struct BrowserTab {
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
    fn opening(url: Option<String>) -> Self {
        Self {
            view: None,
            state: BrowserTabState {
                address: AddressBar {
                    focus_requested: url.is_none(),
                    ..AddressBar::default()
                },
                ..BrowserTabState::default()
            },
            pending: url,
            freeze: PageFreeze::default(),
            visited: None,
            icon_seen: None,
            applied_zoom: 1.0,
            finding: false,
        }
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
        let rect = sync.placed.map(|rect| view_rect(rect, ctx.zoom_factor()));
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
    /// The pane has the tree's focus: a first load takes the keyboard.
    focused: bool,
    ctx: &'a egui::Context,
}

impl BrowserPane {
    /// A pane opening on `url` (browser.md §5).
    pub(crate) fn opening(url: Option<String>) -> Self {
        Self {
            tabs: vec![BrowserTab::opening(url)],
            active: 0,
            shown_changed: false,
            placed: None,
            overlaid: false,
            was_focused: false,
            has_keyboard: false,
            drawn: false,
        }
    }

    fn shown(&mut self) -> &mut BrowserTab {
        &mut self.tabs[self.active]
    }

    pub(crate) fn load(&mut self, url: String) {
        self.shown().load(url);
    }

    pub(crate) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// A tab on the start page, after the shown one, shown in its place.
    fn open_tab(&mut self) {
        self.active += 1;
        self.tabs.insert(self.active, BrowserTab::opening(None));
        self.shown_changed = true;
    }

    /// A tab on `url` after the shown one, left in the background: its page
    /// loads once shown.
    fn open_background_tab(&mut self, url: String) {
        self.tabs
            .insert(self.active + 1, BrowserTab::opening(Some(url)));
    }

    /// Closes tab `index`; the one after it is shown in its place, else the one
    /// before. The last tab stays: closing it closes the pane.
    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() == 1 || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if index < self.active || self.active == self.tabs.len() {
            self.active -= 1;
        }
        self.shown_changed = true;
    }

    fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() && index != self.active {
            self.active = index;
            self.shown_changed = true;
        }
    }

    fn has_native_focus(&self) -> bool {
        self.tabs[self.active]
            .view
            .as_ref()
            .is_some_and(|view| view.is_focused())
    }

    /// Applies a pane's own intent; the shared ones (favorites, import, server)
    /// come back for the app.
    fn apply(&mut self, intent: BrowserIntent) -> Option<BrowserIntent> {
        let tab = self.shown();
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
            BrowserIntent::NewTab => self.open_tab(),
            BrowserIntent::OpenInNewTab(url) => self.open_background_tab(url),
            BrowserIntent::SelectTab(index) => self.select_tab(index),
            BrowserIntent::CloseTab(index) => self.close_tab(index),
            shared => return Some(shared),
        }
        None
    }

    /// Places the shown tab's view on this frame's page area, frozen while egui
    /// draws over it; the other tabs' views stay hidden (browser.md §4).
    fn sync_views(&mut self, engine: Option<&dyn WebEngine>, covered: bool, ctx: &egui::Context) {
        if !std::mem::take(&mut self.drawn) {
            self.has_keyboard = false;
        }
        let placed = self.placed.take();
        let overlaid = std::mem::take(&mut self.overlaid);
        let cover = match placed {
            None => Cover::Away,
            Some(_) if covered || overlaid => Cover::Overlay,
            Some(_) => Cover::None,
        };
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let shown = index == self.active;
            let sync = ViewSync {
                cover: if shown { cover } else { Cover::Away },
                placed: placed.filter(|_| shown),
                focused: self.was_focused,
                ctx,
            };
            tab.sync_view(engine, &sync);
        }
    }
}

fn backdrop_texture(ctx: &egui::Context, snapshot: PageSnapshot) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_premultiplied(
        [snapshot.width, snapshot.height],
        &snapshot.rgba_premultiplied,
    );
    ctx.load_texture("browser-snapshot", image, egui::TextureOptions::LINEAR)
}

fn view_rect(rect: egui::Rect, zoom: f32) -> ViewRect {
    let zoom = f64::from(zoom);
    ViewRect {
        x: f64::from(rect.left()) * zoom,
        y: f64::from(rect.top()) * zoom,
        width: f64::from(rect.width()) * zoom,
        height: f64::from(rect.height()) * zoom,
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

/// What a browser pane's leaf reported this frame.
pub(crate) struct BrowserLeaf {
    /// The pane takes focus in the tree (a click in its toolbar or its page).
    pub(crate) focus: bool,
    /// For the app: favorites, import, Dev server.
    pub(crate) intents: Vec<BrowserIntent>,
    /// Pages to count in the history: URL, title.
    pub(crate) visits: Vec<(String, String)>,
    /// Icons the pages named: page URL, icon URL.
    pub(crate) icons: Vec<(String, String)>,
}

/// One browser pane of the tree (browser.md §4). `focused` is the tree's focus.
pub(crate) fn render_browser_pane(
    ui: &mut egui::Ui,
    browser: &mut BrowserPane,
    focused: bool,
    chrome: &BrowserChrome<'_>,
    any_focused: &mut bool,
) -> BrowserLeaf {
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
    let strip = TabStrip {
        tabs: browser
            .tabs
            .iter()
            .zip(pages.iter().cloned())
            .map(|(tab, page)| tab.summary(page))
            .collect(),
        active: browser.active,
    };
    let active = browser.active;
    let view = BrowserPaneView {
        page: &pages[active],
        tab: &mut browser.tabs[active].state,
        strip: &strip,
    };
    let output = browser_pane(ui, chrome, view);
    browser.placed = output.page_rect;
    browser.overlaid = output.page_overlaid;
    let native_focus = browser.has_native_focus();
    if native_focus {
        surrender_egui_focus(ui.ctx());
    }
    let shown_changed = std::mem::take(&mut browser.shown_changed);
    let takes_keyboard = !browser.was_focused || shown_changed;
    if focused && takes_keyboard && !output.field_focused && !native_focus {
        if let Some(view) = browser.shown().view.as_mut() {
            view.focus();
        }
    }
    browser.was_focused = focused;
    // Like a terminal: the tree's focus counts while no other widget holds the
    // keyboard (a click in the toolbar hands first responder back to egui).
    let egui_idle = ui.ctx().memory(|m| m.focused().is_none());
    browser.has_keyboard = native_focus || output.field_focused || (focused && egui_idle);
    browser.drawn = true;
    *any_focused |= browser.has_keyboard;
    let intents = output
        .intents
        .into_iter()
        .filter_map(|intent| browser.apply(intent))
        .collect();
    BrowserLeaf {
        focus: output.clicked || (native_focus && !focused),
        intents,
        visits,
        icons,
    }
}

/// The page holds the keyboard: no egui widget keeps a caret behind it.
fn surrender_egui_focus(ctx: &egui::Context) {
    if let Some(id) = ctx.memory(|m| m.focused()) {
        ctx.memory_mut(|m| m.surrender_focus(id));
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

    /// Closes the fresh shell of pane `id` and opens a page in its place
    /// (browser.md §3).
    pub(crate) fn turn_into_browser(&mut self, pane_key: &PaneKey, id: PaneId) {
        let url = self.server_url_of(&pane_key.0);
        if let Some(panes) = self.caches.panes.get_mut(pane_key) {
            panes.remove(&id);
        }
        let tab = self.caches.browsers.entry(pane_key.clone()).or_default();
        tab.offers.remove(&id);
        tab.pages.insert(id, BrowserPane::opening(url));
        if let Some(layout) = self.workspace.active_layout_mut() {
            layout.set_focus(id);
        }
    }

    /// The shared intents of the tab's browser panes this frame.
    pub(crate) fn apply_browser_intents(
        &mut self,
        pane_key: &PaneKey,
        intents: Vec<(PaneId, BrowserIntent)>,
        ctx: &egui::Context,
    ) {
        let project = self.browser_project();
        for (id, intent) in intents {
            match intent {
                BrowserIntent::LoadServer => self.load_server_page(pane_key, id),
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

    /// The toolbar's Server button: the default page again.
    pub(crate) fn load_server_page(&mut self, pane_key: &PaneKey, id: PaneId) {
        let Some(url) = self.server_url_of(&pane_key.0) else {
            return;
        };
        if let Some(browser) = self
            .caches
            .browsers
            .get_mut(pane_key)
            .and_then(|tab| tab.pages.get_mut(&id))
        {
            browser.load(url);
        }
    }

    /// While a browser pane holds the keyboard (browser.md §6), `Cmd+T` and
    /// `Cmd+W` act on its tabs, zoom on its page and `Cmd+F` opens its find bar,
    /// taken before helm sees them; `Cmd+W` on its last tab is left to close the
    /// pane.
    pub(crate) fn route_browser_tab_keys(&mut self, ctx: &egui::Context) {
        let Some((key, id)) = self.focused_pane() else {
            return;
        };
        let Some(browser) = self
            .caches
            .browsers
            .get_mut(&key)
            .and_then(|tab| tab.pages.get_mut(&id))
            .filter(|browser| browser.has_keyboard)
        else {
            return;
        };
        if consume_action(ctx, &self.keymap, Action::NewTab) {
            browser.open_tab();
        }
        if browser.tab_count() > 1 && consume_action(ctx, &self.keymap, Action::ClosePane) {
            browser.close_tab(browser.active);
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
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            browser.shown().state.open_find();
        }
    }

    /// The active tab's focused pane.
    fn focused_pane(&self) -> Option<(PaneKey, PaneId)> {
        let index = self.workspace.active()?;
        let tab = self.workspace.active_tab()?;
        let key = self.caches.pane_key(&self.workspace, index, tab)?;
        Some((key, self.workspace.active_layout()?.focus()))
    }

    /// Places every page view for this frame, frozen wherever egui draws above
    /// the panes (browser.md §4). Called once per frame, after every overlay.
    pub(crate) fn sync_browser_views(&mut self, ctx: &egui::Context) {
        let covered = self.page == Page::Preferences
            || self.modal.is_some()
            || egui::Popup::is_any_open(ctx)
            || egui::DragAndDrop::has_any_payload(ctx);
        let engine = self.web_engine.as_deref();
        for tab in self.caches.browsers.values_mut() {
            for browser in tab.pages.values_mut() {
                browser.sync_views(engine, covered, ctx);
            }
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
