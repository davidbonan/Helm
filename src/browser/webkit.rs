//! WebKit adapter of the engine port (browser.md §2): a `WKWebView` per pane,
//! in its own frame view added over the window's content view.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::{Block, RcBlock};
use core_graphics::base::kCGImageAlphaPremultipliedLast;
use core_graphics::color_space::{kCGColorSpaceSRGB, CGColorSpace};
use core_graphics::context::CGContext;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::image::CGImageRef;
use foreign_types::ForeignTypeRef;
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol, ProtocolObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSColorSpace, NSDeleteCharacter, NSDeleteFunctionKey,
    NSDownArrowFunctionKey, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSImage,
    NSLeftArrowFunctionKey, NSMenu, NSMenuItem, NSModalResponse, NSModalResponseOK, NSOpenPanel,
    NSResponder, NSRightArrowFunctionKey, NSUpArrowFunctionKey, NSView,
};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSArray, NSData, NSDate, NSDictionary, NSError, NSHTTPCookie,
    NSHTTPCookieDomain, NSHTTPCookieExpires, NSHTTPCookieName, NSHTTPCookiePath,
    NSHTTPCookieSecure, NSHTTPCookieValue, NSHTTPURLResponse, NSNumber, NSObject, NSPoint,
    NSProgressReporting, NSRect, NSSize, NSString, NSURLRequest, NSURLResponse, NSURL,
};
use objc2_web_kit::{
    WKDownload, WKDownloadDelegate, WKFindConfiguration, WKFindResult, WKFrameInfo, WKNavigation,
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationResponse,
    WKNavigationResponsePolicy, WKOpenPanelParameters, WKScriptMessage, WKScriptMessageHandler,
    WKSnapshotConfiguration, WKUIDelegate, WKUserContentController, WKUserScript,
    WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
    WKWindowFeatures,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::dialog::{DialogAnswer, DialogKind, PageDialog};
use super::download::{self, Download, DownloadStatus};
use super::keyboard::{self, CmdChord, CmdKey};
use super::load_failure::{FailureReason, LoadFailure};
use super::{favicons, Cookie, Navigation, PageSnapshot, PageState, ViewRect, WebEngine, WebView};

const ICON_HANDLER: &str = "helmIcon";
/// Posts the page's URL and its largest raster icon, else `/favicon.ico`, one
/// per line (browser.md §10).
const ICON_REPORTER_JS: &str = "(() => {\
  const size = (link) => Math.max(link.rel.includes('apple') ? 180 : 0, ...(link.getAttribute('sizes') || '').split(/\\s+/).map((s) => parseInt(s, 10) || 0));\
  const links = [...document.querySelectorAll('link[rel~=\"icon\"], link[rel^=\"apple-touch-icon\"]')]\
    .filter((link) => link.href && link.type !== 'image/svg+xml' && !/\\.svg([?#]|$)/i.test(link.href))\
    .sort((a, b) => size(b) - size(a));\
  const icon = links.length ? links[0].href : new URL('/favicon.ico', location.href).href;\
  window.webkit.messageHandlers.helmIcon.postMessage(location.href + '\\n' + icon);\
})();";

const SCROLL_HANDLER: &str = "helmScroll";
/// Reports the document's scroll offset, at most once a frame, and once loaded.
const SCROLL_REPORTER_JS: &str = "(() => {\
  let queued = false;\
  const report = () => { queued = false; window.webkit.messageHandlers.helmScroll.postMessage(window.scrollY); };\
  addEventListener('scroll', () => { if (!queued) { queued = true; requestAnimationFrame(report); } }, { passive: true });\
  report();\
})();";

const POINTER_HANDLER: &str = "helmPointer";
/// Posts 1 when the pointer comes over the page, 0 when it leaves it.
const POINTER_REPORTER_JS: &str = "(() => {\
  let over = false;\
  const report = (now) => { if (now !== over) { over = now; window.webkit.messageHandlers.helmPointer.postMessage(now ? 1 : 0); } };\
  document.addEventListener('mouseover', () => report(true), { passive: true });\
  document.addEventListener('mouseout', (event) => { if (!event.relatedTarget) report(false); }, { passive: true });\
})();";

/// Asks the UI for a frame: what a page tells of itself arrives off egui's input.
type Wake = Rc<dyn Fn()>;

pub struct WebKitEngine {
    host: Retained<NSView>,
    monitor: Retained<AnyObject>,
    wake: Wake,
    mtm: MainThreadMarker,
}

impl WebKitEngine {
    /// The engine of the window behind `window`, calling `wake` when a page
    /// changes on its own; `None` off the main thread or without an AppKit window.
    pub fn attach(window: &impl HasWindowHandle, wake: impl Fn() + 'static) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(appkit) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        let host: Retained<NSView> =
            unsafe { Retained::retain(appkit.ns_view.cast::<NSView>().as_ptr())? };
        let monitor = route_helm_chords(host.clone())?;
        add_edit_menu(mtm);
        Some(Self {
            host,
            monitor,
            wake: Rc::new(wake),
            mtm,
        })
    }
}

impl Drop for WebKitEngine {
    fn drop(&mut self) {
        unsafe { NSEvent::removeMonitor(&self.monitor) };
    }
}

impl WebEngine for WebKitEngine {
    fn open(&self) -> Box<dyn WebView> {
        Box::new(WebKitView::new(&self.host, self.wake.clone(), self.mtm))
    }

    fn import_cookies(&self, cookies: &[Cookie]) -> usize {
        let store = unsafe { WKWebsiteDataStore::defaultDataStore().httpCookieStore() };
        let mut taken = 0;
        for cookie in cookies.iter().filter_map(ns_cookie) {
            unsafe { store.setCookie_completionHandler(&cookie, None) };
            taken += 1;
        }
        taken
    }
}

fn ns_cookie(cookie: &Cookie) -> Option<Retained<NSHTTPCookie>> {
    let text = |value: &str| unsafe { Retained::cast::<AnyObject>(NSString::from_str(value)) };
    let mut properties: Vec<(&NSString, Retained<AnyObject>)> = unsafe {
        vec![
            (NSHTTPCookieDomain, text(&cookie.domain)),
            (NSHTTPCookieName, text(&cookie.name)),
            (NSHTTPCookieValue, text(&cookie.value)),
            (NSHTTPCookiePath, text(&cookie.path)),
        ]
    };
    if let Some(expires) = cookie.expires {
        let date = unsafe { NSDate::dateWithTimeIntervalSince1970(expires) };
        properties.push((unsafe { NSHTTPCookieExpires }, unsafe {
            Retained::cast::<AnyObject>(date)
        }));
    }
    if cookie.secure {
        properties.push((unsafe { NSHTTPCookieSecure }, text("TRUE")));
    }
    if cookie.http_only {
        // No public key constant; NSHTTPCookie reads this one.
        properties.push((ns_string!("HttpOnly"), text("TRUE")));
    }
    let (keys, values): (Vec<&NSString>, Vec<Retained<AnyObject>>) = properties.into_iter().unzip();
    unsafe { NSHTTPCookie::cookieWithProperties(&NSDictionary::from_vec(&keys, values)) }
}

struct WebKitView {
    /// The pane's page area. WebKit docks the inspector inside the web view's
    /// superview, so each page gets its own instead of the whole window.
    frame: Retained<PageFrame>,
    view: Retained<WKWebView>,
    host: Retained<NSView>,
    /// Its dialogs, file pickers, popups.
    ui: Retained<PageUi>,
    /// Its failures and downloads.
    navigation: Retained<PageNavigation>,
    /// What the page's scroll reporter last posted.
    scroll_y: Rc<Cell<f64>>,
    /// What the page's pointer reporter last posted: 1 while over the page.
    pointer: Rc<Cell<f64>>,
    /// What the page's icon reporter last posted: the page's site, its icon.
    icon: Rc<RefCell<Option<(String, String)>>>,
    /// Hidden while it had focus: focus comes back with it.
    refocus_on_show: bool,
    /// The corners last given to the frame's layer.
    corners: (f64, bool),
    /// The snapshot asked last, once WebKit hands it.
    snapshot: Rc<RefCell<Option<PageSnapshot>>>,
    /// Counts the snapshots asked: an answer to an older one is dropped.
    snapshot_asks: Rc<Cell<u64>>,
    /// Whether the last find matched, once WebKit answers.
    find_result: Rc<Cell<Option<bool>>>,
}

impl WebKitView {
    fn new(host: &Retained<NSView>, wake: Wake, mtm: MainThreadMarker) -> Self {
        let config = unsafe { WKWebViewConfiguration::new() };
        unsafe {
            config.setWebsiteDataStore(&WKWebsiteDataStore::defaultDataStore());
            // Private KVC key behind the "Inspect Element" context menu entry.
            let _: () = msg_send![
                &*config.preferences(),
                setValue: &*NSNumber::numberWithBool(true),
                forKey: ns_string!("developerExtrasEnabled")
            ];
        }
        Self::with_configuration(host, &config, wake, mtm)
    }

    /// A view on `config`: helm's own, or the one WebKit hands a popup, which
    /// must be the popup's for its opener to reach it.
    fn with_configuration(
        host: &Retained<NSView>,
        config: &WKWebViewConfiguration,
        wake: Wake,
        mtm: MainThreadMarker,
    ) -> Self {
        let scroll_y = Rc::new(Cell::new(0.0));
        let pointer = Rc::new(Cell::new(0.0));
        let icon = Rc::default();
        let view = unsafe {
            // A popup's configuration carries its opener's reporters: its own replace them.
            config.setUserContentController(&WKUserContentController::new());
            report_number(config, &NumberReport::SCROLL, &scroll_y, mtm);
            report_number(config, &NumberReport::POINTER, &pointer, mtm);
            report_icon(config, &icon, mtm);
            WKWebView::initWithFrame_configuration(mtm.alloc(), NSRect::ZERO, config)
        };
        let ui = PageUi::new(host.clone(), wake.clone(), mtm);
        let navigation = PageNavigation::new(wake, mtm);
        let frame = PageFrame::new(mtm);
        unsafe {
            view.setUIDelegate(Some(ProtocolObject::from_ref(&*ui)));
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation)));
            view.setAllowsBackForwardNavigationGestures(true);
            if view.respondsToSelector(sel!(setInspectable:)) {
                view.setInspectable(true);
            }
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::NSViewWidthSizable
                    | NSAutoresizingMaskOptions::NSViewHeightSizable,
            );
            frame.setWantsLayer(true);
            frame.addSubview(&view);
            frame.setHidden(true);
            host.addSubview(&frame);
        }
        Self {
            frame,
            view,
            host: host.clone(),
            ui,
            navigation,
            scroll_y,
            pointer,
            icon,
            refocus_on_show: false,
            corners: (0.0, false),
            snapshot: Rc::default(),
            snapshot_asks: Rc::default(),
            find_result: Rc::default(),
        }
    }
}

