use image::io::Reader as ImageReader;
use ms::debug::{
    capture_window_by_title, crop::save_crop, detect_ui_markers, save_ui_debug_overlay,
};
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

fn print_rect(name: &str, rect: Option<ms::debug::Rect>) {
    match rect {
        Some(rect) => println!(
            "{}: x={}, y={}, w={}, h={}",
            name, rect.x, rect.y, rect.w, rect.h
        ),
        None => println!("{}: unknown", name),
    }
}

fn print_percent(name: &str, value: Option<f32>) {
    match value {
        Some(p) => println!("{}: {:.1}%", name, p),
        None => println!("{}: unknown", name),
    }
}

fn print_presence(name: &str, rect: Option<ms::debug::Rect>) {
    match rect {
        Some(_) => println!("{}: detected", name),
        None => println!("{}: unknown", name),
    }
}

fn format_percent(value: Option<f32>) -> String {
    value.map(|p| format!("{p:.1}%")).unwrap_or_else(|| "unknown".into())
}

fn format_presence(rect: Option<ms::debug::Rect>) -> &'static str {
    if rect.is_some() { "detected" } else { "unknown" }
}

fn print_hud_summary(frame_index: usize, markers: &ms::debug::UiMarkers) {
    let hp = format_percent(markers.hp_percent);
    let mp = format_percent(markers.mp_percent);
    let xp = format_percent(markers.exp_percent);
    let name = format_presence(markers.name_plate);
    let job = format_presence(markers.class_plate);
    let level = format_presence(markers.level_plate);

    println!(
        "[frame {frame_index}] HP: {hp} | MP: {mp} | XP: {xp} | Name: {name} | Job: {job} | Level: {level}"
    );
}

fn load_image() -> Option<image::RgbaImage> {
    if let Some(image) = capture_window_by_title("maplestory") {
        return Some(image);
    }

    if Path::new("resources/maplestory_hp_frame.png").exists() {
        let image = ImageReader::open("resources/maplestory_hp_frame.png")
            .ok()?
            .decode()
            .ok()?
            .to_rgba8();
        return Some(image);
    }

    None
}

fn main() {
    let mut frame_index = 0usize;
    let mut first_frame = true;

    loop {
        let image = match load_image() {
            Some(image) => image,
            None => {
                println!("No live Maplestory window capture was available and no fallback screenshot was found.");
                println!(
                    "Open the Maplestory Chrome window or place a frame at resources/maplestory_hp_frame.png and rerun."
                );
                return;
            }
        };

        let markers = detect_ui_markers(&image);
        println!("===============================");
        print_hud_summary(frame_index, &markers);
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
                    if let Ok(path) = save_crop(name, &image, rect.x, rect.y, rect.w, rect.h, "debug_out") {
                        println!("Saved {} crop to {}", name, path.display());
                    }
                }
            }
            first_frame = false;
        }

        print_rect("HP bar", markers.hp_bar);
        print_percent("HP %", markers.hp_percent);
        print_rect("MP bar", markers.mp_bar);
        print_percent("MP %", markers.mp_percent);
        print_rect("XP bar", markers.exp_bar);
        print_percent("XP %", markers.exp_percent);
        print_presence("Name plate", markers.name_plate);
        print_presence("Job plate", markers.class_plate);
        print_presence("Level plate", markers.level_plate);

        frame_index += 1;
        io::stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(120));
    }
}
