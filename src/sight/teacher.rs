//! What MapleSyrup asks a vision model, and how it reads the answers.
//!
//! The model is the teacher: it finds the HUD on the player's real game
//! screen (whatever the client, resolution or UI layout) and reads the
//! numbers, and MapleSyrup learns from that to measure the same things on
//! its own, frame by frame. Answers are strict JSON (structured outputs),
//! checked here before anything is believed.
//!
//! Boxes go both ways as `[x0, y0, x1, y1]` from 0 to 1000 across the
//! picture, which carries rulers so the model can be exact.

use image::RgbaImage;
use serde_json::{Value, json};

use crate::ai::images::{self, NBox, Thousandths};

/// One question for the vision model.
#[derive(Debug, Clone)]
pub struct Look {
    /// The answer's schema name.
    pub name: &'static str,
    pub instructions: String,
    /// The parts of the question: text and pictures.
    pub content: Vec<Value>,
    /// The JSON schema the answer must follow, if any.
    pub schema: Option<Value>,
    pub max_output_tokens: u32,
}

/// HUD values as the game prints them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudValues {
    pub level: Option<u32>,
    /// Current and maximum.
    pub hp: Option<(u64, u64)>,
    pub mp: Option<(u64, u64)>,
    pub exp_percent: Option<f32>,
    /// The HP, MP and EXP lines exactly as printed, character for
    /// character (`HP [4200/5000]`), for learning the HUD's font.
    pub hp_text: Option<String>,
    pub mp_text: Option<String>,
    pub exp_text: Option<String>,
    pub map: Option<String>,
    pub name: Option<String>,
    pub job: Option<String>,
    pub notes: String,
}

impl HudValues {
    pub fn hp_percent(&self) -> Option<f32> {
        self.hp.map(|(c, m)| c as f32 / m as f32 * 100.0)
    }

    pub fn mp_percent(&self) -> Option<f32> {
        self.mp.map(|(c, m)| c as f32 / m as f32 * 100.0)
    }

    /// In a few words, for the log.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(l) = self.level {
            parts.push(format!("level {l}"));
        }
        if let Some((c, m)) = self.hp {
            parts.push(format!("HP {c}/{m}"));
        }
        if let Some((c, m)) = self.mp {
            parts.push(format!("MP {c}/{m}"));
        }
        if let Some(e) = self.exp_percent {
            parts.push(format!("EXP {e}%"));
        }
        if let Some(m) = &self.map {
            parts.push(format!("map {m}"));
        }
        if let Some(n) = &self.name {
            parts.push(format!("name {n}"));
        }
        if let Some(j) = &self.job {
            parts.push(format!("job {j}"));
        }
        if parts.is_empty() {
            "nothing readable".into()
        } else {
            parts.join(", ")
        }
    }
}

/// Where the HUD is, and what it says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Calibration {
    pub level: Option<NBox>,
    pub hp: Option<NBox>,
    pub mp: Option<NBox>,
    pub exp: Option<NBox>,
    pub minimap: Option<NBox>,
    pub values: HudValues,
}

fn nullable(kind: &str) -> Value {
    json!({"type": [kind, "null"]})
}

fn nullable_box(what: &str) -> Value {
    json!({"type": ["array", "null"], "items": {"type": "number"}, "description": what})
}

fn nullable_pair(what: &str) -> Value {
    json!({"type": ["array", "null"], "items": {"type": "integer"}, "description": what})
}