impl Drop for WebKitView {
    fn drop(&mut self) {
        // WebKit raises if a dialog's completion handler dies uncalled.
        self.answer_dialog(DialogAnswer::Cancel);
        if self.is_focused() {
            self.release_focus();
        }
        unsafe { self.frame.removeFromSuperview() };
    }
}

impl WebKitView {
    fn is_hidden(&self) -> bool {
        unsafe { self.frame.isHidden() }
    }

    fn release_focus(&self) {
        if let Some(window) = self.host.window() {
            window.makeFirstResponder(Some(&self.host));
        }
    }
}

declare_class!(
    /// A page's area; clicks on its left edge strip, its ivar's width in points,
    /// fall through to helm.
    struct PageFrame;

    unsafe impl ClassType for PageFrame {
        #[inherits(NSResponder, NSObject)]
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HelmBrowserPageFrame";
    }

    impl DeclaredClass for PageFrame {
        type Ivars = Cell<f64>;
    }

    unsafe impl PageFrame {
        #[method_id(hitTest:)]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            if point.x - self.frame().origin.x < self.ivars().get() {
                None
            } else {
                unsafe { msg_send_id![super(self), hitTest: point] }
            }
        }
    }
);

impl PageFrame {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(Cell::new(0.0));
        unsafe { msg_send_id![super(this), initWithFrame: NSRect::ZERO] }
    }
}

