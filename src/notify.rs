//! macOS completion notifications (specs/agents.md §5): when a watched agent
//! finishes a turn, surface a native banner. Two backends behind [`post`].
//!
//! From the `.app`, `UserNotifications` posts under helm's own bundle identity.
//! That is the only backend the system attributes to *helm*: `osascript` posts
//! as **Script Editor**, so the banner carries the wrong app, cannot be tuned in
//! System Settings › Notifications under helm's name, and — the reason this
//! exists — cannot be allowlisted in a Focus mode, which silently suppresses it
//! (`donotdisturbd: outcome: suppressed`, notification filed straight to
//! history).
//!
//! Outside a bundle (`cargo run`, tests) the process has no bundle identifier
//! and `UNUserNotificationCenter` raises, so the `osascript` path stays as the
//! fallback: no entitlement needed, and testable with a fake `osascript`
//! capturing its argv (mirrors `feedback.rs`). Pure domain: no egui dependency.

use std::path::Path;

use crate::git::cli::{self, CliError, CliOutput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyError {
    /// `osascript` binary absent (not macOS, or a broken PATH).
    NotFound,
    /// `osascript` ran but failed to post the notification.
    Failed(String),
}

impl NotifyError {
    pub fn message(&self) -> String {
        match self {
            NotifyError::NotFound => "could not post a notification".to_owned(),
            NotifyError::Failed(detail) => detail.clone(),
        }
    }
}

/// Prepares the `UserNotifications` backend: registers the presentation
/// delegate and asks for authorization (first launch shows the system prompt).
/// Called once at startup, on the main thread, once `NSApplication` exists —
/// [`post`] only reaches the framework after this proved it usable.
pub fn install() {
    #[cfg(target_os = "macos")]
    user_notifications::install();
}

/// Posts a native banner with `title` / `body`, fire-and-forget: the framework
/// call is a cheap async hand-off, and the `osascript` fallback is a ~100 ms
/// subprocess pushed off the UI thread.
pub fn post(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    if user_notifications::post(title, body) {
        return;
    }
    let (title, body) = (title.to_owned(), body.to_owned());
    std::thread::spawn(move || {
        let _ = notify(&title, &body);
    });
}

/// Blocking `osascript` fallback.
pub fn notify(title: &str, body: &str) -> Result<(), NotifyError> {
    notify_with(Path::new("osascript"), title, body)
}

/// Seam: explicit `osascript` path for e2e tests (a fake binary capturing its
/// argv, no banner posted).
pub fn notify_with(osascript: &Path, title: &str, body: &str) -> Result<(), NotifyError> {
    let script = display_notification_script(title, body);
    let output =
        cli::run_program(osascript, Path::new("/"), &["-e", &script]).map_err(|err| match err {
            CliError::NotFound => NotifyError::NotFound,
            CliError::TimedOut(duration) => {
                NotifyError::Failed(format!("osascript timed out after {}s", duration.as_secs()))
            }
            CliError::Io(err) => NotifyError::Failed(err.to_string()),
        })?;
    if !output.success() {
        return Err(NotifyError::Failed(failure_detail(&output)));
    }
    Ok(())
}

/// AppleScript one-liner: `display notification "body" with title "title"`. Both
/// operands are string literals — escaped so a quote or backslash in an agent /
/// repo name cannot break out of the literal.
fn display_notification_script(title: &str, body: &str) -> String {
    format!(
        "display notification \"{}\" with title \"{}\"",
        escape_applescript(body),
        escape_applescript(title),
    )
}

/// Escapes an AppleScript double-quoted string: backslash and quote are the only
/// metacharacters; a raw newline would terminate the `-e` line, so it is folded
/// to a space.
fn escape_applescript(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Title / body of an agent's completion banner: "Claude finished" + the project
/// (and its branch when known).
pub fn completion_message(agent: &str, repo: &str, branch: Option<&str>) -> (String, String) {
    let title = format!("{} finished", crate::agent_watch::display_name(agent));
    let body = match branch {
        Some(branch) if !branch.is_empty() => format!("{repo} · {branch}"),
        _ => repo.to_owned(),
    };
    (title, body)
}

fn failure_detail(output: &CliOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    match output.code {
        Some(code) => format!("osascript exit code {code}"),
        None => "osascript killed by a signal".to_owned(),
    }
}

/// `UserNotifications` backend: the banner is attributed to helm's bundle, so it
/// appears under *helm* in System Settings › Notifications and in a Focus mode's
/// allowed apps.
#[cfg(target_os = "macos")]
mod user_notifications {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use block2::{Block, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
    use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    /// Set once `install` obtained the center. In a process with no bundle,
    /// `currentNotificationCenter` raises an Objective-C exception
    /// (`bundleProxyForCurrentProcess is nil`) that Rust cannot catch — under
    /// `cargo run` it would abort. The flag confines that first, main-thread
    /// probe to startup; `post` never reaches the framework without it.
    static AVAILABLE: AtomicBool = AtomicBool::new(false);
    /// Notification identifiers must differ, or each banner replaces the last.
    static SEQ: AtomicU64 = AtomicU64::new(0);

    declare_class!(
        struct Delegate;

        unsafe impl ClassType for Delegate {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "HelmNotificationDelegate";
        }

        impl DeclaredClass for Delegate {}

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[method(userNotificationCenter:willPresentNotification:withCompletionHandler:)]
            unsafe fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &Block<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                // Without this the banner is dropped whenever helm is the
                // frontmost app — the common case here, since the agent that
                // just finished runs in one of helm's own background tabs.
                completion.call((UNNotificationPresentationOptions::UNNotificationPresentationOptionBanner
                    | UNNotificationPresentationOptions::UNNotificationPresentationOptionList
                    | UNNotificationPresentationOptions::UNNotificationPresentationOptionSound,));
            }
        }
    );

    /// A bundle identifier is what UserNotifications keys the app on; the `.app`
    /// check rejects the bare `cargo run` binary, whose main bundle is its
    /// containing directory.
    fn bundled() -> bool {
        let bundle = NSBundle::mainBundle();
        unsafe {
            bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
        }
    }

    pub fn install() {
        if !bundled() {
            return;
        }
        unsafe {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let delegate: Retained<Delegate> = msg_send_id![Delegate::alloc(), init];
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            // The center holds its delegate unretained, and this one must
            // outlive every notification, i.e. the whole process.
            std::mem::forget(delegate);
            // A refusal is the user's answer, not an error to handle: macOS then
            // drops the notifications itself, and helm is listed in System
            // Settings › Notifications to re-enable — which is what this backend
            // buys. Only a framework-level failure is worth a line, since it is
            // otherwise indistinguishable from a banner nobody looked at.
            let handler = RcBlock::new(|_granted: Bool, err: *mut NSError| {
                if let Some(err) = err.as_ref() {
                    eprintln!("helm: notification authorization failed: {err}");
                }
            });
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::UNAuthorizationOptionAlert
                    | UNAuthorizationOptions::UNAuthorizationOptionSound,
                &handler,
            );
        }
        AVAILABLE.store(true, Ordering::Relaxed);
    }

    /// `false` when the backend is unavailable, leaving the caller its
    /// `osascript` fallback.
    pub fn post(title: &str, body: &str) -> bool {
        if !AVAILABLE.load(Ordering::Relaxed) {
            return false;
        }
        unsafe {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(title));
            content.setBody(&NSString::from_str(body));
            content.setSound(Some(&UNNotificationSound::defaultSound()));
            let id = NSString::from_str(&format!(
                "helm-agent-{}",
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let request =
                UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, None);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_wraps_both_operands_as_applescript_literals() {
        assert_eq!(
            display_notification_script("Claude finished", "helm · main"),
            "display notification \"helm · main\" with title \"Claude finished\"",
        );
    }

    #[test]
    fn escape_neutralises_quotes_backslashes_and_newlines() {
        assert_eq!(escape_applescript("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(
            escape_applescript("line one\nline two"),
            "line one line two"
        );
        // The escaped script never carries a raw newline (would split `-e`).
        let script = display_notification_script("t\"itle", "bo\ndy");
        assert!(!script.contains('\n'), "{script}");
        assert!(
            !script.contains("t\"itle"),
            "unescaped quote leaks: {script}"
        );
    }

    #[test]
    fn completion_message_capitalises_and_appends_the_branch() {
        let (title, body) = completion_message("claude", "helm", Some("main"));
        assert_eq!(title, "Claude finished");
        assert_eq!(body, "helm · main");

        // No branch (or an empty one) ⇒ the repo name alone.
        let (_, body) = completion_message("codex", "api", None);
        assert_eq!(body, "api");
        let (_, body) = completion_message("codex", "api", Some(""));
        assert_eq!(body, "api");
    }

    #[test]
    fn an_error_renders_a_message() {
        assert_eq!(
            NotifyError::NotFound.message(),
            "could not post a notification"
        );
        assert_eq!(NotifyError::Failed("boom".into()).message(), "boom");
    }
}