fn strict(properties: Value) -> Value {
    let required: Vec<String> = properties
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn value_properties() -> serde_json::Map<String, Value> {
    let mut p = serde_json::Map::new();
    p.insert("level".into(), nullable("integer"));
    p.insert("hp".into(), nullable_pair("[current, max] as printed"));
    p.insert("mp".into(), nullable_pair("[current, max] as printed"));
    p.insert("exp_percent".into(), nullable("number"));
    p.insert(
        "hp_text".into(),
        json!({"type": ["string", "null"], "description": "the HP line exactly as printed, character for character, label and brackets included (for example \"HP [4200/5000]\"), or null if any character is unclear"}),
    );
    p.insert(
        "mp_text".into(),
        json!({"type": ["string", "null"], "description": "the MP line exactly as printed, character for character, or null"}),
    );
    p.insert(
        "exp_text".into(),
        json!({"type": ["string", "null"], "description": "the EXP line exactly as printed, character for character (for example \"EXP 35900 [37.51%]\"), or null"}),
    );
    p.insert("map".into(), nullable("string"));
    p.insert("name".into(), nullable("string"));
    p.insert("job".into(), nullable("string"));
    p.insert("notes".into(), json!({"type": "string"}));
    p
}

const READING_RULES: &str = "Read numbers exactly as the game prints them. Never guess: if any digit is unclear, \
give null for that value. HP and MP are [current, max]. exp_percent is the EXP percentage the game shows \
(for example 86.25). level is the character's level (for example 61 for \"Lv. 61\"). map is the map's name, \
name the character's name, job the class or job. notes: anything that gets in the way (a window over the HUD, \
a loading or death screen), or an empty string.";

/// Find the HUD on a frame and read it: the first look at a new screen.
pub fn calibrate(frame: &RgbaImage) -> Look {
    let picture = images::with_rulers(&images::fit(frame, 1600, 1000));
    let (fw, fh) = frame.dimensions();
    let bottom = NBox::new(0.0, 0.72, 1.0, 1.0);
    let strip = images::fit(&images::crop(frame, &bottom), 2000, 400);
    let mut properties = value_properties();
    for (key, what) in [
        ("level_box", "the character's level number on the HUD"),
        ("hp_bar", "the HP bar's whole track, empty part included"),
        ("mp_bar", "the MP bar's whole track, empty part included"),
        ("exp_bar", "the EXP bar's whole track, empty part included"),
        ("minimap", "the minimap window"),
    ] {
        properties.insert(key.into(), nullable_box(what));
    }
    Look {
        name: "hud_layout",
        instructions: format!(
            "You look at screenshots of the game MapleStory (the current global client) for a companion app \
that measures the player's HUD by itself once it knows where it is.\n\
The first picture is the whole game window ({fw}×{fh}) with rulers on its edges: positions run from 0 to 1000 \
across and down the game picture (0,0 is its top left corner; the white margin with the numbers is outside it). \
The second picture is the bottom of the window at full size, for reading small print.\n\
Give tight boxes [x0, y0, x1, y1] in the first picture's 0–1000 coordinates, or null for anything not on screen: \
level_box, hp_bar, mp_bar and exp_bar (each bar's whole track, from where its fill starts to where a full bar would \
end, including any empty part; not the label or the numbers beside it), and minimap. Be precise to a few units: \
use the rulers and the faint grid lines every 100.\n{READING_RULES}"
        ),
        content: vec![
            json!({"type": "input_text", "text": "Find the HUD and read it."}),
            images::input_image(images::jpeg_url(&picture, 88), "high"),
            images::input_image(images::png_url(&strip), "high"),
        ],
        schema: Some(strict(Value::Object(properties))),
        max_output_tokens: 600,
    }
}

/// Read the HUD again where it is known to be: the regular check.
pub fn verify(frame: &RgbaImage, status: &NBox) -> Look {
    let mut crop = images::crop(frame, status);
    if crop.height() < 90 {
        crop = images::enlarged(&crop, 2);
    }
    let crop = images::fit(&crop, 2000, 600);
    Look {
        name: "hud_values",
        instructions: format!(
            "You read the HUD of the game MapleStory for a companion app. The picture is the part of the screen \
with the character's level and the HP, MP and EXP bars.\n{READING_RULES}"
        ),
        content: vec![
            json!({"type": "input_text", "text": "Read the HUD."}),
            images::input_image(images::png_url(&crop), "high"),
        ],
        schema: Some(strict(Value::Object(value_properties()))),
        max_output_tokens: 300,
    }
}

/// Pin down something the player pointed at: a close-up around the rough
/// box, with rulers. Returns the question and the close-up's place in the
/// frame (the answer's box is within it).
pub fn refine(frame: &RgbaImage, rough: &NBox, name: &str, describe: &str) -> (Look, NBox) {
    let (fw, fh) = frame.dimensions();
    // The rough box with as much again around it, at least 80 pixels.
    let min_w = 80.0 / fw as f32;
    let min_h = 80.0 / fh as f32;
    let (cx, cy) = rough.center();
    let half_w = (rough.width() * 1.0).max(min_w);
    let half_h = (rough.height() * 1.0).max(min_h);
    let around = NBox::new(cx - half_w, cy - half_h, cx + half_w, cy + half_h);
    let mut close = images::crop(frame, &around);
    if close.width() < 400 {
        close = images::enlarged(&close, (400 / close.width().max(1)).clamp(1, 4));
    }
    let picture = images::with_rulers(&images::fit(&close, 1200, 1200));
    let look = Look {
        name: "pinpoint",
        instructions: "You help a companion app for the game MapleStory learn what something looks like. The picture \
is a close-up of the game screen with rulers on its edges: positions run from 0 to 1000 across and down the \
picture (the white margin is outside it). Give the tight box [x0, y0, x1, y1] around the thing described, \
in those coordinates, or null if it is not in the picture. If there are several, pick the one nearest the middle. \
seen: what you see there, in a few words."
            .into(),
        content: vec![
            json!({"type": "input_text", "text": format!("Find: {name} — {describe}")}),
            images::input_image(images::png_url(&picture), "high"),
        ],
        schema: Some(strict(json!({
            "box": nullable_box("the tight box, 0 to 1000"),
            "seen": {"type": "string"},
        }))),
        max_output_tokens: 200,
    };
    (look, around)
}

/// A close look at part of the screen, to answer a question about it.
pub fn closer(frame: &RgbaImage, place: &NBox, question: &str) -> Look {
    let mut crop = images::crop(frame, place);
    if crop.width() < 500 {
        crop = images::enlarged(&crop, (500 / crop.width().max(1)).clamp(1, 4));
    }
    let crop = images::fit(&crop, 1600, 1600);
    Look {
        name: "closer",
        instructions: "You look closely at part of a MapleStory game screen for a companion app and answer the \
question about it briefly and exactly, reading any text as written. If it can't be made out, say so."
            .into(),
        content: vec![
            json!({"type": "input_text", "text": question}),
            images::input_image(images::png_url(&crop), "high"),
        ],
        schema: None,
        max_output_tokens: 300,
    }
}

/// Is `candidate` the same kind of thing as `reference` (a monster in
/// another pose, say)?
pub fn same(candidate: &RgbaImage, reference: &RgbaImage, name: &str, describe: &str) -> Look {
    let big = |i: &RgbaImage| images::enlarged(i, (192 / i.width().max(1)).clamp(1, 6));
    Look {
        name: "same_thing",
        instructions: "A companion app for MapleStory learned what something looks like from the reference picture \
and found the candidate picture on screen. Say whether the candidate shows the same kind of thing (the same \
monster, NPC, item or icon, possibly in another pose, animation frame or facing the other way) — not just \
something similar-looking. If unsure, say false."
            .into(),
        content: vec![
            json!({"type": "input_text", "text": format!("Reference: {name} — {describe}. First the reference, then the candidate.")}),
            images::input_image(images::png_url(&big(reference)), "high"),
            images::input_image(images::png_url(&big(candidate)), "high"),
        ],
        schema: Some(strict(json!({"same": {"type": "boolean"}}))),
        max_output_tokens: 50,
    }
}

fn parse(text: &str) -> Option<serde_json::Map<String, Value>> {
    let text = text.trim();
    // A model may wrap JSON in a code fence.
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(|t| t.trim_end_matches("```").trim())
        .unwrap_or(text);
    serde_json::from_str::<Value>(text)
        .ok()?
        .as_object()
        .cloned()
}

fn nbox(v: Option<&Value>) -> Option<NBox> {
    let values: Vec<f64> = v?.as_array()?.iter().filter_map(Value::as_f64).collect();
    NBox::from_thousandths(&values)
}

fn pair(v: Option<&Value>) -> Option<(u64, u64)> {
    let a = v?.as_array()?;
    let (c, m) = (a.first()?.as_u64()?, a.get(1)?.as_u64()?);
    // A believable reading: a maximum, and a current not above it.
    (m > 0 && m < 1_000_000_000 && c <= m).then_some((c, m))
}

fn text(v: Option<&Value>) -> Option<String> {
    v?.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() < 80)
        .map(String::from)
}

