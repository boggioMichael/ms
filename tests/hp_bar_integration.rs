use image::io::Reader as ImageReader;
use ms::debug::{detect_ui_markers, find_hp_bar, save_ui_debug_overlay};

#[test]
fn finds_hp_bar_in_resource_photo() {
    let path = std::path::Path::new("resources/maplestory_hp_frame.png");
    if !path.exists() {
        panic!("resources/maplestory_hp_frame.png is missing");
    }

    let image = ImageReader::open(path)
        .expect("failed to open resource image")
        .decode()
        .expect("failed to decode resource image")
        .to_rgba8();

    let markers = detect_ui_markers(&image);
    let debug_path = save_ui_debug_overlay("maplestory_ui", &image, &markers, "debug_out")
        .expect("failed to save UI debug overlay");

    assert!(
        markers.hp_bar.is_some(),
        "HP unknown; saved debug overlay at {}",
        debug_path.display()
    );
    assert!(markers.mp_bar.is_some(), "MP should be detected");
    assert!(markers.exp_bar.is_some(), "EXP should be detected");
    assert!(markers.name_plate.is_some(), "player name should be detected");
    assert!(markers.class_plate.is_some(), "class should be detected");
    assert!(markers.level_plate.is_some(), "level should be detected");

    let bar =
        find_hp_bar(&image).expect("HP bar should be detected in the provided resource frame");
    assert!(bar.w >= 40, "HP bar width should be at least 40 pixels");
    assert!(bar.h >= 4, "HP bar height should be at least 4 pixels");
    assert!(
        bar.x < image.width() / 2,
        "HP bar should be located in the left half of the frame"
    );
}
