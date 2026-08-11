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
        title: String,
    }

    struct WindowListState {
        titles: Vec<String>,
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

        let title = OsString::from_wide(&buffer[..written as usize])
            .to_string_lossy()
            .into_owned();
        let matches_query = title.to_ascii_lowercase().contains(&state.query);
        eprintln!(
            "[window-search] candidate {:?}; matches {:?}: {}",
            title, state.query, matches_query
        );
        if matches_query {
            state.found = hwnd;
            state.title = title;
            return BOOL(0);
        }

        BOOL(1)
    }

    unsafe extern "system" fn list_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam.0 as *mut WindowListState) };
        if unsafe { !IsWindowVisible(hwnd).as_bool() } {
            return BOOL(1);
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return BOOL(1);
        }
        let mut buffer = vec![0u16; (len + 1) as usize];
        let written = unsafe { GetWindowTextW(hwnd, &mut buffer) };
        if written > 0 {
            state.titles.push(
                OsString::from_wide(&buffer[..written as usize])
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        BOOL(1)
    }

    fn visible_window_titles() -> Vec<String> {
        let mut state = WindowListState { titles: Vec::new() };
        unsafe {
            let _ = EnumWindows(
                Some(list_windows_proc),
                LPARAM(&mut state as *mut _ as isize),
            );
        }
        state.titles
    }

    pub fn capture_window_by_title(search_title: &str) -> Option<RgbaImage> {
        capture_window_by_title_info(search_title).map(|(_, image)| image)
    }

    pub fn capture_window_by_title_info(search_title: &str) -> Option<(String, RgbaImage)> {
        let query = search_title.to_lowercase();
        let mut state = WindowSearchState {
            query,
            found: HWND(ptr::null_mut()),
            title: String::new(),
        };

        unsafe {
            let enumeration = EnumWindows(
                Some(enum_windows_proc),
                LPARAM(&mut state as *mut _ as isize),
            );
            if enumeration.is_err() && state.found.0.is_null() {
                eprintln!("[window-search] EnumWindows failed");
                return None;
            }
            if state.found.0.is_null() {
                return None;
            }

            let hwnd = state.found;
            let mut rect = RECT::default();
            if GetClientRect(hwnd, &mut rect).is_err() {
                eprintln!("[window-search] GetClientRect failed for {:?}", state.title);
                return None;
            }

            let mut origin = POINT::default();
            if !windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin).as_bool() {
                eprintln!(
                    "[window-search] ClientToScreen failed for {:?}",
                    state.title
                );
                return None;
            }

            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width <= 0 || height <= 0 {
                eprintln!(
                    "[window-search] rejected {:?}: invalid client size {}x{}",
                    state.title, width, height
                );
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
                eprintln!("[window-search] BitBlt failed for {:?}", state.title);
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
            bmi.bmiHeader.biCompression = BI_RGB.0;

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
                eprintln!("[window-search] GetDIBits failed for {:?}", state.title);
                return None;
            }

            for chunk in buffer.chunks_exact_mut(4) {
                chunk.swap(0, 2);
                chunk[3] = 255;
            }

            RgbaImage::from_raw(width as u32, height as u32, buffer)
                .map(|image| (state.title.clone(), image))
        }
    }

    /// Capture the first visible game client window using common MapleStory
    /// title substring without requiring an exact title.
    pub fn capture_game_window_info() -> Option<(String, RgbaImage)> {
        const GAME_TITLE_SUBSTRING: &str = "maplestory";
        eprintln!(
            "[window-search] searching visible titles for case-insensitive substring: {GAME_TITLE_SUBSTRING:?}"
        );
        if let Some(capture) = capture_window_by_title_info(GAME_TITLE_SUBSTRING) {
            eprintln!(
                "[window-search] selected window {:?} ({}x{})",
                capture.0,
                capture.1.width(),
                capture.1.height()
            );
            return Some(capture);
        }
        let candidates = visible_window_titles();
        eprintln!(
            "[window-search] no matching window found; visible titled windows: {}",
            if candidates.is_empty() {
                "<none>".to_string()
            } else {
                candidates.join(" | ")
            }
        );
        None
    }
}

#[cfg(target_os = "windows")]
pub use windows_capture::{
    capture_game_window_info, capture_window_by_title, capture_window_by_title_info,
};

#[cfg(not(target_os = "windows"))]
pub fn capture_window_by_title(_: &str) -> Option<RgbaImage> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn capture_game_window_info() -> Option<(String, RgbaImage)> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window_by_title_info(_: &str) -> Option<(String, RgbaImage)> {
    None
}
