//! The panel's window, laid over the game: see-through, every click passing
//! through to the game, never taking the focus, always on top — and shown
//! only while the game is the window in front, so it does not float over
//! other programs. This is the window syrup's Minesweeper coach draws its
//! marks in.
//!
//! By default it also keeps out of screen captures, so the vision engine
//! never reads the panel back when it has to copy the screen; with
//! `on_stream` it shows up in OBS and screenshots instead.

#[cfg(not(windows))]
use crate::app::panel::Panel;

/// Where the game's drawing area is on the desktop, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameArea {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// Whether the game is the window in front.
    pub foreground: bool,
    /// The game's process, whose sound is turned down while MapleSyrup talks.
    pub pid: u32,
}

#[cfg(windows)]
pub use win::{Overlay, game_area};

#[cfg(not(windows))]
pub struct Overlay;

#[cfg(not(windows))]
impl Overlay {
    pub fn new(_on_stream: bool) -> Result<Overlay, String> {
        Err("the on-screen panel is only on Windows".into())
    }
    pub fn pump(&self) {}
    pub fn show(&mut self, _panel: &Panel, _at: (i32, i32)) -> Result<(), String> {
        Ok(())
    }
    pub fn hide(&mut self) {}
}

#[cfg(not(windows))]
pub fn game_area(_title: &str) -> Option<GameArea> {
    None
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    use windows::Win32::Foundation::{
        COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
        BLENDFUNCTION, CLIP_DEFAULT_PRECIS, ClientToScreen, CreateCompatibleDC, CreateDIBSection,
        CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
        DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_NORMAL,
        FW_SEMIBOLD, GdiFlush, GetDC, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, SetBkMode,
        SetTextColor, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetClientRect,
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, MSG, PM_REMOVE, PeekMessageW,
        RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SetWindowDisplayAffinity, ShowWindow,
        TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WNDCLASSEXW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_POPUP,
    };
    use windows::core::{HSTRING, PCWSTR, w};

    use super::GameArea;
    use crate::app::panel::Panel;

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// The game window's drawing area on the desktop, when it is open and
    /// not minimised. `title` is the window's exact title.
    pub fn game_area(title: &str) -> Option<GameArea> {
        unsafe {
            let hwnd = FindWindowW(PCWSTR::null(), &HSTRING::from(title)).ok()?;
            if hwnd.is_invalid() || IsIconic(hwnd).as_bool() {
                return None;
            }
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect).ok()?;
            let mut origin = POINT::default();
            if !ClientToScreen(hwnd, &mut origin).as_bool() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            Some(GameArea {
                left: origin.x,
                top: origin.y,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                foreground: GetForegroundWindow() == hwnd,
                pid,
            })
        }
    }

    pub struct Overlay {
        hwnd: HWND,
        visible: bool,
        shown: Option<(Panel, (i32, i32))>,
    }

    impl Overlay {
        pub fn new(on_stream: bool) -> Result<Overlay, String> {
            unsafe {
                let instance: HINSTANCE = GetModuleHandleW(PCWSTR::null())
                    .map_err(|e| e.to_string())?
                    .into();
                let class = w!("MapleSyrupPanel");
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: class,
                    ..Default::default()
                };
                RegisterClassExW(&wc);
                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    class,
                    w!("MapleSyrup panel"),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    Some(instance),
                    None,
                )
                .map_err(|e| e.to_string())?;
                if !on_stream {
                    let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
                }
                Ok(Overlay {
                    hwnd,
                    visible: false,
                    shown: None,
                })
            }
        }

        /// Let the window handle what the system sends it.
        pub fn pump(&self) {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    let _ = DispatchMessageW(&msg);
                }
            }
        }

        pub fn hide(&mut self) {
            if self.visible {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
                self.visible = false;
            }
        }

        /// Show `panel` with its top left at `at` on the desktop.
        pub fn show(&mut self, panel: &Panel, at: (i32, i32)) -> Result<(), String> {
            if self.visible
                && self
                    .shown
                    .as_ref()
                    .is_some_and(|(p, a)| p == panel && *a == at)
            {
                return Ok(());
            }
            let (w, h) = (panel.image.width() as i32, panel.image.height() as i32);
            if w <= 0 || h <= 0 {
                return Ok(());
            }
            unsafe {
                let screen = GetDC(None);
                let mem = CreateCompatibleDC(Some(screen));
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits: *mut c_void = std::ptr::null_mut();
                let dib =
                    match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                        Ok(d) if !bits.is_null() => d,
                        Ok(d) => {
                            let _ = DeleteObject(d.into());
                            let _ = DeleteDC(mem);
                            let _ = ReleaseDC(None, screen);
                            return Err("could not make the panel's bitmap".into());
                        }
                        Err(e) => {
                            let _ = DeleteDC(mem);
                            let _ = ReleaseDC(None, screen);
                            return Err(e.to_string());
                        }
                    };
                let old = SelectObject(mem, dib.into());
                let buf = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
                for (i, p) in panel.image.pixels().enumerate() {
                    buf[4 * i..4 * i + 4].copy_from_slice(&[p.0[2], p.0[1], p.0[0], p.0[3]]);
                }
                for text in &panel.texts {
                    write(mem, text);
                }
                let _ = GdiFlush();
                // GDI leaves what it wrote with alpha 0: make the words opaque.
                for (i, p) in panel.image.pixels().enumerate() {
                    let px = &mut buf[4 * i..4 * i + 4];
                    if p.0[3] == 0 {
                        px.copy_from_slice(&[0, 0, 0, 0]);
                    } else if px[..3] != [p.0[2], p.0[1], p.0[0]] {
                        px[3] = 255;
                    }
                }
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let to = POINT { x: at.0, y: at.1 };
                let size = SIZE { cx: w, cy: h };
                let from = POINT { x: 0, y: 0 };
                let done = UpdateLayeredWindow(
                    self.hwnd,
                    Some(screen),
                    Some(&to as *const _),
                    Some(&size as *const _),
                    Some(mem),
                    Some(&from as *const _),
                    COLORREF(0),
                    Some(&blend as *const _),
                    ULW_ALPHA,
                );
                let _ = SelectObject(mem, old);
                let _ = DeleteObject(dib.into());
                let _ = DeleteDC(mem);
                let _ = ReleaseDC(None, screen);
                done.map_err(|e| e.to_string())?;
                if !self.visible {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    self.visible = true;
                }
            }
            self.shown = Some((panel.clone(), at));
            Ok(())
        }
    }

    fn write(dc: windows::Win32::Graphics::Gdi::HDC, text: &crate::app::panel::Text) {
        if text.text.is_empty() {
            return;
        }
        unsafe {
            let font = CreateFontW(
                -text.size,
                0,
                0,
                0,
                if text.bold {
                    FW_SEMIBOLD.0 as i32
                } else {
                    FW_NORMAL.0 as i32
                },
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                0,
                w!("Segoe UI"),
            );
            let old = SelectObject(dc, font.into());
            let _ = SetBkMode(dc, TRANSPARENT);
            let (r, g, b) = text.color;
            let _ = SetTextColor(
                dc,
                COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16)),
            );
            let mut wide: Vec<u16> = text.text.encode_utf16().collect();
            let (left, top, right, bottom) = text.rect;
            let mut rect = RECT {
                left,
                top,
                right,
                bottom,
            };
            let align = if text.right { DT_RIGHT } else { DT_LEFT };
            let _ = DrawTextW(
                dc,
                &mut wide,
                &mut rect,
                align | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS,
            );
            let _ = SelectObject(dc, old);
            let _ = DeleteObject(font.into());
        }
    }
}
