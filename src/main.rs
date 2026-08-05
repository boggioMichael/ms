use image::io::Reader as ImageReader;
use ms::debug::{capture_game_window_info, crop::save_crop};
use ms::hud::{detect_hud_snapshot, save_ui_debug_overlay};
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
            let raw = metric.raw_text.as_deref().unwrap_or("no OCR text");
            format!("{value} / {percent} (OCR: {raw})")
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
        "[frame {frame_index}] HP: {hp} | MP: {mp} | XP: {xp} | Name: {name} | Job: {job} | Level: {level}"
    );
    println!(
        "[frame {frame_index}] rectangles: HP={:?} MP={:?} EXP={:?} Name={:?} Job={:?} Level={:?}",
        snapshot.markers.hp_bar,
        snapshot.markers.mp_bar,
        snapshot.markers.exp_bar,
        snapshot.markers.name_plate,
        snapshot.markers.class_plate,
        snapshot.markers.level_plate,
    );
}

fn load_image() -> Option<(String, image::RgbaImage)> {
    if let Some((title, image)) = capture_game_window_info() {
        return Some((title, image));
    }

    if Path::new("resources/last.png").exists() {
        let image = ImageReader::open("resources/last.png")
            .ok()?
            .decode()
            .ok()?
            .to_rgba8();
        return Some(("resources/last.png".to_string(), image));
    }

    if Path::new("resources/maplestory_hp_frame.png").exists() {
        let image = ImageReader::open("resources/maplestory_hp_frame.png")
            .ok()?
            .decode()
            .ok()?
            .to_rgba8();
        return Some(("resources/maplestory_hp_frame.png".to_string(), image));
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
                println!(
                    "No live Maplestory window capture was available and no fallback screenshot was found."
                );
                println!(
                    "Open the Maplestory Chrome window or place a frame at resources/maplestory_hp_frame.png and rerun."
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

        if first_frame {
            let debug_path = save_ui_debug_overlay("maplestory_ui", &image, &markers, "debug_out")
                .expect("failed to write debug overlay");
            println!("Saved UI debug overlay to {}", debug_path.display());

            let crops = [
                ("hp_bar", markers.hp_bar),
                ("mp_bar", markers.mp_bar),
                ("xp_bar", markers.exp_bar),
                ("name_plate", markers.name_plate),
                ("job_plate", markers.class_plate),
                ("level_plate", markers.level_plate),
            ];

            for (name, rect) in crops {
                if let Some(rect) = rect {
                    if let Ok(path) =
                        save_crop(name, &image, rect.x, rect.y, rect.w, rect.h, "debug_out")
                    {
                        println!("Saved {} crop to {}", name, path.display());
                    }
                }
            }
            first_frame = false;
        }

        frame_index += 1;
        io::stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(120));
    }
}
