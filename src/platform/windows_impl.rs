//! Windows: the console and the process (the coach in syrup does the same).

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    CONSOLE_MODE, CONSOLE_SCREEN_BUFFER_INFO, CTRL_BREAK_EVENT, CTRL_C_EVENT,
    ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode,
    GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleCtrlHandler,
    SetConsoleMode, SetConsoleOutputCP, SetConsoleTitleW,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::core::{BOOL, HSTRING};

use super::ConsoleSetup;

unsafe extern "system" fn on_ctrl(kind: u32) -> BOOL {
    if kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT {
        super::request_stop();
        // Handled: the main loop stops and tidies up.
        return BOOL(1);
    }
    // Closing the window, logging off: put the game's volume back if it was
    // turned down for MapleSyrup's voice, then let Windows end the process.
    super::sound::restore();
    BOOL(0)
}

pub fn init(title: &str) -> ConsoleSetup {
    unsafe {
        // Capture pixels as they are on a display scaled above 100%. Fails
        // harmlessly if a manifest already chose an awareness.
        let dpi_aware =
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok();
        let _ = SetConsoleTitleW(&HSTRING::from(title));
        // UTF-8 for anything written as bytes (Rust's own console writes are UTF-16 already).
        let _ = SetConsoleOutputCP(65001);
        let _ = SetConsoleCtrlHandler(Some(on_ctrl), true);
        let ansi = match GetStdHandle(STD_OUTPUT_HANDLE) {
            Ok(handle) if !handle.is_invalid() => enable_vt(handle),
            _ => false,
        };
        ConsoleSetup { ansi, dpi_aware }
    }
}

/// Turn on escape-code processing (Windows 10 and later).
unsafe fn enable_vt(handle: HANDLE) -> bool {
    let mut mode = CONSOLE_MODE::default();
    unsafe {
        if GetConsoleMode(handle, &mut mode).is_err() {
            // Not a console (output redirected to a file or a pipe).
            return false;
        }
        if mode.contains(ENABLE_VIRTUAL_TERMINAL_PROCESSING) {
            return true;
        }
        SetConsoleMode(
            handle,
            mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        )
        .is_ok()
    }
}

/// The console window's size in characters: (columns, rows).
pub fn console_size() -> Option<(usize, usize)> {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
        GetConsoleScreenBufferInfo(handle, &mut info).ok()?;
        let columns = (info.srWindow.Right - info.srWindow.Left + 1).max(0) as usize;
        let rows = (info.srWindow.Bottom - info.srWindow.Top + 1).max(0) as usize;
        (columns > 0 && rows > 0).then_some((columns, rows))
    }
}
