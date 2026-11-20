//! Browser pane (specs/browser.md): the engine port and the URL rules.

pub mod dialog;
pub mod download;
pub mod favicons;
pub mod fold;
pub mod freeze;
pub mod import;
pub mod keyboard;
pub mod library;
pub mod load_failure;
pub mod page_view;
pub mod pip;
pub mod sections;
pub mod split;
pub mod url;
#[cfg(target_os = "macos")]
pub mod webkit;

use dialog::{DialogAnswer, PageDialog};
use download::Download;
use load_failure::LoadFailure;

/// What the toolbar shows of the page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageState {
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// 0.0‥1.0 while loading.
    pub progress: f32,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// sRGB the toolbar takes: the page's `theme-color`, else its background.
    pub tint: Option<[u8; 3]>,
    /// The main document's vertical scroll, in CSS pixels.
    pub scroll_y: f64,
    /// The pointer is over the page: egui sees none of its moves there.
    pub pointer_over: bool,
    /// The icon the page names (browser.md §10), once it reported it.
    pub icon_url: Option<String>,
    /// Why the last load failed, until another starts (browser.md §14).
    pub failure: Option<LoadFailure>,
    /// What the page waits on the user for (browser.md §12).
    pub dialog: Option<PageDialog>,
    /// The files the page handed over, oldest first (browser.md §13).
    pub downloads: Vec<Download>,
    /// The page closed itself (`window.close()`).
    pub closed: bool,
}

/// A rectangle in window points, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Rounding of the bottom corners, those of the block the page sits in.
    pub radius: f64,
    /// The top corners round too: a split half's card (browser.md §16).
    pub rounds_top: bool,
    /// Width of the left edge whose clicks go to helm, not the page: the
    /// sidebar's resize grab reaches over it.
    pub pass_through_left: f64,
}

/// A picture of the page, standing in for the view while egui draws over it.
#[derive(Debug, Clone, PartialEq)]
pub struct PageSnapshot {
    pub width: usize,
    pub height: usize,
    pub rgba_premultiplied: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Back,
    Forward,
    /// After a failed load, tries it again.
    Reload,
    /// Reload bypassing the cache.
    ReloadFromOrigin,
    Stop,
}

/// A cookie carried from another browser (browser.md §9).
#[derive(Debug, Clone, PartialEq)]
pub struct Cookie {
    /// As the browser stored it: `.example.com` covers the subdomains.
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    /// Unix seconds; `None` ⇒ a session cookie.
    pub expires: Option<f64>,
    pub secure: bool,
    pub http_only: bool,
}

/// Opens pages; one engine per window (browser.md §2).
pub trait WebEngine {
    fn open(&self) -> Box<dyn WebView>;
    /// Into the persistent profile every page shares; how many it took.
    fn import_cookies(&self, cookies: &[Cookie]) -> usize;
}

/// One page shown over one pane (browser.md §2).
pub trait WebView {
    /// `None` hides the view.
    fn place(&mut self, rect: Option<ViewRect>);
    fn load(&mut self, url: &str);
    fn navigate(&mut self, navigation: Navigation);
    fn page(&self) -> PageState;
    fn focus(&mut self);
    fn is_focused(&self) -> bool;
    fn show_inspector(&mut self);
    /// Asks for a snapshot of the page; one asked before and not taken is dropped.
    fn request_snapshot(&mut self);
    /// The snapshot asked for, once it is in.
    fn take_snapshot(&mut self) -> Option<PageSnapshot>;
    /// 1.0 is 100 %.
    fn set_zoom(&mut self, zoom: f64);
    /// Selects the next match of `text` after the current one (before it when
    /// `backwards`), wrapping around.
    fn find(&mut self, text: &str, backwards: bool);
    /// Whether the last `find` matched, once the engine answers.
    fn take_find_result(&mut self) -> Option<bool>;
    /// Answers the page's dialog; none pending ⇒ nothing.
    fn answer_dialog(&mut self, answer: DialogAnswer);
    /// A page this one opened (`target="_blank"`, `window.open`), already
    /// loading in its own view.
    fn take_opened(&mut self) -> Option<Box<dyn WebView>>;
}
