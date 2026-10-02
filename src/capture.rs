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
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    /// PW_CLIENTONLY | PW_RENDERFULLCONTENT: render just the client area,
    /// and include content drawn outside the classic GDI path.
    const PW_CLIENTONLY_FULL: u32 = 0x0000_0001 | 0x0000_0002;
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

    /// Every visible titled window, so a caller can offer a choice instead
    /// of relying on the title heuristic.
    ///
    /// Title matching cannot cover every case: a private server, a custom
    /// client, or a test window will not be called "MapleStory", and there
    /// is no way to guess which window a user means. Listing them lets the
    /// user say.
    pub fn list_windows() -> Vec<String> {
        visible_window_titles()
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

            // Ask the window to draw itself, rather than copying the screen
            // region it occupies. Copying the screen returns whatever is
            // visually on top: with the game behind another window, the
            // "capture" is of that other window, and the vision pipeline
            // then analyses the wrong pixels entirely. PrintWindow reads the
            // window's own surface, so it works while occluded.
            let printed = PrintWindow(hwnd, hdc_mem, PRINT_WINDOW_FLAGS(PW_CLIENTONLY_FULL));

            if !printed.as_bool() {
                // Some windows (hardware-accelerated or protected content)
                // refuse PrintWindow; fall back to the screen copy, which
                // still works as long as the window is unobstructed.
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
                    // The screen DC comes from GetDC and must be released on every
                    // path out of this block, not just the success path, or each
                    // failed capture burns one of the process' 10k GDI handles.
                    let _ = ReleaseDC(None, hdc_screen);
                    return None;
                }
            }

            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = width;
            bmi.bmiHeader.biHeight = -height;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB.0;

            let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
            let mut result = GetDIBits(
                hdc_mem,
                hbitmap,
                0,
                height as u32,
                Some(buffer.as_mut_ptr() as *mut _),
                &mut bmi,
                windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
            );

            // PrintWindow can report success yet return an empty surface for
            // content the GPU draws (DirectX game clients, video overlays):
            // the frame comes back a flat colour. That is indistinguishable
            // from a real capture to everything downstream, which would then
            // analyse a blank image and report "nothing detected" forever, so
            // check for it and retry via the screen instead.
            if printed.as_bool()
                && result != 0
                && is_blank(&buffer)
                && BitBlt(
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
                .is_ok()
            {
                result = GetDIBits(
                    hdc_mem,
                    hbitmap,
                    0,
                    height as u32,
                    Some(buffer.as_mut_ptr() as *mut _),
                    &mut bmi,
                    windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
                );
            }

            let _ = SelectObject(hdc_mem, old_obj);
            let _ = DeleteObject(hbitmap.into());
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(None, hdc_screen);

            if result == 0 {
                eprintln!("[window-search] GetDIBits failed for {:?}", state.title);
                return None;
            }

            for chunk in buffer.as_chunks_mut::<4>().0 {
                chunk.swap(0, 2);
                chunk[3] = 255;
            }

            RgbaImage::from_raw(width as u32, height as u32, buffer)
                .map(|image| (state.title.clone(), image))
        }
    }

    /// Is this captured surface effectively featureless?
    ///
    /// Used to spot a PrintWindow call that "succeeded" but returned nothing
    /// drawable. Sampling rather than scanning every pixel keeps this off
    /// the per-frame cost; a real game frame varies within any few hundred
    /// samples, so a uniform sample means a uniform image.
    fn is_blank(buffer: &[u8]) -> bool {
        const SAMPLES: usize = 512;
        let pixels = buffer.len() / 4;
        if pixels == 0 {
            return true;
        }
        let stride = (pixels / SAMPLES).max(1);
        let first = &buffer[0..3];
        !(0..pixels)
            .step_by(stride)
            .any(|index| buffer[index * 4..index * 4 + 3] != *first)
    }

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
        let candidates = visible_window_titles();
        let lowered: Vec<(String, String)> = candidates
            .iter()
            .map(|title| (title.clone(), title.to_ascii_lowercase()))
            .collect();

        let chosen = super::pick_game_window(&candidates);
        let best = chosen
            .as_ref()
            .and_then(|title| lowered.iter().find(|(t, _)| t == title));

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
    capture_game_window_info, capture_window_by_title, capture_window_by_title_info, list_windows,
};

#[cfg(not(target_os = "windows"))]
use image::RgbaImage;

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