/// `kCALayerMinXMinYCorner | kCALayerMaxXMinYCorner`: the bottom corners of an
/// unflipped view's layer.
const BOTTOM_CORNERS: usize = 1 | 2;
const ALL_CORNERS: usize = 1 | 2 | 4 | 8;

/// Clips the page to the rounded corners of the block it sits in.
fn round_corners(frame: &NSView, radius: f64, rounds_top: bool) {
    let corners = if rounds_top {
        ALL_CORNERS
    } else {
        BOTTOM_CORNERS
    };
    let layer: Option<Retained<AnyObject>> = unsafe { msg_send_id![frame, layer] };
    let Some(layer) = layer else {
        return;
    };
    unsafe {
        let _: () = msg_send![&*layer, setCornerRadius: radius];
        let _: () = msg_send![&*layer, setMaskedCorners: corners];
        let _: () = msg_send![&*layer, setMasksToBounds: radius > 0.0];
    }
}

impl WebView for WebKitView {
    fn place(&mut self, rect: Option<ViewRect>) {
        let Some(rect) = rect else {
            if !self.is_hidden() {
                if self.is_focused() {
                    self.refocus_on_show = true;
                    self.release_focus();
                }
                self.frame.setHidden(true);
            }
            return;
        };
        let area = NSRect::new(
            NSPoint::new(rect.x, rect.y),
            NSSize::new(rect.width, rect.height),
        );
        unsafe {
            if self.frame.frame() != area {
                self.frame.setFrame(area);
            }
        }
        self.frame.ivars().set(rect.pass_through_left);
        let corners = (rect.radius, rect.rounds_top);
        if self.corners != corners {
            round_corners(&self.frame, rect.radius, rect.rounds_top);
            self.corners = corners;
        }
        if self.is_hidden() {
            self.frame.setHidden(false);
            if std::mem::take(&mut self.refocus_on_show) {
                self.focus();
            }
        }
    }

    fn load(&mut self, url: &str) {
        let Some(url) = (unsafe { NSURL::URLWithString(&NSString::from_str(url)) }) else {
            return;
        };
        unsafe { self.view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
    }

    fn navigate(&mut self, navigation: Navigation) {
        let failed = self.navigation.ivars().failure.borrow().clone();
        if let (Some(failure), Navigation::Reload | Navigation::ReloadFromOrigin) =
            (failed, navigation)
        {
            self.load(&failure.url);
            return;
        }
        unsafe {
            match navigation {
                Navigation::Back => drop(self.view.goBack()),
                Navigation::Forward => drop(self.view.goForward()),
                Navigation::Reload => drop(self.view.reload()),
                Navigation::ReloadFromOrigin => drop(self.view.reloadFromOrigin()),
                Navigation::Stop => self.view.stopLoading(),
            }
        }
    }

    fn page(&self) -> PageState {
        let url = unsafe {
            self.view
                .URL()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string())
                .unwrap_or_default()
        };
        let icon_url = self.icon.borrow().as_ref().and_then(|(site, icon)| {
            (favicons::site_of(&url) == Some(site.as_str())).then(|| icon.clone())
        });
        let failure = self.navigation.ivars().failure.borrow().clone();
        let ui = self.ui.ivars();
        unsafe {
            PageState {
                url: failure.as_ref().map_or(url, |failure| failure.url.clone()),
                title: self
                    .view
                    .title()
                    .map(|title| title.to_string())
                    .unwrap_or_default(),
                loading: self.view.isLoading(),
                progress: self.view.estimatedProgress() as f32,
                can_go_back: self.view.canGoBack(),
                can_go_forward: self.view.canGoForward(),
                tint: page_tint(&self.view),
                scroll_y: self.scroll_y.get(),
                pointer_over: self.pointer.get() > 0.0,
                icon_url,
                failure,
                dialog: ui
                    .dialog
                    .borrow()
                    .as_ref()
                    .map(|asked| asked.dialog.clone()),
                downloads: self.navigation.ivars().downloads.downloads(),
                closed: ui.closed.get(),
            }
        }
    }

