//! The window as a picture, for the checks that look at a page without clicking (`INCIPIT_SHOT`,
//! docs/development.md), so a change to what is on screen can be looked at before and after on
//! each system.
//!
//! - macOS: `screencapture` of this window alone, which macOS composes whatever covers it.
//! - Windows: the window's part of the screen, as DWM composed it. The glass (the system's acrylic)
//!   is drawn by DWM behind the window, not by the program, so asking the window to draw itself
//!   (`PrintWindow`) would leave it out. The window is first put above the others without taking
//!   the keyboard (a run with `INCIPIT_SHOT` opens without focus), and the copy waits for DWM's
//!   next composed frame (`DwmFlush`), so the screen shows it raised. Its bounds are DWM's
//!   (`DWMWA_EXTENDED_FRAME_BOUNDS`: without the invisible resize border, in physical pixels), and
//!   the screen is read with this thread per-monitor DPI aware, in the same pixels. A locked or
//!   sleeping screen gives a black picture.
//! - Linux under X11: the window's pixels from the X server (`GetImage`), through a connection of
//!   its own. Under Wayland a program cannot read the screen unless the person agrees each time
//!   (the desktop portal asks), so there it is refused with what to do instead.
//!
//! Windows and X11 both hand over 32-bit pixels in the order blue, green, red, then a byte that is
//! padding or premultiplied alpha. The picture is made opaque: a person sees the window over
//! whatever is behind it, never through to nothing.

use gpui::Window;

/// Save the window as a PNG (automated checks).
pub fn snapshot(window: &Window, path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        mac(window, path)
    }
    #[cfg(any(windows, target_os = "linux"))]
    {
        let (width, height, bgrx) = pixels(window)?;
        let image = image::RgbaImage::from_raw(width, height, opaque_rgba(&bgrx))
            .ok_or("the window's pixels did not fill its size")?;
        image
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (window, path);
        Err("snapshots are taken on macOS, Windows and Linux only".into())
    }
}

/// Blue, green, red and a fourth byte per pixel, as Windows' and X11's 32-bit pixels are, made into
/// opaque red, green, blue, alpha.
#[cfg(any(windows, target_os = "linux", test))]
fn opaque_rgba(bgrx: &[u8]) -> Vec<u8> {
    let (pixels, _) = bgrx.as_chunks::<4>();
    pixels
        .iter()
        .flat_map(|&[b, g, r, _]| [r, g, b, u8::MAX])
        .collect()
}

#[cfg(target_os = "macos")]
fn mac(window: &Window, path: &std::path::Path) -> Result<(), String> {
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

/// The window's width, height and pixels as the screen shows them (see the module's words).
#[cfg(windows)]
fn pixels(window: &Window) -> Result<(u32, u32, Vec<u8>), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmFlush, DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return Err("not a Win32 window".into());
    };
    let hwnd = HWND(h.hwnd.get() as *mut c_void);
    let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
    let mut bounds = RECT::default();
    // SAFETY: `hwnd` is the live window GPUI handed us, used on the thread that owns it, and
    // `bounds` is a RECT of the size DwmGetWindowAttribute is told it has. Topmost and back puts
    // the window above every ordinary window (HWND_NOTOPMOST alone does nothing to a window that
    // is not topmost) without changing what has the keyboard.
    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags).map_err(|e| e.message())?;
        SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags).map_err(|e| e.message())?;
        DwmFlush().map_err(|e| e.message())?;
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .map_err(|e| e.message())?;
    }
    // SAFETY: changes only this thread's DPI awareness, and the previous one is put back.
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let copied = screen_part(&bounds);
    // SAFETY: as above.
    unsafe { SetThreadDpiAwarenessContext(previous) };
    copied
}

