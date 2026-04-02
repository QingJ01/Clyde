#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSWindow, NSWindowCollectionBehavior, NSWorkspace,
    NSWorkspaceActiveSpaceDidChangeNotification,
};
#[cfg(target_os = "macos")]
use core::ptr::NonNull;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSNotification, NSThread};

/// Applies Space-follow behavior to a window.
///
/// Safety note (macOS/AppKit): `NSWindow` calls must run on the AppKit main
/// thread. This helper checks `NSThread::isMainThread` and no-ops when called
/// off-main-thread, because this API shape cannot hop threads or surface errors.
///
/// On macOS, this enables `NSWindowCollectionBehaviorMoveToActiveSpace` and
/// clears `NSWindowCollectionBehaviorCanJoinAllSpaces` so the single window
/// follows the currently active Space instead of appearing on all Spaces.
/// On non-macOS platforms, this is a no-op.
pub fn apply_space_follow(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    unsafe {
        let Some(ns_window) = ns_window(window, "apply_space_follow") else {
            return;
        };

        let current_behavior = ns_window.collectionBehavior();
        let updated_behavior = (current_behavior | NSWindowCollectionBehavior::MoveToActiveSpace)
            & !NSWindowCollectionBehavior::CanJoinAllSpaces;

        if updated_behavior != current_behavior {
            ns_window.setCollectionBehavior(updated_behavior);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
    }
}

/// Refreshes an existing window after a Space switch without activating app focus.
///
/// This should run on AppKit main thread.
#[allow(dead_code)]
pub fn refresh_space_follow(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    unsafe {
        let Some(ns_window) = ns_window(window, "refresh_space_follow") else {
            return;
        };

        let current_behavior = ns_window.collectionBehavior();
        let updated_behavior = (current_behavior | NSWindowCollectionBehavior::MoveToActiveSpace)
            & !NSWindowCollectionBehavior::CanJoinAllSpaces;
        if updated_behavior != current_behavior {
            ns_window.setCollectionBehavior(updated_behavior);
        }

        if !ns_window.isOnActiveSpace() {
            ns_window.orderFrontRegardless();
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
    }
}

/// Installs a macOS active Space observer and invokes callback on changes.
///
/// The observer is intentionally leaked for process lifetime so notifications
/// remain active without additional global state plumbing.
#[allow(dead_code)]
pub fn install_active_space_observer<F>(on_change: F)
where
    F: Fn() + 'static,
{
    #[cfg(target_os = "macos")]
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            on_change();
        });
        let token = center.addObserverForName_object_queue_usingBlock(
            Some(&NSWorkspaceActiveSpaceDidChangeNotification),
            None,
            None,
            &block,
        );

        std::mem::forget(block);
        std::mem::forget(token);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = on_change;
    }
}

#[cfg(target_os = "macos")]
unsafe fn ns_window<'a>(window: &'a tauri::WebviewWindow, context: &str) -> Option<&'a NSWindow> {
    if !NSThread::isMainThread_class() {
        eprintln!("{context}: no-op because call is not on AppKit main thread");
        return None;
    }

    let ns_window_ptr = match window.ns_window() {
        Ok(ptr) => ptr as *mut NSWindow,
        Err(err) => {
            eprintln!("{context}: no-op because ns_window() failed: {err}");
            return None;
        }
    };

    if ns_window_ptr.is_null() {
        eprintln!("{context}: no-op because ns_window() returned null");
        return None;
    }

    Some(&*ns_window_ptr)
}
