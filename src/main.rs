use image::io::Reader as ImageReader;
use ms::capture::capture_game_window_info;
use ms::hud::{annotate_ui_markers, detect_hud_snapshot};
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

fn format_metric(metric: &Option<ms::hud::HudMetric>) -> String {
    match metric {
        Some(metric) => {
            let percent = metric
                .percent
                .map(|p| format!("{p:.1}%"))
                .unwrap_or_else(|| "unknown".into());
            let value = metric
                .value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into());
            format!("{value} / {percent}")
        }
        None => "unknown".into(),
    }
}

fn format_text(value: &Option<String>) -> String {
    value.as_deref().unwrap_or("unknown").to_string()
}

fn print_hud_summary(frame_index: usize, snapshot: &ms::hud::HudSnapshot) {
    let hp = format_metric(&snapshot.hp);
    let mp = format_metric(&snapshot.mp);
    let xp = format_metric(&snapshot.exp);
    let name = format_text(&snapshot.player_name);
    let job = format_text(&snapshot.character_class);
    let level = format_text(&snapshot.level);

    println!(
        "[{frame_index}] HP {hp} | MP {mp} | EXP {xp} | Name {name} | Job {job} | Level {level}"
    );
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

    loop {
        let (source, image) = match load_image() {
            Some(item) => item,
            None => {
                println!("No live MapleStory window capture was available.");
                println!(
                    "Open the game window, or pass a screenshot path explicitly: cargo run -- resources/maplestory.png"
                );
                return;
            }
        };

        if first_frame {
            println!(
                "Inspecting window/source: {} ({}x{}); OCR values and detection rectangles follow.",
                source,
                image.width(),
                image.height()
            );
        }

        let snapshot = detect_hud_snapshot(&image);
        let markers = snapshot.markers.clone();
        println!("===============================");
        print_hud_summary(frame_index, &snapshot);
        println!("===============================");

        std::fs::create_dir_all("out").expect("failed to create output directory");
        write_debug_overlay(&image, &markers, frame_index);
        first_frame = false;

        frame_index += 1;
        io::stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(120));
    }
}
