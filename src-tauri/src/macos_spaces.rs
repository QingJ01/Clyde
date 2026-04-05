#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSWindow, NSWindowCollectionBehavior, NSWindowLevel, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceActiveSpaceDidChangeNotification,
};
#[cfg(target_os = "macos")]
use core::ptr::NonNull;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSNotification, NSThread};

/// NSScreenSaverWindowLevel (1000) — the same level Electron uses for
/// `setAlwaysOnTop(true, 'screen-saver')`.
#[cfg(target_os = "macos")]
const OVERLAY_WINDOW_LEVEL: NSWindowLevel = 1000;

/// Desired collection behavior: visible on all Spaces (including fullscreen
/// Spaces and other monitors), acts as a fullscreen auxiliary window, and
/// stays stationary during Space-switch animations.
#[cfg(target_os = "macos")]
fn overlay_behavior(current: NSWindowCollectionBehavior) -> NSWindowCollectionBehavior {
    (current
        | NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::FullScreenAuxiliary
        | NSWindowCollectionBehavior::Stationary)
        & !NSWindowCollectionBehavior::MoveToActiveSpace
}

/// Promotes an NSWindow to an NSPanel subclass at runtime using
/// `object_setClass`.  The custom subclass overrides `styleMask` to include
/// `NonactivatingPanel`, which is the *only* way since macOS 10.14 to float
/// above fullscreen apps (plain NSWindow rejects this style mask bit).
///
/// The subclass is registered once and cached for the process lifetime.
#[cfg(target_os = "macos")]
unsafe fn promote_to_panel(ns_window: &NSWindow) {
    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
    use std::sync::Once;

    // The name for our dynamic subclass.
    static REGISTER: Once = Once::new();
    static mut PANEL_CLASS: *const AnyClass = std::ptr::null();

    REGISTER.call_once(|| {
        // Create a subclass of NSPanel (which is itself an NSWindow subclass).
        // Using NSPanel as superclass instead of NSWindow means the runtime
        // won't reject the NonactivatingPanel style mask.
        let superclass = AnyClass::get(c"NSPanel").expect("NSPanel class not found");
        let mut builder = ClassBuilder::new(c"ClydeOverlayPanel", superclass)
            .expect("failed to create ClydeOverlayPanel class");

        // Override -styleMask to always include NonactivatingPanel.
        // Use raw pointer to avoid lifetime issues with add_method.
        extern "C" fn override_style_mask(this: *mut AnyObject, _sel: Sel) -> usize {
            let superclass = AnyClass::get(c"NSPanel").unwrap();
            let super_mask: usize = unsafe {
                objc2::msg_send![super(unsafe { &*this }, superclass), styleMask]
            };
            super_mask | (1 << 7) // NonactivatingPanel
        }

        unsafe {
            builder.add_method(
                objc2::sel!(styleMask),
                override_style_mask as extern "C" fn(*mut AnyObject, Sel) -> usize,
            );
        }

        let cls = builder.register();
        unsafe { PANEL_CLASS = cls as *const AnyClass; }
    });

    let cls = unsafe { &*PANEL_CLASS };
    let obj = ns_window as *const NSWindow as *mut AnyObject;
    objc2::ffi::object_setClass(obj, cls as *const AnyClass);
}

/// Applies overlay behavior to a window so it is visible on all Spaces
/// (including fullscreen apps) and on every connected display.
///
/// This promotes the NSWindow to an NSPanel subclass (via runtime class swap),
/// sets the collection behavior for all-spaces + fullscreen-auxiliary, and
/// raises the window level to screen-saver.
///
/// On non-macOS platforms, this is a no-op.
pub fn apply_space_follow(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    unsafe {
        let Some(ns_window) = ns_window(window, "apply_space_follow") else {
            return;
        };

        // 1. Promote to NSPanel subclass so NonactivatingPanel works.
        promote_to_panel(ns_window);

        // 2. Set collection behavior for all Spaces + fullscreen auxiliary.
        let current_behavior = ns_window.collectionBehavior();
        let updated_behavior = overlay_behavior(current_behavior);
        ns_window.setCollectionBehavior(updated_behavior);

        // 3. Set window level high enough to float above everything.
        ns_window.setLevel(OVERLAY_WINDOW_LEVEL);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
    }
}

/// Refreshes an existing window after a Space switch without activating app focus.
///
/// Re-applies overlay behavior and brings the window to the front if needed.
/// This should run on AppKit main thread.
#[allow(dead_code)]
pub fn refresh_space_follow(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    unsafe {
        let Some(ns_window) = ns_window(window, "refresh_space_follow") else {
            return;
        };

        let current_behavior = ns_window.collectionBehavior();
        let updated_behavior = overlay_behavior(current_behavior);
        if updated_behavior != current_behavior {
            ns_window.setCollectionBehavior(updated_behavior);
        }

        ns_window.setLevel(OVERLAY_WINDOW_LEVEL);

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