    fn focus(&mut self) {
        if self.is_hidden() {
            self.refocus_on_show = true;
            return;
        }
        if let Some(window) = self.host.window() {
            window.makeFirstResponder(Some(&self.view));
        }
    }

    fn is_focused(&self) -> bool {
        first_responder_view(&self.host)
            .is_some_and(|view| unsafe { view.isDescendantOf(&self.frame) })
    }

    fn show_inspector(&mut self) {
        // `_inspector` is private WebKit API: the docked Web Inspector of this view.
        unsafe {
            if !self.view.respondsToSelector(sel!(_inspector)) {
                return;
            }
            let inspector: *mut AnyObject = msg_send![&*self.view, _inspector];
            if let Some(inspector) = inspector.as_ref() {
                let _: () = msg_send![inspector, show];
            }
        }
    }

    fn request_snapshot(&mut self) {
        let ask = self.snapshot_asks.get() + 1;
        self.snapshot_asks.set(ask);
        self.snapshot.replace(None);
        let (slot, asks) = (self.snapshot.clone(), self.snapshot_asks.clone());
        let done = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
            if asks.get() != ask {
                return;
            }
            if let Some(image) = unsafe { image.as_ref() } {
                slot.replace(snapshot_pixels(image));
            }
        });
        unsafe {
            let config = WKSnapshotConfiguration::new();
            config.setAfterScreenUpdates(false);
            self.view
                .takeSnapshotWithConfiguration_completionHandler(Some(&config), &done);
        }
    }

    fn take_snapshot(&mut self) -> Option<PageSnapshot> {
        self.snapshot.take()
    }

    fn set_zoom(&mut self, zoom: f64) {
        unsafe { self.view.setPageZoom(zoom) };
    }

    fn find(&mut self, text: &str, backwards: bool) {
        self.find_result.set(None);
        let slot = self.find_result.clone();
        let done = RcBlock::new(move |result: NonNull<WKFindResult>| {
            slot.set(Some(unsafe { result.as_ref().matchFound() }));
        });
        unsafe {
            let config = WKFindConfiguration::new();
            config.setBackwards(backwards);
            config.setWraps(true);
            self.view.findString_withConfiguration_completionHandler(
                &NSString::from_str(text),
                Some(&config),
                &done,
            );
        }
    }

    fn take_find_result(&mut self) -> Option<bool> {
        self.find_result.take()
    }

    fn answer_dialog(&mut self, answer: DialogAnswer) {
        let asked = self.ui.ivars().dialog.borrow_mut().take();
        if let Some(asked) = asked {
            asked.reply.send(&answer);
        }
    }

    fn take_opened(&mut self) -> Option<Box<dyn WebView>> {
        let mut opened = self.ui.ivars().opened.borrow_mut();
        if opened.is_empty() {
            return None;
        }
        Some(Box::new(opened.remove(0)))
    }
}

/// `CGImageRef`, as AppKit's method signatures encode it.
#[repr(C)]
struct OpaqueCGImage {
    _private: [u8; 0],
}

unsafe impl RefEncode for OpaqueCGImage {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}

/// The snapshot's pixels, at the screen's scale.
fn snapshot_pixels(image: &NSImage) -> Option<PageSnapshot> {
    let raw: *mut OpaqueCGImage = unsafe {
        msg_send![
            image,
            CGImageForProposedRect: std::ptr::null_mut::<NSRect>(),
            context: std::ptr::null::<AnyObject>(),
            hints: std::ptr::null::<AnyObject>()
        ]
    };
    if raw.is_null() {
        return None;
    }
    let image = unsafe { CGImageRef::from_ptr(raw.cast()) }.to_owned();
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return None;
    }
    let space = CGColorSpace::create_with_name(unsafe { kCGColorSpaceSRGB })?;
    let mut context = CGContext::create_bitmap_context(
        None,
        width,
        height,
        8,
        width * 4,
        &space,
        kCGImageAlphaPremultipliedLast,
    );
    let whole = CGRect::new(
        &CGPoint::new(0.0, 0.0),
        &CGSize::new(width as f64, height as f64),
    );
    context.draw_image(whole, &image);
    Some(PageSnapshot {
        width,
        height,
        rgba_premultiplied: context.data().to_vec(),
    })
}

/// A script posting numbers to its handler.
struct NumberReport {
    source: &'static str,
    handler: &'static str,
}

impl NumberReport {
    const SCROLL: Self = Self {
        source: SCROLL_REPORTER_JS,
        handler: SCROLL_HANDLER,
    };
    const POINTER: Self = Self {
        source: POINTER_REPORTER_JS,
        handler: POINTER_HANDLER,
    };
}

/// Injects `report` in every page of `config`, what it posts landing in `value`.
unsafe fn report_number(
    config: &WKWebViewConfiguration,
    report: &NumberReport,
    value: &Rc<Cell<f64>>,
    mtm: MainThreadMarker,
) {
    let controller = config.userContentController();
    let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
        mtm.alloc(),
        &NSString::from_str(report.source),
        WKUserScriptInjectionTime::AtDocumentEnd,
        true,
    );
    controller.addUserScript(&script);
    let reporter = NumberReporter::new(value.clone(), mtm);
    controller.addScriptMessageHandler_name(
        ProtocolObject::from_ref(&*reporter),
        &NSString::from_str(report.handler),
    );
}

