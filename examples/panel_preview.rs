//! The panel over the game (without its text, which Windows draws), laid on
//! the committed screenshot: `cargo run --example panel_preview -- out.png`.

use ms::app::dog::Dog;
use ms::app::panel::{self, Content};
use ms::companion::Observation;
use ms::vision::snapshot::PerceptionPipeline;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "panel-preview.png".into());
    let mut game = image::open("resources/maplestory.png")
        .expect("run from the repository")
        .to_rgba8();
    let world = PerceptionPipeline::new().detect(&game);
    let obs = Observation::from_world("MapleStory", &world);
    let scale = panel::scale_for(game.height() as i32);
    let (_, h) = panel::size(scale);
    let mut dog = Dog::load().unwrap();
    let frame = dog.frame(10, h).clone();
    let painted = panel::paint(
        &Content {
            obs: Some(&obs),
            exp_per_hour: Some(9.1),
            phone_connected: Some(true),
            speaking: true,
            muted: false,
            last_line: Some("Hey! HP looks great."),
            dog: Some(&frame),
        },
        scale,
    );
    let x = game.width() - painted.image.width() - 12;
    // Premultiplied over.
    for (px, py, p) in painted.image.enumerate_pixels() {
        let dst = game.get_pixel_mut(x + px, 12 + py);
        let a = p.0[3] as u32;
        for c in 0..3 {
            dst.0[c] = (p.0[c] as u32 + dst.0[c] as u32 * (255 - a) / 255).min(255) as u8;
        }
    }
    game.save(&out).unwrap();
    println!("{out}");
}
