//! MapleSyrup capture framework - entry point

mod errors;
mod frame;
mod frame_source;
mod fps;
mod keyboard;
mod window_search;
mod live_capture;
mod static_image;
mod vision;

use crate::live_capture::LiveCapture;
use crate::static_image::StaticImageCapture;
use crate::errors::CaptureError;
use crate::frame_source::FrameSource;
use crate::frame::Frame;
use crate::fps::FpsCounter;
use env_logger::Env;
use log::{error, info, warn};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), anyhow::Error> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!("MapleSyrup capture framework starting");

    // Determine backend: search for MapleStory window
    let maybe_hwnd = window_search::find_maplestory_window();

    // Shared current frame for saving when F8 pressed
    let current_frame: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));

    // Choose backend
    // The backend is used only on the main thread here; LiveCapture contains raw HWND pointers
    // which are not Send. Use a non-Send trait object to avoid requiring Send for implementations.
    // If MapleStory window is not found, do not use assets — print and exit
    if maybe_hwnd.is_none() {
        println!("cannot find maplestory");
        return Ok(());
    }

    // convert winapi HWND to windows::Win32::Foundation::HWND
    let hwnd = maybe_hwnd.unwrap();
    let hwnd_win = windows::Win32::Foundation::HWND(hwnd as isize);
    // Get window title for display
    let title = window_search::get_window_title(hwnd).unwrap_or_else(|| "(unknown)".to_string());
    info!("MapleStory window found (hwnd={:?}), trying LiveCapture", hwnd_win);
    let lc = match LiveCapture::new(hwnd_win) {
        Ok(lc) => lc,
        Err(e) => {
            eprintln!("LiveCapture failed: {}", e);
            return Ok(());
        }
    };
    let mut boxed_src: Box<dyn FrameSource> = Box::new(lc);
    let captured_window_name = Some(title);

    // Print the selected source name
    if let Some(name) = &captured_window_name {
        println!("Capturing source: {}", name);
    }

    // Short-run mode: capture for 5 seconds, then save and show last frame
    let run_deadline = Instant::now() + Duration::from_secs(5);

    // FPS counter
    let fps = Arc::new(Mutex::new(FpsCounter::new(60)));

    // Clone shared state
    let fps_thread = fps.clone();

    // Start keyboard thread for F8 (hotkey_watch_loop spawns its own thread) - still OK to run but not needed for this short run
    let frame_for_key = current_frame.clone();
    keyboard::hotkey_watch_loop(frame_for_key);

    // Capture loop - run until deadline
    loop {
        if Instant::now() >= run_deadline {
            break;
        }
        let started = Instant::now();
        match boxed_src.next_frame() {
            Ok(frame) => {
                // update shared current frame
                {
                    let mut guard = current_frame.lock().unwrap();
                    *guard = Some(frame.clone());
                }
                // update fps
                {
                    let mut f = fps_thread.lock().unwrap();
                    f.frame();
                }

                // Estimate HP percentage from the frame (best-effort) and print
                match crate::vision::estimate_hp_percentage(&frame) {
                    Some(pct) => println!("HP: {:.1}%", pct),
                    None => println!("HP: unknown"),
                }

                // Print periodic info
                if started.elapsed() > Duration::from_secs(0) {
                    let f = fps.lock().unwrap();
                    let now_fps = f.current_fps();
                    info!("frame captured w={} h={} fps={:.2}", frame.width, frame.height, now_fps);
                }

                // Sleep a short amount to avoid busy loop (allow ~60fps)
                let elapsed = started.elapsed();
                let target = Duration::from_millis(16);
                if elapsed < target {
                    thread::sleep(target - elapsed);
                }
            }
            Err(err) => match err {
                CaptureError::NoTestSource => {
                    // Print once per second
                    println!("NO TEST SOURCE FOUND");
                    thread::sleep(Duration::from_secs(1));
                }
                other => {
                    error!("Capture error: {}", other);
                    thread::sleep(Duration::from_millis(100));
                }
            },
        }
    }

    // After deadline - save last frame if available and open it
    if let Some(last_frame) = current_frame.lock().unwrap().as_ref() {
        // ensure captures dir
        let _ = std::fs::create_dir_all("captures");
        let out_path = std::path::Path::new("captures/last.png");
        match last_frame.save_png(out_path) {
            Ok(()) => {
                println!("Saved last frame to {}", out_path.display());
                // Try to open with default image viewer (cmd /C start "" path)
                let path_str = out_path.canonicalize().unwrap_or_else(|_| out_path.to_path_buf()).to_string_lossy().to_string();
                let _ = std::process::Command::new("cmd").args(&["/C", "start", "", &path_str]).spawn();
            }
            Err(e) => {
                error!("Failed to save last frame: {}", e);
            }
        }
    } else {
        println!("No frame captured in the run period.");
    }

    Ok(())
}
