use image::io::Reader as ImageReader;
use ms::capture::capture_game_window_info;
use ms::hud::{annotate_ui_markers, detect_hud_snapshot};
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

fn format_text(value: &Option<String>) -> String {
    value.as_deref().unwrap_or("unknown").to_string()
}

const DIM: &str = "\u{1b}[2m";
const BOLD: &str = "\u{1b}[1m";
const RED: &str = "\u{1b}[31m";
const BLUE: &str = "\u{1b}[34m";
const YELLOW: &str = "\u{1b}[33m";
const RESET: &str = "\u{1b}[0m";

fn bar(percent: Option<f32>, color: &str) -> String {
    const WIDTH: usize = 24;
    match percent {
        Some(p) => {
            let filled = ((p / 100.0) * WIDTH as f32)
                .round()
                .clamp(0.0, WIDTH as f32) as usize;
            format!(
                "{color}{}{RESET}{DIM}{}{RESET} {p:>5.1}%",
                "█".repeat(filled),
                "░".repeat(WIDTH - filled)
            )
        }
        None => format!("{DIM}{} {:>5}{RESET}", "░".repeat(WIDTH), "--"),
    }
}

fn print_hud_summary(frame_index: usize, snapshot: &ms::hud::HudSnapshot) {
    let name = format_text(&snapshot.player_name);
    let job = format_text(&snapshot.character_class);
    let level = format_text(&snapshot.level);

    // Redraw the panel in place so a live capture reads as one updating view.
    if frame_index > 0 {
        print!("\u{1b}[6A");
    }
    println!("\u{1b}[2K{BOLD}╭─ MapleSyrup{RESET} {DIM}· frame {frame_index}{RESET}");
    println!(
        "\u{1b}[2K{DIM}│{RESET} HP  {}",
        bar(snapshot.hp.as_ref().and_then(|m| m.percent), RED)
    );
    println!(
        "\u{1b}[2K{DIM}│{RESET} MP  {}",
        bar(snapshot.mp.as_ref().and_then(|m| m.percent), BLUE)
    );
    println!(
        "\u{1b}[2K{DIM}│{RESET} EXP {}",
        bar(snapshot.exp.as_ref().and_then(|m| m.percent), YELLOW)
    );
    println!(
        "\u{1b}[2K{DIM}│{RESET} {BOLD}{name}{RESET} {DIM}·{RESET} {job} {DIM}·{RESET} Lv {level}"
    );
    println!("\u{1b}[2K{DIM}╰────────────────────────────────────{RESET}");
}

fn write_debug_overlay(image: &image::RgbaImage, markers: &ms::hud::UiMarkers, frame_index: usize) {
    let annotated = annotate_ui_markers(image, markers);
    let frame_path = format!("out/last-frame-{frame_index:06}.png");

    if let Err(err) = annotated.save(&frame_path) {
        eprintln!("warning: failed to write frame overlay {frame_path}: {err}");
        return;
    }

    // Best-effort stable filename for tools/scripts that read a fixed path.
    if let Err(err) = annotated.save("out/last-frame.png") {
        eprintln!("warning: failed to refresh out/last-frame.png: {err}");
    }
}

fn load_image() -> Option<(String, image::RgbaImage)> {
    if let Some((title, image)) = capture_game_window_info() {
        return Some((title, image));
    }

    let path = std::env::args().nth(1)?;
    let path = Path::new(&path);
    if path.exists() {
        let image = ImageReader::open(path).ok()?.decode().ok()?.to_rgba8();
        return Some((path.display().to_string(), image));
    }

    None
}

fn main() {
    let mut frame_index = 0usize;
    let mut first_frame = true;

    let has_fallback_arg = std::env::args().nth(1).is_some();
    let mut waiting_announced = false;

    loop {
        let (source, image) = match load_image() {
            Some(item) => item,
            None => {
                // With no input at all, wait for the game rather than
                // exiting: starting this before launching MapleStory is the
                // normal way to use it.
                if !has_fallback_arg {
                    if !waiting_announced {
                        println!("Waiting for the MapleStory window… {DIM}(ctrl-c to stop){RESET}");
                        println!(
                            "{DIM}Or run against a capture instead:{RESET}\n  \
                             cargo run --release --bin ms -- resources/maplestory.png\n  \
                             cargo run --release --bin vision_debug -- chaos-zakum-solo-lvl230.mp4"
                        );
                        waiting_announced = true;
                    }
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }

                println!("Could not read the image passed on the command line.");
                return;
            }
        };

        if first_frame {
            println!(
                "{DIM}source{RESET} {source} {DIM}({}x{}){RESET}",
                image.width(),
                image.height()
            );
            if !ms::vision::ocr::is_ocr_available() {
                println!(
                    "{YELLOW}!{RESET} Tesseract OCR not found — name, job, level and numeric \
                     values stay unknown. Install it: {BOLD}winget install -e --id UB-Mannheim.TesseractOCR{RESET}"
                );
            }
            println!();
        }

        let snapshot = detect_hud_snapshot(&image);
        let markers = snapshot.markers.clone();
        print_hud_summary(frame_index, &snapshot);

        std::fs::create_dir_all("out").expect("failed to create output directory");
        write_debug_overlay(&image, &markers, frame_index);
        first_frame = false;

        frame_index += 1;
        io::stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(120));
    }
}
