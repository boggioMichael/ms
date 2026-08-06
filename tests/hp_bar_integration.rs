use image::io::Reader as ImageReader;
use ms::hud::{detect_hud_snapshot, save_ui_debug_overlay};

#[test]
fn finds_hp_bar_in_resource_photo() {
    let path = std::path::Path::new("resources/last.png");
    if !path.exists() {
        panic!("resources/last.png is missing");
    }

    let image = ImageReader::open(path)
        .expect("failed to open resource image")
        .decode()
        .expect("failed to decode resource image")
        .to_rgba8();

    let snapshot = detect_hud_snapshot(&image);
    let markers = snapshot.markers;
    let debug_path = save_ui_debug_overlay("maplestory_ui", &image, &markers, "out")
        .expect("failed to save UI debug overlay");

    assert!(
        markers.hp_bar.is_some(),
        "HP unknown; saved debug overlay at {}",
        debug_path.display()
    );
    assert!(markers.mp_bar.is_some(), "MP should be detected");
    assert!(markers.exp_bar.is_some(), "EXP should be detected");
    let hp = markers.hp_bar.expect("HP bar should be detected");
    let mp = markers.mp_bar.expect("MP bar should be detected");
    let exp = markers.exp_bar.expect("EXP bar should be detected");
    let hud_top = image.height() * 9 / 10;
    assert!(
        hp.y >= hud_top && mp.y >= hud_top && exp.y >= hud_top,
        "status bars must come from the bottom HUD: HP={hp:?}, MP={mp:?}, EXP={exp:?}"
    );
    assert!(
        hp.x < mp.x && mp.x < exp.x,
        "expected HP, MP, EXP left-to-right order: HP={hp:?}, MP={mp:?}, EXP={exp:?}"
    );
    assert!(hp.w >= 40, "HP bar width should be at least 40 pixels");
    assert!(mp.w >= 40, "MP bar width should be at least 40 pixels");
    assert!(exp.w >= 40, "EXP bar width should be at least 40 pixels");

    let name = markers.name_plate.expect("name row should be detected");
    let job = markers.class_plate.expect("job row should be detected");
    let level = markers.level_plate.expect("level row should be detected");
    assert!(
        name.x < image.width() / 4 && name.y < job.y && job.y < level.y,
        "expected stacked player stat rows: name={name:?}, job={job:?}, level={level:?}"
    );
}
