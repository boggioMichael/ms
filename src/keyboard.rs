use crate::frame::Frame;
use log::info;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Watch for F8 key presses using GetAsyncKeyState and save the current frame when pressed.
pub fn hotkey_watch_loop(shared_frame: Arc<Mutex<Option<Frame>>>) {
    thread::spawn(move || loop {
        // VK_F8 is 0x77
        // GetAsyncKeyState returns a SHORT; cast to u16 for bitmask checks
        let state = unsafe { winapi::um::winuser::GetAsyncKeyState(0x77) } as u16;
        if (state & 0x8000u16) != 0u16 {
            let maybe = { shared_frame.lock().unwrap().clone() };
            if let Some(frame) = maybe {
                let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let filename = format!("captures/frame-{}.png", ts);
                if let Err(e) = std::fs::create_dir_all("captures") {
                    log::warn!("Failed to create captures dir: {}", e);
                }
                match frame.save_png(&filename) {
                    Ok(_) => info!("Saved frame to {}", filename),
                    Err(e) => log::error!("Failed saving frame: {}", e),
                }
                // simple debounce
                thread::sleep(Duration::from_millis(400));
            }
        }
        thread::sleep(Duration::from_millis(16));
    });
}
