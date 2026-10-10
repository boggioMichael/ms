//! The classic HUD (MapleStory Classic World), read in the game's own font.
//!
//! `resources/hud-classic-4k-strip.png` is the bottom 216 rows of a player's
//! 3,840 × 2,160 frame on Classic World: the old status bar, whose numbers
//! sit above the bars, small and thin, dressed with the field's name and
//! yellow-green brackets —
//!
//! ```text
//!   HP[178/178]     MP[101/101]     EXP. 619[49.84%]
//!   ███████████     ███████████     ██████░░░░░
//! ```
//!
//! Only the HP/MP/EXP part of the status bar is kept (x 1,380 – 2,440); the
//! rest is painted (20, 20, 30), so the character's name is not in the
//! repository. MapleSyrup saved the frame with its own "HUD found" box drawn
//! on it; the two rows of the box that crossed the text (y 2,089 – 2,090 of
//! the frame) were repaired by interpolating the rows above and below.
//!
//! The bands are the ones the player's own sight found for this HUD
//! (`learned/layout.json`), and the labels are the ones the teacher gave
//! for it, spelled the ways a labeller spells them. Learning used to fail
//! for every one of them: the line was looked for at most 16 rows above the
//! bar, which at 4K holds only the feet of the digits (22 rows tall, ending
//! 11 rows above the bar), and on and around the bar, where the bar's own
//! nine tick marks and two ends split into eleven "glyphs".
//!
//! `resources/hud-4k-strip.png` is the modern HUD of the same player's
//! main character (bold digits on the bars): one font learns both, and each
//! HUD must still read right.

use std::path::PathBuf;
use std::time::Instant;

use image::RgbaImage;
use ms::sight::Sight;
use ms::sight::numbers::{Field, Numbers, Value};
use syrup::geometry::NormRect;

/// A strip of the bottom of a 4K frame, as the whole frame.
fn frame(strip: &str) -> RgbaImage {
    let strip = image::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(strip))
        .expect("the strip fixture")
        .to_rgba8();
    let mut frame = RgbaImage::from_pixel(3840, 2160, image::Rgba([20, 20, 30, 255]));
    image::imageops::replace(&mut frame, &strip, 0, (2160 - strip.height()) as i64);
    frame
}

fn classic() -> RgbaImage {
    frame("resources/hud-classic-4k-strip.png")
}

fn modern() -> RgbaImage {
    frame("resources/hud-4k-strip.png")
}

/// The classic HUD's bars, as the player's sight found them.
const CLASSIC: [(Field, NormRect); 3] = [
    (
        Field::Hp,
        NormRect {
            x0: 0.36822918,
            y0: 0.9777778,
            x1: 0.4450521,
            y1: 0.9925926,
        },
    ),
    (
        Field::Mp,
        NormRect {
            x0: 0.44739583,
            y0: 0.9777778,
            x1: 0.5239583,
            y1: 0.9930556,
        },
    ),
    (
        Field::Exp,
        NormRect {
            x0: 0.53020835,
            y0: 0.9777778,
            x1: 0.6132866,
            y1: 0.9930556,
        },
    ),
];

/// What the classic HUD says: HP 178/178, MP 101/101, EXP 49.84%.
fn classic_value(field: Field) -> Value {
    match field {
        Field::Hp => Value::Amount {
            current: 178,
            max: 178,
        },
        Field::Mp => Value::Amount {
            current: 101,
            max: 101,
        },
        Field::Exp => Value::Percent(49.84),
    }
}

/// The modern HUD's bars: HP and MP found from the pixels; the EXP bar as
/// the player's sight had it once a reading fitted its track (from the
/// pixels alone it is its fill, 34% of the screen's width, and the line
/// printed over the middle of the bar lies beyond it).
fn modern_bands(frame: &RgbaImage) -> [(Field, NormRect); 3] {
    let dir = temp_dir("modern-bands");
    let mut sight = Sight::load(&dir);
    sight.find_hud(frame).expect("the modern HUD is found");
    let layout = sight.layout.clone().expect("a layout");
    let _ = std::fs::remove_dir_all(&dir);
    [
        (Field::Hp, layout.hp.expect("the HP bar").band),
        (Field::Mp, layout.mp.expect("the MP bar").band),
        (
            Field::Exp,
            NormRect {
                x0: 0.0,
                y0: 0.9907407,
                x1: 0.99979514,
                y1: 0.9990741,
            },
        ),
    ]
}