fn values(o: &serde_json::Map<String, Value>) -> HudValues {
    HudValues {
        level: o
            .get("level")
            .and_then(Value::as_u64)
            .filter(|l| (1..=300).contains(l))
            .map(|l| l as u32),
        hp: pair(o.get("hp")),
        mp: pair(o.get("mp")),
        exp_percent: o
            .get("exp_percent")
            .and_then(Value::as_f64)
            .filter(|e| (0.0..=100.0).contains(e))
            .map(|e| e as f32),
        hp_text: text(o.get("hp_text")),
        mp_text: text(o.get("mp_text")),
        exp_text: text(o.get("exp_text")),
        map: text(o.get("map")),
        name: text(o.get("name")),
        job: text(o.get("job")),
        notes: o
            .get("notes")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
    }
}

pub fn parse_calibration(answer: &str) -> Option<Calibration> {
    let o = parse(answer)?;
    Some(Calibration {
        level: nbox(o.get("level_box")),
        hp: nbox(o.get("hp_bar")),
        mp: nbox(o.get("mp_bar")),
        exp: nbox(o.get("exp_bar")),
        minimap: nbox(o.get("minimap")),
        values: values(&o),
    })
}

pub fn parse_values(answer: &str) -> Option<HudValues> {
    Some(values(&parse(answer)?))
}