/// Injects the icon reporter in every page of `config`; what it posts lands in
/// `icon`.
unsafe fn report_icon(
    config: &WKWebViewConfiguration,
    icon: &Rc<RefCell<Option<(String, String)>>>,
    mtm: MainThreadMarker,
) {
    let controller = config.userContentController();
    let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
        mtm.alloc(),
        &NSString::from_str(ICON_REPORTER_JS),
        WKUserScriptInjectionTime::AtDocumentEnd,
        true,
    );
    controller.addUserScript(&script);
    let reporter = IconReporter::new(icon.clone(), mtm);
    controller.addScriptMessageHandler_name(
        ProtocolObject::from_ref(&*reporter),
        &NSString::from_str(ICON_HANDLER),
    );
}

/// The page's `theme-color`, else the color WebKit derives from its background;
/// `None` when translucent or before macOS 12.
fn page_tint(view: &WKWebView) -> Option<[u8; 3]> {
    if !view.respondsToSelector(sel!(themeColor)) {
        return None;
    }
    let color =
        unsafe { view.themeColor() }.unwrap_or_else(|| unsafe { view.underPageBackgroundColor() });
    let srgb = unsafe { color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) }?;
    let (mut red, mut green, mut blue, mut alpha) = (0.0, 0.0, 0.0, 0.0);
    unsafe { srgb.getRed_green_blue_alpha(&mut red, &mut green, &mut blue, &mut alpha) };
    if alpha < 1.0 {
        return None;
    }
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([channel(red), channel(green), channel(blue)])
}

/// The window's first responder when it is a view.
fn first_responder_view(host: &NSView) -> Option<Retained<NSView>> {
    let responder: Retained<NSResponder> = host.window()?.firstResponder()?;
    if !responder.isKindOfClass(NSView::class()) {
        return None;
    }
    Some(unsafe { Retained::cast::<NSView>(responder) })
}

/// Whether a page view (not helm's own) holds the keyboard.
fn page_has_focus(host: &NSView) -> bool {
    let Some(mut view) = first_responder_view(host) else {
        return false;
    };
    loop {
        if view.isKindOfClass(WKWebView::class()) {
            return true;
        }
        match unsafe { view.superview() } {
            Some(parent) if &*parent != host => view = parent,
            _ => return false,
        }
    }
}

fn is_helm_chord(event: &NSEvent) -> bool {
    let flags = unsafe { event.modifierFlags() };
    if !flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand) {
        return false;
    }
    let typed = unsafe { event.charactersIgnoringModifiers() }
        .and_then(|chars| chars.to_string().chars().next());
    let Some(typed) = typed else {
        return true;
    };
    !keyboard::page_keeps(CmdChord {
        key: cmd_key(typed),
        alt: flags.contains(NSEventModifierFlags::NSEventModifierFlagOption),
        ctrl: flags.contains(NSEventModifierFlags::NSEventModifierFlagControl),
    })
}

fn cmd_key(typed: char) -> CmdKey {
    let arrows = [
        NSUpArrowFunctionKey,
        NSDownArrowFunctionKey,
        NSLeftArrowFunctionKey,
        NSRightArrowFunctionKey,
    ];
    match u32::from(typed) {
        code if code == NSDeleteCharacter => CmdKey::Backspace,
        code if code == NSDeleteFunctionKey => CmdKey::ForwardDelete,
        code if arrows.contains(&code) => CmdKey::Arrow,
        _ => CmdKey::Char(typed),
    }
}

/// The Edit menu winit leaves out: a page copies and pastes only through its key equivalents.
fn add_edit_menu(mtm: MainThreadMarker) {
    let Some(menu_bar) = (unsafe { NSApplication::sharedApplication(mtm).mainMenu() }) else {
        return;
    };
    let title = ns_string!("Edit");
    if unsafe { menu_bar.itemWithTitle(title) }.is_some() {
        return;
    }
    let menu = unsafe { NSMenu::initWithTitle(mtm.alloc(), title) };
    let entries = [
        (ns_string!("Undo"), sel!(undo:), ns_string!("z")),
        (ns_string!("Redo"), sel!(redo:), ns_string!("Z")),
        (ns_string!("Cut"), sel!(cut:), ns_string!("x")),
        (ns_string!("Copy"), sel!(copy:), ns_string!("c")),
        (ns_string!("Paste"), sel!(paste:), ns_string!("v")),
        (ns_string!("Select All"), sel!(selectAll:), ns_string!("a")),
    ];
    for (item_title, action, key) in entries {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                item_title,
                Some(action),
                key,
            )
        };
        menu.addItem(&item);
    }
    let edit_item = NSMenuItem::new(mtm);
    edit_item.setSubmenu(Some(&menu));
    menu_bar.addItem(&edit_item);
}

/// Sends helm's `Cmd` chords typed in a page to the egui view instead
/// (browser.md §6). The page keeps its first-responder status, so the pane stays
/// the focused one while helm handles the chord.
fn route_helm_chords(host: Retained<NSView>) -> Option<Retained<AnyObject>> {
    let routed: Rc<RefCell<HashSet<u16>>> = Rc::default();
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let ev = unsafe { event.as_ref() };
        if !page_has_focus(&host) {
            return event.as_ptr();
        }
        let key_code = unsafe { ev.keyCode() };
        let event_type = unsafe { ev.r#type() };
        unsafe {
            if event_type == NSEventType::FlagsChanged {
                host.flagsChanged(ev);
            } else if event_type == NSEventType::KeyDown && is_helm_chord(ev) {
                routed.borrow_mut().insert(key_code);
                host.keyDown(ev);
                return std::ptr::null_mut();
            } else if event_type == NSEventType::KeyUp && routed.borrow_mut().remove(&key_code) {
                host.keyUp(ev);
                return std::ptr::null_mut();
            }
        }
        event.as_ptr()
    });
    let mask = NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged;
    unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }
}

