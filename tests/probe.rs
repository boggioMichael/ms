use image::RgbaImage;
use ms::vision::geometry::Rect;
use ms::vision::hud_geometry::detect_ui_markers;
use ms::vision::quality::assess_text_quality;

#[test]
fn probe() {
    let fx: RgbaImage = image::io::Reader::open("resources/maplestory.png")
        .unwrap()
        .decode()
        .unwrap()
        .to_rgba8();
    let m = detect_ui_markers(&fx);
    for (n, r) in [
        ("fixture HP", m.hp_bar),
        ("fixture MP", m.mp_bar),
        ("fixture EXP", m.exp_bar),
    ] {
        if let Some(r) = r {
            let q = assess_text_quality(
                &fx,
                Rect {
                    x: r.x,
                    y: r.y,
                    w: r.w,
                    h: 16,
                },
            );
            println!(
                "{n:14} sharpness={:.3} samples={}",
                q.sharpness, q.edge_samples
            );
        }
    }
    if let Ok(rd) = image::io::Reader::open("out/live-MINGW64.png") {
        let live = rd.decode().unwrap().to_rgba8();
        for (n, y) in [
            ("live term row1", 40u32),
            ("live term row2", 120),
            ("live term row3", 300),
        ] {
            let q = assess_text_quality(
                &live,
                Rect {
                    x: 0,
                    y,
                    w: live.width(),
                    h: 16,
                },
            );
            println!(
                "{n:14} sharpness={:.3} samples={}",
                q.sharpness, q.edge_samples
            );
        }
    } else {
        println!("no live capture saved");
    }
}
