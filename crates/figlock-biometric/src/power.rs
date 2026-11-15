//! Telling the app that the Mac is going to sleep or the person's screen is being locked, so the vault can lock
//! before the screen is seen again (ADR-0030). Elsewhere nothing is watched and the app's own clock check is
//! the only defence.

/// Call `on_event` whenever the Mac is about to sleep, the display goes to sleep, the screen is locked or
/// this user's session is switched away. It may run on any thread, so it must be cheap and `Send`.
#[cfg(target_os = "macos")]
pub fn watch_sleep(on_event: impl Fn() + Send + Sync + 'static) {
    imp::watch(on_event);
}

#[cfg(not(target_os = "macos"))]
pub fn watch_sleep(_on_event: impl Fn() + Send + Sync + 'static) {}

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "registers notification observers with AppKit")]
mod imp {
    use std::ptr::NonNull;
    use std::sync::Arc;

    use block2::RcBlock;
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceScreensDidSleepNotification,
        NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::{
        NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSNotificationName,
        NSString,
    };

    pub fn watch(on_event: impl Fn() + Send + Sync + 'static) {
        let on_event = Arc::new(on_event);
        let block = RcBlock::new(move |_: NonNull<NSNotification>| on_event());
        let observe = |center: &NSNotificationCenter, name: &NSNotificationName| {
            // SAFETY: the block only calls a `Send + Sync` closure, and is copied by the center, which keeps it
            // for the life of the process: the returned observer token is deliberately never removed.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            };
            std::mem::forget(token);
        };
        // SAFETY: the notification names are framework constants, valid for the life of the process.
        unsafe {
            let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
            observe(&workspace, NSWorkspaceWillSleepNotification);
            observe(&workspace, NSWorkspaceScreensDidSleepNotification);
            observe(&workspace, NSWorkspaceSessionDidResignActiveNotification);
        }
        // The lock screen has no public workspace notification; this one is what the system posts for it.
        let locked = NSString::from_str("com.apple.screenIsLocked");
        observe(&NSDistributedNotificationCenter::defaultCenter(), &locked);
    }
}
