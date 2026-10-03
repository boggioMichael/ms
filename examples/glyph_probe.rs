use ms::vision::hud_geometry::detect_ui_markers;
use syrup::glyphs::{GlyphOptions, GlyphSet};
use syrup::threshold::Channel;

fn main() {
    let frame = image::open("resources/maplestory.png").unwrap().to_rgba8();
    let m = detect_ui_markers(&frame);
    let regions = [
        ("HP", m.hp_bar.unwrap(), "HP[400/400]"),
        ("MP", m.mp_bar.unwrap(), "MP[1291/1351]"),
        ("EXP", m.exp_bar.unwrap(), "EXP35900[37.51%]"),
    ];
    for channel in [Channel::Min, Channel::Luma, Channel::Max] {
        for span in [60u8, 40, 25] {
            let mut font = GlyphSet::new(GlyphOptions {
                channel,
                evidence_span: span,
                ..Default::default()
            });
            let mut learned = Vec::new();
            for (name, r, label) in &regions {
                match font.learn(&frame, *r, label) {
                    Ok(n) => learned.push(format!("{name}:{n}")),
                    Err(e) => learned.push(format!("{name}:ERR {e}")),
                }
            }
            let mut reads = Vec::new();
            for (name, r, _) in &regions {
                let d = font.read(&frame, *r);
                let all = font
                    .read_all(&frame, *r)
                    .map(|t| t.text)
                    .unwrap_or_default();
                reads.push(format!("{name}={:?} (all: {all})", d.value.map(|t| t.text)));
            }
            println!(
                "{channel:?} span {span}: learn {} | read {}",
                learned.join(" "),
                reads.join(" | ")
            );
        }
    }
}
