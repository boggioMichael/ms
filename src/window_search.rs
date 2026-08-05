use log::info;
use std::ptr::null_mut;

/// Find a MapleStory window by title (case-insensitive) and return its HWND if found.
pub fn find_maplestory_window() -> Option<winapi::shared::windef::HWND> {
    unsafe extern "system" fn enum_windows_proc(hwnd: winapi::shared::windef::HWND, lparam: winapi::shared::minwindef::LPARAM) -> winapi::shared::minwindef::BOOL {
        let buf_len = 512;
        // Check window text/title
        let mut title_buf: Vec<u16> = vec![0u16; buf_len];
        let title_len = winapi::um::winuser::GetWindowTextW(hwnd, title_buf.as_mut_ptr(), buf_len as i32);
        if title_len > 0 {
            let s = String::from_utf16_lossy(&title_buf[..title_len as usize]);
            if s.to_lowercase().contains("maplestory") {
                let out = lparam as *mut winapi::shared::windef::HWND;
                if !out.is_null() {
                    *out = hwnd;
                    return 0; // stop enumeration
                }
            }
        }
        // Also check the window class name (some games put name in class)
        let mut class_buf: Vec<u16> = vec![0u16; 256];
        let class_len = winapi::um::winuser::GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
        if class_len > 0 {
            let cls = String::from_utf16_lossy(&class_buf[..class_len as usize]);
            if cls.to_lowercase().contains("maplestory") || cls.to_lowercase().contains("maple") {
                let out = lparam as *mut winapi::shared::windef::HWND;
                if !out.is_null() {
                    *out = hwnd;
                    return 0;
                }
            }
        }
        1 // continue
    }

    unsafe {
        let mut found: winapi::shared::windef::HWND = std::ptr::null_mut();
        let p = &mut found as *mut _ as winapi::shared::minwindef::LPARAM;
        winapi::um::winuser::EnumWindows(Some(enum_windows_proc), p);
        if !found.is_null() {
            info!("Found MapleStory window: {:?}", found);
            Some(found)
        } else {
            None
        }
    }
}

/// Return the window title for a given HWND, if available.
pub fn get_window_title(hwnd: winapi::shared::windef::HWND) -> Option<String> {
    unsafe {
        let len = winapi::um::winuser::GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf: Vec<u16> = vec![0u16; (len + 1) as usize];
        let read = winapi::um::winuser::GetWindowTextW(hwnd, buf.as_mut_ptr(), (len + 1) as i32);
        if read > 0 {
            Some(String::from_utf16_lossy(&buf[..read as usize]))
        } else {
            None
        }
    }
}
