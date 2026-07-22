//! Inbound `helm://` targets (specs/cli.md §4).
//!
//! LaunchServices delivers the URL as an Apple Event on the main thread, at any
//! time — including before the first frame exists on a cold launch. The event
//! handler therefore only parks the target here and wakes the event loop; the
//! app drains it at the top of its next frame.

use std::path::PathBuf;
use std::sync::Mutex;

static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);
static REPAINTER: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Lets a URL arriving while the app is idle wake the event loop.
pub fn arm(ctx: &egui::Context) {
    if let Ok(mut repainter) = REPAINTER.lock() {
        *repainter = Some(ctx.clone());
    }
}

/// Parks a target from a `helm://` URL. An unparsable URL is dropped in
/// silence: the CLI already validated on its side, and anything else reaching
/// us is untrusted input, not a user error to report.
pub fn push_url(url: &str) {
    let Some(target) = crate::cli::target_from_url(url) else {
        return;
    };
    if let Ok(mut pending) = PENDING.lock() {
        // A burst of URLs collapses to the last one: they are all "activate this
        // row", and only the final one is what the user is looking at.
        *pending = Some(target);
    }
    if let Ok(repainter) = REPAINTER.lock() {
        if let Some(ctx) = repainter.as_ref() {
            ctx.request_repaint();
        }
    }
}

pub fn take() -> Option<PathBuf> {
    PENDING.lock().ok()?.take()
}

/// Registers the `kAEGetURL` handler. Called once, before the event loop starts,
/// so a cold launch's own URL is not missed.
#[cfg(target_os = "macos")]
pub fn install_handler() {
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::AnyObject;
    use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSObject};

    // 'GURL' (event class and id) and '----' (keyDirectObject), the four-char
    // codes of the Internet event suite.
    const INTERNET_EVENT_CLASS: u32 = 0x4755_524C;
    const AE_GET_URL: u32 = 0x4755_524C;
    const KEY_DIRECT_OBJECT: u32 = 0x2D2D_2D2D;

    declare_class!(
        struct UrlHandler;

        unsafe impl ClassType for UrlHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "HelmUrlHandler";
        }

        impl DeclaredClass for UrlHandler {}

        unsafe impl UrlHandler {
            #[method_id(init)]
            fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
                let this = this.set_ivars(());
                unsafe { msg_send_id![super(this), init] }
            }

            #[method(handleGetURLEvent:withReplyEvent:)]
            unsafe fn handle_get_url(
                &self,
                event: &NSAppleEventDescriptor,
                _reply: &NSAppleEventDescriptor,
            ) {
                let param: Option<Retained<NSAppleEventDescriptor>> =
                    msg_send_id![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT];
                let Some(url) = param.and_then(|p| p.stringValue()) else {
                    return;
                };
                push_url(&url.to_string());
            }
        }
    );

    // Leaked on purpose: the manager keeps an unretained reference to its
    // handler, and this one must outlive every event, i.e. the whole process.
    let handler: Retained<UrlHandler> = unsafe { msg_send_id![UrlHandler::alloc(), init] };
    let handler: *mut AnyObject = Retained::into_raw(handler).cast();
    unsafe {
        let manager = NSAppleEventManager::sharedAppleEventManager();
        let _: () = msg_send![
            &*manager,
            setEventHandler: &*handler,
            andSelector: sel!(handleGetURLEvent:withReplyEvent:),
            forEventClass: INTERNET_EVENT_CLASS,
            andEventID: AE_GET_URL,
        ];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install_handler() {}