/// What the modern HUD says, as the teacher spells it, and its value.
fn modern_line(field: Field) -> (&'static str, Value) {
    match field {
        Field::Hp => (
            "4785 / 4785",
            Value::Amount {
                current: 4785,
                max: 4785,
            },
        ),
        Field::Mp => (
            "3084 / 3105",
            Value::Amount {
                current: 3084,
                max: 3105,
            },
        ),
        Field::Exp => ("1,185,906 [34.36%]", Value::Percent(34.36)),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ms-classic-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Reads `field` on `frame` a few times over: the same every time.
fn read_steadily(
    numbers: &mut Numbers,
    frame: &RgbaImage,
    field: Field,
    band: &NormRect,
) -> Option<Value> {
    let first = numbers.read(frame, field, band);
    for _ in 0..4 {
        assert_eq!(
            numbers.read(frame, field, band),
            first,
            "{field:?} read differently on the same frame"
        );
    }
    first.map(|r| r.value)
}

#[test]
fn every_label_the_teacher_gives_for_the_classic_hud_is_learned_and_read_every_frame() {
    let frame = classic();
    let now = Instant::now();
    let labels: [(Field, &[&str]); 3] = [
        (Field::Hp, &["178/178", "HP[178/178]", "HP [178/178]"]),
        (Field::Mp, &["101/101", "MP[101/101]", "MP [101/101]"]),
        (
            Field::Exp,
            &[
                "619[49.84%]",
                "EXP 619[49.84%]",
                "EXP 619 [49.84%]",
                "49.84%",
            ],
        ),
    ];
    let mut failed = Vec::new();
    for ((field, band), (_, labels)) in CLASSIC.iter().zip(labels) {
        for label in labels {
            let dir = temp_dir(&format!(
                "{}-{}",
                field.label(),
                label.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
            ));
            let mut numbers = Numbers::load(&dir);
            let learned = numbers.learn(&frame, *field, band, label, "model", now);
            let read = read_steadily(&mut numbers, &frame, *field, band);
            let again = Numbers::load(&dir)
                .read(&frame, *field, band)
                .map(|r| r.value);
            if learned.is_err() || read != Some(classic_value(*field)) || again != read {
                failed.push(format!(
                    "{field:?} {label:?}: learned {learned:?}; read {read:?}; after a reload {again:?}"
                ));
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

#[test]
fn a_classic_label_that_is_not_what_the_line_says_is_not_learned() {
    // The teacher's second look read the EXP number as 613, and the line
    // says 619: the percent agrees with the bar, the digits do not.
    let frame = classic();
    let dir = temp_dir("wrong");
    let mut numbers = Numbers::load(&dir);
    let (field, band) = CLASSIC[2];
    let why = numbers
        .learn(&frame, field, &band, "613[49.84%]", "model", Instant::now())
        .unwrap_err();
    assert!(why.contains("could not be learned"), "{why}");
    assert_eq!(numbers.glyphs(), 0, "nothing kept from a wrong label");
    assert!(numbers.read(&frame, field, &band).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_modern_hud_is_read_as_before() {
    let frame = modern();
    let dir = temp_dir("modern");
    let mut numbers = Numbers::load(&dir);
    let now = Instant::now();
    for (field, band) in modern_bands(&frame) {
        let (label, value) = modern_line(field);
        let learned = numbers.learn(&frame, field, &band, label, "model", now);
        assert!(learned.is_ok(), "{field:?}: {learned:?}");
        assert_eq!(
            read_steadily(&mut numbers, &frame, field, &band),
            Some(value),
            "{field:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_modern_hp_line_is_not_learned_as_the_mp_line_above_its_bar() {
    // On the modern HUD the HP bar lies right over the MP bar, its numbers
    // on its own fill: a label that swaps the two (both bars full, so the
    // bar cannot tell) must not teach MP to read HP's numbers from above.
    let frame = modern();
    let bands = modern_bands(&frame);
    let dir = temp_dir("swapped");
    let mut numbers = Numbers::load(&dir);
    let now = Instant::now();
    let swapped = numbers.learn(&frame, Field::Mp, &bands[1].1, "4785 / 4785", "model", now);
    assert!(swapped.is_err(), "{swapped:?}");
    assert!(numbers.read(&frame, Field::Mp, &bands[1].1).is_none());
    // The right label is learned on the bar, and read.
    numbers
        .learn(&frame, Field::Mp, &bands[1].1, "3084 / 3105", "model", now)
        .unwrap();
    assert_eq!(
        read_steadily(&mut numbers, &frame, Field::Mp, &bands[1].1),
        Some(modern_line(Field::Mp).1)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_font_learns_both_huds_in_either_order_and_reads_each() {
    let classic = classic();
    let modern = modern();
    let modern_bands = modern_bands(&modern);
    let now = Instant::now();
    for classic_first in [true, false] {
        let dir = temp_dir(&format!("both-{classic_first}"));
        let mut numbers = Numbers::load(&dir);
        let learn_classic = |numbers: &mut Numbers| {
            for (field, band) in CLASSIC {
                let label = match field {
                    Field::Hp => "HP[178/178]",
                    Field::Mp => "MP[101/101]",
                    Field::Exp => "619[49.84%]",
                };
                let learned = numbers.learn(&classic, field, &band, label, "model", now);
                assert!(learned.is_ok(), "classic {field:?}: {learned:?}");
            }
        };
        let learn_modern = |numbers: &mut Numbers| {
            for (field, band) in modern_bands {
                let learned =
                    numbers.learn(&modern, field, &band, modern_line(field).0, "model", now);
                assert!(learned.is_ok(), "modern {field:?}: {learned:?}");
            }
        };
        if classic_first {
            learn_classic(&mut numbers);
            learn_modern(&mut numbers);
        } else {
            learn_modern(&mut numbers);
            learn_classic(&mut numbers);
        }
        // The player switches characters, back and forth: each HUD reads
        // right on its first frame, with the same font, and after a reload.
        for round in 0..2 {
            for (field, band) in CLASSIC {
                assert_eq!(
                    read_steadily(&mut numbers, &classic, field, &band),
                    Some(classic_value(field)),
                    "classic {field:?}, classic learned first: {classic_first}, round {round}"
                );
            }
            for (field, band) in modern_bands {
                assert_eq!(
                    read_steadily(&mut numbers, &modern, field, &band),
                    Some(modern_line(field).1),
                    "modern {field:?}, classic learned first: {classic_first}, round {round}"
                );
            }
            numbers = Numbers::load(&dir);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
