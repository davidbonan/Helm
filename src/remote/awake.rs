//! Keeps the Mac reachable while phone access is on (specs/remote.md §5): no idle
//! **system** sleep and no App Nap; the display may still sleep and the screen lock.

use objc2::rc::Retained;
use objc2_foundation::{NSActivityOptions, NSObject, NSProcessInfo, NSString};

pub const REASON: &str = "helm phone access";

/// The activity lasts as long as this value.
pub struct KeepAwake {
    activity: Retained<NSObject>,
}

// NSProcessInfo activities may begin and end on any thread (Foundation documents
// the class as thread-safe); the server thread that notices the LAN address gone ends it.
unsafe impl Send for KeepAwake {}

impl KeepAwake {
    pub fn begin() -> Self {
        let activity = unsafe {
            NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                NSActivityOptions::NSActivityUserInitiated,
                &NSString::from_str(REASON),
            )
        };
        Self { activity }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        unsafe { NSProcessInfo::processInfo().endActivity(&self.activity) };
    }
}
