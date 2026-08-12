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

    /// Windows whose titles can legitimately contain "maplestory" without
    /// being the actual game client, e.g. a media player playing a recorded
    /// clip or a browser tab with the word in its title.
    const NON_GAME_TITLE_MARKERS: &[&str] = &[
        ".mp4",
        ".mkv",
        ".avi",
        ".mov",
        ".wmv",
        ".webm",
        ".gif",
        ".flv",
        "vlc",
        "media player",
        "potplayer",
        "mpc-",
        "mpc-hc",
        "quicktime",
        "youtube",
        "chrome",
        "firefox",
        "edge",
        "obs ",
    ];

    fn last_status() -> &'static std::sync::Mutex<Option<String>> {
        static LAST: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
            std::sync::OnceLock::new();
        LAST.get_or_init(|| std::sync::Mutex::new(None))
    }

    const DIM: &str = "\u{1b}[2m";
    const BOLD: &str = "\u{1b}[1m";
    const CYAN: &str = "\u{1b}[36m";
    const GREEN: &str = "\u{1b}[32m";
    const YELLOW: &str = "\u{1b}[33m";
    const RESET: &str = "\u{1b}[0m";

    fn truncate_title(title: &str, max: usize) -> String {
        let chars: Vec<char> = title.chars().collect();
        if chars.len() <= max {
            return title.to_string();
        }
        format!(
            "{}…",
            chars[..max.saturating_sub(1)].iter().collect::<String>()
        )
    }

    /// Redraws the search line in place, so scanning reads as one animated
    /// line rather than a new line per candidate per frame.
    fn draw_search_line(line: &str) {
        eprint!("\r\u{1b}[2K{line}");
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }

    /// Animates the candidate scan, then leaves a single settled line on
    /// screen. Only replays when the outcome changes, so a polling loop
    /// stays quiet once it has locked on.
    fn animate_scan(candidates: &[(String, String)], chosen: Option<&str>) {
        const FRAMES: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];

        for (index, (title, _)) in candidates.iter().enumerate() {
            let spinner = FRAMES[index % FRAMES.len()];
            let is_match = chosen == Some(title.as_str());
            let verdict = if is_match {
                format!("{GREEN}{BOLD}MATCH{RESET}")
            } else {
                format!("{DIM}no{RESET}")
            };
            draw_search_line(&format!(
                "{CYAN}{spinner}{RESET} scanning windows {DIM}…{RESET} [{}{DIM} · {RESET}{verdict}]",
                truncate_title(title, 44)
            ));
            std::thread::sleep(std::time::Duration::from_millis(70));
            if is_match {
                break;
            }
        }

        match chosen {
            Some(title) => draw_search_line(&format!(
                "{GREEN}✓{RESET} game window {BOLD}{}{RESET}\n",
                truncate_title(title, 52)
            )),
            None => draw_search_line(&format!(
                "{YELLOW}○{RESET} no MapleStory window found {DIM}(waiting…){RESET}\n"
            )),
        }
    }

    /// Capture the visible game client window, preferring an exact title
    /// match ("MapleStory") and otherwise a substring match that isn't a
    /// known non-game window (video players, browsers, etc.).
    pub fn capture_game_window_info() -> Option<(String, RgbaImage)> {
        const GAME_TITLE_SUBSTRING: &str = "maplestory";

        let candidates = visible_window_titles();
        let lowered: Vec<(String, String)> = candidates
            .iter()
            .map(|title| (title.clone(), title.to_ascii_lowercase()))
            .collect();

        let exact = lowered
            .iter()
            .find(|(_, lower)| lower == GAME_TITLE_SUBSTRING);

        let best = exact.or_else(|| {
            lowered.iter().find(|(_, lower)| {
                lower.contains(GAME_TITLE_SUBSTRING)
                    && !NON_GAME_TITLE_MARKERS
                        .iter()
                        .any(|marker| lower.contains(marker))
            })
        });

        {
            let outcome = best
                .map(|(title, _)| title.clone())
                .unwrap_or_else(|| "<none>".to_string());
            let mut last = last_status().lock().unwrap();
            if last.as_deref() != Some(outcome.as_str()) {
                animate_scan(&lowered, best.map(|(title, _)| title.as_str()));
                *last = Some(outcome);
            }
        }

        let (title, _) = best?;
        capture_window_by_title_info(title)
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
