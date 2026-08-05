//! Window and screen capture helpers for developer debugging.
//!
//! When running on Windows, this module can capture a named window by title
//! substring and return an RGBA image for vision analysis.

#[cfg(target_os = "windows")]
mod windows_capture {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;

    use image::RgbaImage;
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };
    use windows::core::BOOL;

    struct WindowSearchState {
        query: String,
        found: HWND,
    }

    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam.0 as *mut WindowSearchState) };
        if unsafe { !IsWindowVisible(hwnd).as_bool() } {
            return BOOL(1);
        }

        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return BOOL(1);
        }

        let mut buffer = vec![0u16; (len + 1) as usize];
        let written = unsafe { GetWindowTextW(hwnd, &mut buffer) };
        if written <= 0 {
            return BOOL(1);
        }

        let text = OsString::from_wide(&buffer[..written as usize])
            .to_string_lossy()
            .to_lowercase();
        if text.contains(&state.query) {
            state.found = hwnd;
            return BOOL(0);
        }

        BOOL(1)
    }

    pub fn capture_window_by_title(search_title: &str) -> Option<RgbaImage> {
        let query = search_title.to_lowercase();
        let mut state = WindowSearchState {
            query,
            found: HWND(ptr::null_mut()),
        };

        unsafe {
            if EnumWindows(
                Some(enum_windows_proc),
                LPARAM(&mut state as *mut _ as isize),
            )
            .is_err()
            {
                return None;
            }
            if state.found.0.is_null() {
                return None;
            }

            let hwnd = state.found;
            let mut rect = RECT::default();
            if GetClientRect(hwnd, &mut rect).is_err() {
                return None;
            }

            let mut origin = POINT::default();
            if !windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin).as_bool() {
                return None;
            }

            let width = (rect.right - rect.left) as i32;
            let height = (rect.bottom - rect.top) as i32;
            if width <= 0 || height <= 0 {
                return None;
            }

            let hdc_screen = GetDC(None);
            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            let hbitmap = CreateCompatibleBitmap(hdc_screen, width, height);
            let old_obj = SelectObject(hdc_mem, hbitmap.into());
            if BitBlt(
                hdc_mem,
                0,
                0,
                width,
                height,
                Some(hdc_screen),
                origin.x,
                origin.y,
                SRCCOPY,
            )
            .is_err()
            {
                let _ = SelectObject(hdc_mem, old_obj);
                let _ = DeleteObject(hbitmap.into());
                let _ = DeleteDC(hdc_mem);
                return None;
            }

            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = width;
            bmi.bmiHeader.biHeight = -height;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB.0 as u32;

            let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
            let result = GetDIBits(
                hdc_mem,
                hbitmap,
                0,
                height as u32,
                Some(buffer.as_mut_ptr() as *mut _),
                &mut bmi,
                windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
            );

            let _ = SelectObject(hdc_mem, old_obj);
            let _ = DeleteObject(hbitmap.into());
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(None, hdc_screen);

            if result == 0 {
                return None;
            }

            for chunk in buffer.chunks_exact_mut(4) {
                chunk.swap(0, 2);
                chunk[3] = 255;
            }

            RgbaImage::from_raw(width as u32, height as u32, buffer)
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_capture::capture_window_by_title;

#[cfg(not(target_os = "windows"))]
pub fn capture_window_by_title(_: &str) -> Option<RgbaImage> {
    None
}