/// The pinpointed box, in fractions of the whole frame.
pub fn parse_refined(answer: &str, around: &NBox) -> Option<NBox> {
    let o = parse(answer)?;
    Some(nbox(o.get("box"))?.within(around))
}

pub fn parse_same(answer: &str) -> Option<bool> {
    parse(answer)?.get("same")?.as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn answers_are_read_and_checked() {
        let c = parse_calibration(
            r#"{"level_box":[30,940,80,965],"hp_bar":[410,930,600,945],"mp_bar":null,"exp_bar":[0,990,1000,1000],
               "minimap":[0,0,200,180],"level":61,"hp":[4200,5000],"mp":[900,2000],"exp_percent":86.25,
               "map":"Henesys","name":"Michael","job":"Assassin","notes":""}"#,
        )
        .unwrap();
        assert_eq!(c.level.unwrap().to_thousandths(), [30, 940, 80, 965]);
        assert!(c.mp.is_none());
        assert_eq!(c.values.level, Some(61));
        assert_eq!(c.values.hp, Some((4200, 5000)));
        assert_eq!(c.values.hp_percent(), Some(84.0));
        assert_eq!(c.values.exp_percent, Some(86.25));
        assert_eq!(c.values.job.as_deref(), Some("Assassin"));
        // Unbelievable values are dropped; a code fence is fine.
        let v = parse_values("```json\n{\"level\":0,\"hp\":[6000,5000],\"mp\":[1,2],\"exp_percent\":186,\"map\":\"\",\"name\":null,\"job\":null,\"notes\":\"menu open\"}\n```").unwrap();
        assert_eq!(v.level, None);
        assert_eq!(v.hp, None);
        assert_eq!(v.mp, Some((1, 2)));
        assert_eq!(v.exp_percent, None);
        assert_eq!(v.map, None);
        assert_eq!(v.notes, "menu open");
        assert!(parse_values("not json").is_none());
        // A box found in a close-up, back in the frame.
        let around = NBox::new(0.5, 0.5, 0.7, 0.7);
        let b = parse_refined(r#"{"box":[250,250,750,750],"seen":"a mushroom"}"#, &around).unwrap();
        assert!(
            (b.x0 - 0.55).abs() < 1e-5 && (b.y1 - 0.65).abs() < 1e-5,
            "{b:?}"
        );
        assert_eq!(
            parse_refined(r#"{"box":null,"seen":"nothing"}"#, &around),
            None
        );
        assert_eq!(parse_same(r#"{"same":true}"#), Some(true));
    }

    #[test]
    fn questions_carry_pictures_and_a_strict_schema() {
        let frame = RgbaImage::from_pixel(1366, 768, Rgba([30, 60, 90, 255]));
        let look = calibrate(&frame);
        assert_eq!(look.content.len(), 3);
        assert!(
            look.content[1]["image_url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/jpeg;base64,")
        );
        let schema = look.schema.unwrap();
        assert_eq!(schema["additionalProperties"], false);
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for key in ["level_box", "hp_bar", "level", "hp", "exp_percent", "notes"] {
            assert!(required.contains(&key), "{key}");
        }
        assert!(look.instructions.contains("1366×768"));
        let (look, around) = refine(
            &frame,
            &NBox::new(0.5, 0.5, 0.52, 0.53),
            "Orange Mushroom",
            "orange cap",
        );
        assert!(around.width() > 0.05);
        assert!(
            look.content[0]["text"]
                .as_str()
                .unwrap()
                .contains("Orange Mushroom")
        );
        assert!(
            verify(&frame, &NBox::new(0.2, 0.9, 0.8, 1.0))
                .schema
                .is_some()
        );
        assert!(
            closer(
                &frame,
                &NBox::new(0.0, 0.0, 0.3, 0.3),
                "What quest is this?"
            )
            .schema
            .is_none()
        );
    }
}