/// The dialog a page waits on, and how to answer it.
struct AskedDialog {
    dialog: PageDialog,
    reply: DialogReply,
}

/// The completion handler WebKit waits on: called once, always.
enum DialogReply {
    Alert(RcBlock<dyn Fn()>),
    Confirm(RcBlock<dyn Fn(Bool)>),
    Prompt(RcBlock<dyn Fn(*mut NSString)>),
}

impl DialogReply {
    fn send(self, answer: &DialogAnswer) {
        let accepted = matches!(answer, DialogAnswer::Accept(_));
        match (self, answer) {
            (Self::Alert(reply), _) => reply.call(()),
            (Self::Confirm(reply), _) => reply.call((Bool::new(accepted),)),
            (Self::Prompt(reply), DialogAnswer::Accept(text)) => {
                let text = NSString::from_str(text);
                reply.call((Retained::as_ptr(&text).cast_mut(),));
            }
            (Self::Prompt(reply), DialogAnswer::Cancel) => reply.call((std::ptr::null_mut(),)),
        }
    }
}

struct PageUiIvars {
    host: Retained<NSView>,
    wake: Wake,
    /// Popups WebKit opened, not yet taken by a tab.
    opened: RefCell<Vec<WebKitView>>,
    dialog: RefCell<Option<AskedDialog>>,
    /// The page called `window.close()`.
    closed: Cell<bool>,
}

declare_class!(
    /// A page's popups, dialogs and file pickers (browser.md §2, §12).
    struct PageUi;

    unsafe impl ClassType for PageUi {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HelmBrowserPageUi";
    }

    impl DeclaredClass for PageUi {
        type Ivars = PageUiIvars;
    }

    unsafe impl NSObjectProtocol for PageUi {}

    unsafe impl WKUIDelegate for PageUi {
        #[method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:)]
        fn create_web_view(
            &self,
            _web_view: &WKWebView,
            configuration: &WKWebViewConfiguration,
            _action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            let ivars = self.ivars();
            let popup = WebKitView::with_configuration(
                &ivars.host,
                configuration,
                ivars.wake.clone(),
                MainThreadMarker::from(self),
            );
            let view = popup.view.clone();
            ivars.opened.borrow_mut().push(popup);
            (ivars.wake)();
            Some(view)
        }

        #[method(webViewDidClose:)]
        fn did_close(&self, _web_view: &WKWebView) {
            self.ivars().closed.set(true);
            (self.ivars().wake)();
        }

        #[method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:)]
        fn alert(
            &self,
            web_view: &WKWebView,
            message: &NSString,
            _frame: &WKFrameInfo,
            reply: &Block<dyn Fn()>,
        ) {
            self.ask(web_view, DialogKind::Alert, message, DialogReply::Alert(reply.copy()));
        }

        #[method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:)]
        fn confirm(
            &self,
            web_view: &WKWebView,
            message: &NSString,
            _frame: &WKFrameInfo,
            reply: &Block<dyn Fn(Bool)>,
        ) {
            self.ask(web_view, DialogKind::Confirm, message, DialogReply::Confirm(reply.copy()));
        }

        #[method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:)]
        fn prompt(
            &self,
            web_view: &WKWebView,
            message: &NSString,
            default: Option<&NSString>,
            _frame: &WKFrameInfo,
            reply: &Block<dyn Fn(*mut NSString)>,
        ) {
            let kind = DialogKind::Prompt {
                default: default.map(NSString::to_string).unwrap_or_default(),
            };
            self.ask(web_view, kind, message, DialogReply::Prompt(reply.copy()));
        }

        #[method(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:)]
        fn open_panel(
            &self,
            web_view: &WKWebView,
            parameters: &WKOpenPanelParameters,
            _frame: &WKFrameInfo,
            reply: &Block<dyn Fn(*mut NSArray<NSURL>)>,
        ) {
            choose_files(web_view, parameters, reply.copy(), MainThreadMarker::from(self));
        }
    }
);

impl PageUi {
    fn new(host: Retained<NSView>, wake: Wake, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(PageUiIvars {
            host,
            wake,
            opened: RefCell::default(),
            dialog: RefCell::default(),
            closed: Cell::new(false),
        });
        unsafe { msg_send_id![super(this), init] }
    }

    fn ask(&self, web_view: &WKWebView, kind: DialogKind, message: &NSString, reply: DialogReply) {
        let dialog = PageDialog {
            kind,
            message: message.to_string(),
            site: favicons::site_of(&view_url(web_view))
                .unwrap_or_default()
                .to_owned(),
        };
        let earlier = self
            .ivars()
            .dialog
            .replace(Some(AskedDialog { dialog, reply }));
        if let Some(earlier) = earlier {
            earlier.reply.send(&DialogAnswer::Cancel);
        }
        (self.ivars().wake)();
    }
}

fn view_url(view: &WKWebView) -> String {
    unsafe { view.URL().and_then(|url| url.absoluteString()) }
        .map(|url| url.to_string())
        .unwrap_or_default()
}