#[cfg(not(target_os = "windows"))]
pub fn list_windows() -> Vec<String> {
    Vec::new()
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
    "maplesyrup",
];

/// Which of `titles` is the game: one called exactly "MapleStory", or else
/// the first containing it that is not a known non-game window (a video
/// player, a browser, MapleSyrup's own windows).
pub fn pick_game_window(titles: &[String]) -> Option<String> {
    const GAME: &str = "maplestory";
    let lowered: Vec<String> = titles.iter().map(|t| t.to_ascii_lowercase()).collect();
    let exact = lowered.iter().position(|t| t.trim() == GAME);
    let fallback = || {
        lowered
            .iter()
            .position(|t| t.contains(GAME) && !NON_GAME_TITLE_MARKERS.iter().any(|m| t.contains(m)))
    };
    exact.or_else(fallback).map(|i| titles[i].clone())
}

/// What one attempt to capture the game gave.
pub enum Captured {
    /// The game window's title and its pixels.
    Frame {
        title: String,
        image: image::RgbaImage,
    },
    /// No window that looks like the game.
    NotFound,
    /// The window is there but gave no picture, and why.
    Unavailable(String),
}

/// A capture session on the game window, through syrup's capture: the
/// window is found once and kept between frames (syrup's `Window`), and a
/// failed capture says why, so the companion can tell "minimised" from
/// "closed".
pub struct GameCapture {
    /// A title to match instead of looking for MapleStory.
    query: Option<String>,
    window: Option<syrup::capture::Window>,
}

impl GameCapture {
    /// Find the MapleStory client by its title.
    pub fn auto() -> Self {
        Self {
            query: None,
            window: None,
        }
    }

    /// Capture the first window whose title contains `query` instead.
    pub fn titled(query: &str) -> Self {
        Self {
            query: Some(query.to_string()),
            window: None,
        }
    }

    pub fn capture(&mut self) -> Captured {
        if self.window.is_none() {
            let titles = syrup::capture::list_windows();
            let target = match &self.query {
                Some(query) => {
                    let query = query.to_lowercase();
                    titles
                        .iter()
                        .find(|t| t.to_lowercase().contains(&query) && !is_own_window(t))
                        .cloned()
                }
                None => pick_game_window(&titles),
            };
            let Some(target) = target else {
                return Captured::NotFound;
            };
            match syrup::capture::Window::find(&target) {
                // syrup matches by "contains"; make sure it is the one chosen.
                Ok(window) if window.title() == target => self.window = Some(window),
                Ok(window) => {
                    return Captured::Unavailable(format!(
                        "another window, \"{}\", has the game's title in its own; close it or start MapleSyrup with --window",
                        window.title()
                    ));
                }
                Err(syrup::capture::CaptureError::NotFound) => return Captured::NotFound,
                Err(e) => return Captured::Unavailable(e.to_string()),
            }
        }
        let Some(window) = self.window.as_mut() else {
            return Captured::NotFound;
        };
        match window.capture() {
            Ok(image) => Captured::Frame {
                title: window.title().to_string(),
                image,
            },
            Err(syrup::capture::CaptureError::Closed | syrup::capture::CaptureError::NotFound) => {
                self.window = None;
                Captured::NotFound
            }
            Err(e) => {
                let reason = e.to_string();
                if reason.contains("minimised") {
                    Captured::Unavailable("it is minimised".into())
                } else {
                    Captured::Unavailable(reason)
                }
            }
        }
    }
}

/// MapleSyrup's own windows (the preview, the console) mention the game.
fn is_own_window(title: &str) -> bool {
    title.to_ascii_lowercase().contains("maplesyrup")
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn titles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_exact_title_wins_over_lookalikes_above_it() {
        let list = titles(&["MapleStory boss run.mp4 - VLC", "MapleSyrup", "MapleStory"]);
        assert_eq!(pick_game_window(&list).as_deref(), Some("MapleStory"));
    }

    #[test]
    fn a_decorated_title_is_taken_when_nothing_is_exact() {
        let list = titles(&["MapleStory - YouTube - Google Chrome", "MapleStory v.271"]);
        assert_eq!(pick_game_window(&list).as_deref(), Some("MapleStory v.271"));
    }

    #[test]
    fn no_game_no_pick() {
        let list = titles(&["MapleSyrup — Vision Preview", "Notepad", "maplestory.mkv"]);
        assert_eq!(pick_game_window(&list), None);
    }
}
