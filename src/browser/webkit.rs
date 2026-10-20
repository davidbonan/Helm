//! WebKit adapter of the engine port (browser.md §2): a `WKWebView` per pane,
//! in its own frame view added over the window's content view.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use core_graphics::base::kCGImageAlphaPremultipliedLast;
use core_graphics::color_space::{kCGColorSpaceSRGB, CGColorSpace};
use core_graphics::context::CGContext;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::image::CGImageRef;
use foreign_types::ForeignTypeRef;
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColorSpace, NSEvent, NSEventMask, NSEventModifierFlags,
    NSEventType, NSImage, NSResponder, NSView,
};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSDate, NSDictionary, NSError, NSHTTPCookie, NSHTTPCookieDomain,
    NSHTTPCookieExpires, NSHTTPCookieName, NSHTTPCookiePath, NSHTTPCookieSecure, NSHTTPCookieValue,
    NSNumber, NSObject, NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSURL,
};
use objc2_web_kit::{
    WKFindConfiguration, WKFindResult, WKNavigationAction, WKScriptMessage, WKScriptMessageHandler,
    WKSnapshotConfiguration, WKUIDelegate, WKUserContentController, WKUserScript,
    WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
    WKWindowFeatures,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{favicons, Cookie, Navigation, PageSnapshot, PageState, ViewRect, WebEngine, WebView};

/// Chords the page keeps while it has focus (browser.md §6); the system ones
/// (`Cmd+Q`, `Cmd+H`, `Cmd+M`) go on to the app menu.
const PAGE_CHORDS: [&str; 8] = ["c", "v", "x", "a", "z", "q", "h", "m"];

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

pub struct WebKitEngine {
    host: Retained<NSView>,
    monitor: Retained<AnyObject>,
    mtm: MainThreadMarker,
}

impl WebKitEngine {
    /// The engine of the window behind `window`; `None` off the main thread or
    /// without an AppKit window.
    pub fn attach(window: &impl HasWindowHandle) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(appkit) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        let host: Retained<NSView> =
            unsafe { Retained::retain(appkit.ns_view.cast::<NSView>().as_ptr())? };
        let monitor = route_helm_chords(host.clone())?;
        Some(Self { host, monitor, mtm })
    }
}

impl Drop for WebKitEngine {
    fn drop(&mut self) {
        unsafe { NSEvent::removeMonitor(&self.monitor) };
    }
}

impl WebEngine for WebKitEngine {
    fn open(&self) -> Box<dyn WebView> {
        Box::new(WebKitView::new(&self.host, self.mtm))
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
    frame: Retained<NSView>,
    view: Retained<WKWebView>,
    host: Retained<NSView>,
    _popups: Retained<OpenInPlace>,
    /// What the page's scroll reporter last posted.
    scroll_y: Rc<Cell<f64>>,
    /// What the page's pointer reporter last posted: 1 while over the page.
    pointer: Rc<Cell<f64>>,
    /// What the page's icon reporter last posted: the page's site, its icon.
    icon: Rc<RefCell<Option<(String, String)>>>,
    /// Hidden while it had focus: focus comes back with it.
    refocus_on_show: bool,
    /// The bottom corners' rounding last given to the frame's layer.
    bottom_radius: f64,
    /// The snapshot asked last, once WebKit hands it.
    snapshot: Rc<RefCell<Option<PageSnapshot>>>,
    /// Counts the snapshots asked: an answer to an older one is dropped.
    snapshot_asks: Rc<Cell<u64>>,
    /// Whether the last find matched, once WebKit answers.
    find_result: Rc<Cell<Option<bool>>>,
}

impl WebKitView {
    fn new(host: &Retained<NSView>, mtm: MainThreadMarker) -> Self {
        let scroll_y = Rc::new(Cell::new(0.0));
        let pointer = Rc::new(Cell::new(0.0));
        let icon = Rc::default();
        let view = unsafe {
            let config = WKWebViewConfiguration::new();
            config.setWebsiteDataStore(&WKWebsiteDataStore::defaultDataStore());
            // Private KVC key behind the "Inspect Element" context menu entry.
            let _: () = msg_send![
                &*config.preferences(),
                setValue: &*NSNumber::numberWithBool(true),
                forKey: ns_string!("developerExtrasEnabled")
            ];
            report_number(&config, &NumberReport::SCROLL, &scroll_y, mtm);
            report_number(&config, &NumberReport::POINTER, &pointer, mtm);
            report_icon(&config, &icon, mtm);
            WKWebView::initWithFrame_configuration(mtm.alloc(), NSRect::ZERO, &config)
        };
        let popups = OpenInPlace::new(mtm);
        let frame = unsafe { NSView::initWithFrame(mtm.alloc(), NSRect::ZERO) };
        unsafe {
            view.setUIDelegate(Some(ProtocolObject::from_ref(&*popups)));
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
            _popups: popups,
            scroll_y,
            pointer,
            icon,
            refocus_on_show: false,
            bottom_radius: 0.0,
            snapshot: Rc::default(),
            snapshot_asks: Rc::default(),
            find_result: Rc::default(),
        }
    }
}

impl Drop for WebKitView {
    fn drop(&mut self) {
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

/// `kCALayerMinXMinYCorner | kCALayerMaxXMinYCorner`: the bottom corners of an
/// unflipped view's layer.
const BOTTOM_CORNERS: usize = 1 | 2;

/// Clips the page to the rounded bottom corners of the block it sits in.
fn round_bottom_corners(frame: &NSView, radius: f64) {
    let layer: Option<Retained<AnyObject>> = unsafe { msg_send_id![frame, layer] };
    let Some(layer) = layer else {
        return;
    };
    unsafe {
        let _: () = msg_send![&*layer, setCornerRadius: radius];
        let _: () = msg_send![&*layer, setMaskedCorners: BOTTOM_CORNERS];
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
        if self.bottom_radius != rect.bottom_radius {
            round_bottom_corners(&self.frame, rect.bottom_radius);
            self.bottom_radius = rect.bottom_radius;
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
        unsafe {
            PageState {
                url,
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
    let shift = flags.contains(NSEventModifierFlags::NSEventModifierFlagShift);
    let key = unsafe { event.charactersIgnoringModifiers() }
        .map(|chars| chars.to_string().to_lowercase())
        .unwrap_or_default();
    if key == "z" && shift {
        return false;
    }
    !PAGE_CHORDS.contains(&key.as_str())
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

declare_class!(
    /// `target="_blank"` and `window.open` load in the same pane (browser.md §2).
    struct OpenInPlace;

    unsafe impl ClassType for OpenInPlace {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HelmBrowserOpenInPlace";
    }

    impl DeclaredClass for OpenInPlace {}

    unsafe impl NSObjectProtocol for OpenInPlace {}

    unsafe impl WKUIDelegate for OpenInPlace {
        #[method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:)]
        fn create_web_view(
            &self,
            web_view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            unsafe { web_view.loadRequest(&action.request()) };
            None
        }
    }
);

impl OpenInPlace {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send_id![super(this), init] }
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