/// The page's `<input type="file">`: the open panel, as a sheet on the window.
fn choose_files(
    web_view: &WKWebView,
    parameters: &WKOpenPanelParameters,
    reply: RcBlock<dyn Fn(*mut NSArray<NSURL>)>,
    mtm: MainThreadMarker,
) {
    let Some(window) = web_view.window() else {
        reply.call((std::ptr::null_mut(),));
        return;
    };
    let panel = unsafe { NSOpenPanel::openPanel(mtm) };
    unsafe {
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(parameters.allowsDirectories());
        panel.setAllowsMultipleSelection(parameters.allowsMultipleSelection());
    }
    let chosen = panel.clone();
    let done = RcBlock::new(move |response: NSModalResponse| {
        if response != NSModalResponseOK {
            reply.call((std::ptr::null_mut(),));
            return;
        }
        let urls = unsafe { chosen.URLs() };
        reply.call((Retained::as_ptr(&urls).cast_mut(),));
    });
    unsafe { panel.beginSheetModalForWindow_completionHandler(&window, &done) };
}

struct NavigationIvars {
    wake: Wake,
    failure: RefCell<Option<LoadFailure>>,
    downloads: Retained<DownloadTracker>,
}

declare_class!(
    /// A page's failed loads and the responses it saves instead of showing
    /// (browser.md §13, §14).
    struct PageNavigation;

    unsafe impl ClassType for PageNavigation {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HelmBrowserPageNavigation";
    }

    impl DeclaredClass for PageNavigation {
        type Ivars = NavigationIvars;
    }

    unsafe impl NSObjectProtocol for PageNavigation {}

    unsafe impl WKNavigationDelegate for PageNavigation {
        #[method(webView:decidePolicyForNavigationAction:decisionHandler:)]
        fn decide_action(
            &self,
            _web_view: &WKWebView,
            action: &WKNavigationAction,
            decide: &Block<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let downloads = action.respondsToSelector(sel!(shouldPerformDownload))
                && unsafe { action.shouldPerformDownload() };
            decide.call((if downloads {
                WKNavigationActionPolicy::Download
            } else {
                WKNavigationActionPolicy::Allow
            },));
        }

        #[method(webView:decidePolicyForNavigationResponse:decisionHandler:)]
        fn decide_response(
            &self,
            _web_view: &WKWebView,
            response: &WKNavigationResponse,
            decide: &Block<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            let served = unsafe { response.response() };
            let saved = !unsafe { response.canShowMIMEType() } || is_attachment(&served);
            decide.call((if saved {
                WKNavigationResponsePolicy::Download
            } else {
                WKNavigationResponsePolicy::Allow
            },));
        }

        #[method(webView:navigationAction:didBecomeDownload:)]
        fn action_became_download(
            &self,
            _web_view: &WKWebView,
            _action: &WKNavigationAction,
            download: &WKDownload,
        ) {
            self.ivars().downloads.track(download);
        }

        #[method(webView:navigationResponse:didBecomeDownload:)]
        fn response_became_download(
            &self,
            _web_view: &WKWebView,
            _response: &WKNavigationResponse,
            download: &WKDownload,
        ) {
            self.ivars().downloads.track(download);
        }

        #[method(webView:didStartProvisionalNavigation:)]
        fn did_start(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            self.ivars().failure.replace(None);
        }

        #[method(webView:didFailProvisionalNavigation:withError:)]
        fn did_fail(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            let reason = FailureReason::of_error(
                &error.domain().to_string(),
                error.code() as i64,
                &error.localizedDescription().to_string(),
            );
            if let Some(reason) = reason {
                let url = failing_url(error).unwrap_or_else(|| view_url(web_view));
                self.fail(LoadFailure { url, reason });
            }
        }

        #[method(webViewWebContentProcessDidTerminate:)]
        fn did_crash(&self, web_view: &WKWebView) {
            self.fail(LoadFailure {
                url: view_url(web_view),
                reason: FailureReason::Crashed,
            });
        }
    }
);

impl PageNavigation {
    fn new(wake: Wake, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(NavigationIvars {
            downloads: DownloadTracker::new(wake.clone(), mtm),
            wake,
            failure: RefCell::default(),
        });
        unsafe { msg_send_id![super(this), init] }
    }

    fn fail(&self, failure: LoadFailure) {
        self.ivars().failure.replace(Some(failure));
        (self.ivars().wake)();
    }
}

/// The URL a navigation failed on, as the error carries it.
fn failing_url(error: &NSError) -> Option<String> {
    let info = error.userInfo();
    let value = info.get(ns_string!("NSErrorFailingURLStringKey"))?;
    let is_string: bool = unsafe { msg_send![value, isKindOfClass: NSString::class()] };
    if !is_string {
        return None;
    }
    let text: &NSString = unsafe { &*(value as *const AnyObject).cast::<NSString>() };
    Some(text.to_string())
}

fn is_attachment(response: &NSURLResponse) -> bool {
    if !response.isKindOfClass(NSHTTPURLResponse::class()) {
        return false;
    }
    let http: &NSHTTPURLResponse = unsafe { &*(response as *const NSURLResponse).cast() };
    unsafe { http.valueForHTTPHeaderField(ns_string!("Content-Disposition")) }
        .is_some_and(|disposition| download::is_attachment(&disposition.to_string()))
}

/// A download and what the shelf shows of it.
struct Tracked {
    download: Retained<WKDownload>,
    record: Download,
}

struct DownloadIvars {
    wake: Wake,
    tracked: RefCell<Vec<Tracked>>,
}

