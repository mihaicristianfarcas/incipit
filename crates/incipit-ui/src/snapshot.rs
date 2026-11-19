//! The window as a picture, for the checks that look at a page without clicking (`INCIPIT_SHOT`,
//! docs/development.md). On a Mac, `screencapture` of this window alone; elsewhere unsupported.

use gpui::Window;

/// Save the window as a PNG (automated checks). On a Mac, through
/// `screencapture` of this window alone; elsewhere unsupported.
pub fn snapshot(window: &Window, path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return Err("not an AppKit window".into());
        };
        // SAFETY: `ns_view` is the live NSView of the window GPUI handed us, on the main thread;
        // `window` and `windowNumber` are NSView's and NSWindow's own methods, and a null window
        // is checked before it is messaged.
        let number: isize = unsafe {
            let view = h.ns_view.as_ptr() as *mut objc2::runtime::AnyObject;
            let win: *mut objc2::runtime::AnyObject = objc2::msg_send![view, window];
            if win.is_null() {
                return Err("the view has no window".into());
            }
            objc2::msg_send![win, windowNumber]
        };
        let out = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x", "-o", "-l", &number.to_string()])
            .arg(path)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() && path.exists() {
            Ok(())
        } else {
            Err(format!(
                "screencapture failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, path);
        Err("snapshots are taken on a Mac only".into())
    }
}