/// The pixels of the screen inside `bounds`, as DWM last composed it.
#[cfg(windows)]
fn screen_part(bounds: &windows::Win32::Foundation::RECT) -> Result<(u32, u32, Vec<u8>), String> {
    use std::ffi::c_void;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
    };

    let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
    if width <= 0 || height <= 0 {
        return Err("the window has no size on screen".into());
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: rows from the top down, as a PNG has them.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: every handle made here is released before returning, whatever happens. The DIB
    // section's memory is width × height 32-bit pixels, top-down, as `info` asks, and it is read
    // only after BitBlt has filled it and before the bitmap is deleted.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits: *mut c_void = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        {
            Err(e) => Err(e.message()),
            Ok(bitmap) => {
                let previous = SelectObject(memory, bitmap.into());
                let copied = BitBlt(
                    memory,
                    0,
                    0,
                    width,
                    height,
                    Some(screen),
                    bounds.left,
                    bounds.top,
                    SRCCOPY | CAPTUREBLT,
                )
                .map_err(|e| e.message())
                .map(|()| {
                    let len = width as usize * height as usize * 4;
                    std::slice::from_raw_parts(bits as *const u8, len).to_vec()
                });
                SelectObject(memory, previous);
                let _ = DeleteObject(bitmap.into());
                copied
            }
        };
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result.map(|pixels| (width as u32, height as u32, pixels))
    }
}

/// The window's width, height and pixels from the X server (see the module's words).
#[cfg(target_os = "linux")]
fn pixels(window: &Window) -> Result<(u32, u32, Vec<u8>), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let id = match handle.as_raw() {
        RawWindowHandle::Xcb(h) => h.window.get(),
        RawWindowHandle::Xlib(h) => h.window as u32,
        RawWindowHandle::Wayland(_) => {
            return Err(
                "under Wayland the app cannot take its own picture; run it under X11 \
                        (WAYLAND_DISPLAY= with DISPLAY set) or on a virtual screen (Xvfb)"
                    .into(),
            )
        }
        _ => return Err("not an X11 window".into()),
    };
    let failed = |e: &dyn std::fmt::Display| format!("the X server: {e}");
    let (x, _) = x11rb::connect(None).map_err(|e| failed(&e))?;
    let geometry = x
        .get_geometry(id)
        .map_err(|e| failed(&e))?
        .reply()
        .map_err(|e| failed(&e))?;
    let visual = x
        .get_window_attributes(id)
        .map_err(|e| failed(&e))?
        .reply()
        .map_err(|e| failed(&e))?
        .visual;
    let setup = x.setup();
    let bits = setup
        .pixmap_formats
        .iter()
        .find(|f| f.depth == geometry.depth)
        .map(|f| f.bits_per_pixel);
    let masks = setup
        .roots
        .iter()
        .flat_map(|screen| &screen.allowed_depths)
        .flat_map(|depth| &depth.visuals)
        .find(|v| v.visual_id == visual)
        .map(|v| (v.red_mask, v.green_mask, v.blue_mask));
    if setup.image_byte_order != ImageOrder::LSB_FIRST
        || bits != Some(32)
        || masks != Some((0xff_0000, 0xff00, 0xff))
    {
        return Err(format!(
            "the X server's pixels are not 32-bit blue, green, red (depth {}, {bits:?} bits)",
            geometry.depth
        ));
    }
    let image = x
        .get_image(
            ImageFormat::Z_PIXMAP,
            id,
            0,
            0,
            geometry.width,
            geometry.height,
            u32::MAX,
        )
        .map_err(|e| failed(&e))?
        .reply()
        .map_err(|e| failed(&e))?;
    Ok((
        u32::from(geometry.width),
        u32::from(geometry.height),
        image.data,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blue_green_red_pixel_becomes_an_opaque_red_green_blue_one() {
        let bgrx = [10, 20, 30, 0, 1, 2, 3, 128];
        assert_eq!(opaque_rgba(&bgrx), [30, 20, 10, 255, 3, 2, 1, 255]);
    }

    #[test]
    fn a_short_last_pixel_is_left_out() {
        assert_eq!(opaque_rgba(&[1, 2, 3, 4, 5, 6]), [3, 2, 1, 255]);
    }
}