declare_class!(
    /// Writes a page's downloads into the Downloads folder (browser.md §13).
    struct DownloadTracker;

    unsafe impl ClassType for DownloadTracker {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HelmBrowserDownloadTracker";
    }

    impl DeclaredClass for DownloadTracker {
        type Ivars = DownloadIvars;
    }

    unsafe impl NSObjectProtocol for DownloadTracker {}

    unsafe impl WKDownloadDelegate for DownloadTracker {
        #[method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:)]
        fn decide_destination(
            &self,
            download: &WKDownload,
            _response: &NSURLResponse,
            suggested: &NSString,
            reply: &Block<dyn Fn(*mut NSURL)>,
        ) {
            let path = download::destination(&download::downloads_dir(), &suggested.to_string());
            let url = unsafe { NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy())) };
            self.edit(download, |record| {
                record.file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                record.path = Some(path.clone());
            });
            reply.call((Retained::as_ptr(&url).cast_mut(),));
        }

        #[method(downloadDidFinish:)]
        fn did_finish(&self, download: &WKDownload) {
            self.edit(download, |record| record.status = DownloadStatus::Done);
        }

        #[method(download:didFailWithError:resumeData:)]
        fn did_fail(&self, download: &WKDownload, error: &NSError, _resume: Option<&NSData>) {
            let reason = error.localizedDescription().to_string();
            self.edit(download, |record| record.status = DownloadStatus::Failed(reason.clone()));
        }
    }
);

impl DownloadTracker {
    fn new(wake: Wake, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(DownloadIvars {
            wake,
            tracked: RefCell::default(),
        });
        unsafe { msg_send_id![super(this), init] }
    }

    fn track(&self, download: &WKDownload) {
        unsafe { download.setDelegate(Some(ProtocolObject::from_ref(self))) };
        let mut tracked = self.ivars().tracked.borrow_mut();
        let file_name = unsafe { download.originalRequest() }
            .and_then(|request| unsafe { request.URL() })
            .and_then(|url| unsafe { url.lastPathComponent() })
            .map(|name| name.to_string())
            .unwrap_or_default();
        let id = tracked.len() as u64;
        tracked.push(Tracked {
            download: download.retain(),
            record: Download {
                id,
                file_name,
                path: None,
                status: DownloadStatus::Running(None),
            },
        });
        drop(tracked);
        (self.ivars().wake)();
    }

    fn edit(&self, download: &WKDownload, edit: impl FnOnce(&mut Download)) {
        let mut tracked = self.ivars().tracked.borrow_mut();
        let Some(entry) = tracked
            .iter_mut()
            .find(|entry| std::ptr::eq(&*entry.download, download))
        else {
            return;
        };
        edit(&mut entry.record);
        drop(tracked);
        (self.ivars().wake)();
    }

    /// Each download, its progress read now.
    fn downloads(&self) -> Vec<Download> {
        self.ivars()
            .tracked
            .borrow()
            .iter()
            .map(|entry| {
                let mut record = entry.record.clone();
                if let DownloadStatus::Running(_) = record.status {
                    let progress = unsafe { entry.download.progress() };
                    let known = unsafe { progress.totalUnitCount() } > 0;
                    record.status = DownloadStatus::Running(
                        known.then(|| unsafe { progress.fractionCompleted() } as f32),
                    );
                }
                record
            })
            .collect()
    }
}

declare_class!(
    /// Receives the numbers a page's reporter posts: scroll offsets, pointer.
    struct NumberReporter;

    unsafe impl ClassType for NumberReporter {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HelmBrowserNumberReporter";
    }

    impl DeclaredClass for NumberReporter {
        type Ivars = Rc<Cell<f64>>;
    }

    unsafe impl NSObjectProtocol for NumberReporter {}

    unsafe impl WKScriptMessageHandler for NumberReporter {
        #[method(userContentController:didReceiveScriptMessage:)]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            let body = unsafe { message.body() };
            let is_number: bool = unsafe { msg_send![&*body, isKindOfClass: NSNumber::class()] };
            if !is_number {
                return;
            }
            let number = unsafe { Retained::cast::<NSNumber>(body) }.as_f64();
            self.ivars().set(number);
        }
    }
);

declare_class!(
    /// Receives the page URL and icon the page's reporter posts.
    struct IconReporter;

    unsafe impl ClassType for IconReporter {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HelmBrowserIconReporter";
    }

    impl DeclaredClass for IconReporter {
        type Ivars = Rc<RefCell<Option<(String, String)>>>;
    }

    unsafe impl NSObjectProtocol for IconReporter {}

    unsafe impl WKScriptMessageHandler for IconReporter {
        #[method(userContentController:didReceiveScriptMessage:)]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            let body = unsafe { message.body() };
            let is_string: bool = unsafe { msg_send![&*body, isKindOfClass: NSString::class()] };
            if !is_string {
                return;
            }
            let posted = unsafe { Retained::cast::<NSString>(body) }.to_string();
            let reported = posted.split_once('\n').and_then(|(page, icon)| {
                let site = favicons::site_of(page)?;
                Some((site.to_owned(), icon.to_owned()))
            });
            *self.ivars().borrow_mut() = reported;
        }
    }
);

impl IconReporter {
    fn new(icon: Rc<RefCell<Option<(String, String)>>>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(icon);
        unsafe { msg_send_id![super(this), init] }
    }
}

impl NumberReporter {
    fn new(value: Rc<Cell<f64>>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(value);
        unsafe { msg_send_id![super(this), init] }
    }
}
