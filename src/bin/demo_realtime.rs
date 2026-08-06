// Real-time game state perception demo
//
// This demo continuously captures MapleStory frames, processes them through
// the complete perception pipeline, and serializes the output into a structured
// game state object. All data is displayed with latency measurements to prove
// real-time AI-reactive capability.
//
// Run with:
//   cargo run --release --bin demo_realtime
//
// Or with specific log level:
//   RUST_LOG=info cargo run --release --bin demo_realtime

use ms::capture::capture_game_window_info;
use ms::game_state::*;
use ms::logging::init_tracing;
use ms::util::timing::{FrameTimer, FPSCounter};
use ms::vision::snapshot::PerceptionPipeline;
use std::time::Instant;

fn main() {
    init_tracing("info");

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║                                                                ║");
    println!("║         🎮 REAL-TIME GAME STATE PERCEPTION DEMO 🎮             ║");
    println!("║                                                                ║");
    println!("║   Continuous frame capture → vision pipeline → game state     ║");
    println!("║                                                                ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    println!("Waiting for MapleStory window capture...\n");

    loop {
        if let Some((window_title, image)) = capture_game_window_info() {
            println!("\n✅ Captured: {} ({}x{})", window_title, image.width(), image.height());
            println!("\n🚀 Starting perception pipeline...\n");

            run_perception_loop(&image);
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn run_perception_loop(initial_image: &image::RgbaImage) {
    let mut pipeline = PerceptionPipeline::new();
    let mut frame_timer = FrameTimer::new();
    let mut fps_counter = FPSCounter::new(30);
    let mut frame_number: u64 = 0;
    let start_time = Instant::now();

    let mut last_window_title = String::new();

    // Process initial frame
    process_and_display_frame(
        &mut pipeline,
        initial_image,
        &mut frame_number,
        &mut frame_timer,
        &mut fps_counter,
        start_time,
    );

    // Continuous capture loop
    let mut frame_count_since_title_update = 0;
    loop {
        match capture_game_window_info() {
            Some((window_title, image)) => {
                // Update title every 30 frames to reduce console spam
                if frame_count_since_title_update == 0 {
                    if window_title != last_window_title {
                        println!(
                            "📍 Window: {} ({}x{})",
                            window_title, image.width(), image.height()
                        );
                        last_window_title = window_title;
                    }
                }
                frame_count_since_title_update = (frame_count_since_title_update + 1) % 30;

                process_and_display_frame(
                    &mut pipeline,
                    &image,
                    &mut frame_number,
                    &mut frame_timer,
                    &mut fps_counter,
                    start_time,
                );
            }
            None => {
                println!("\n⚠️  Window lost. Retrying in 1 second...");
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
}

fn process_and_display_frame(
    pipeline: &mut PerceptionPipeline,
    image: &image::RgbaImage,
    frame_number: &mut u64,
    frame_timer: &mut ms::util::timing::FrameTimer,
    fps_counter: &mut ms::util::timing::FPSCounter,
    start_time: Instant,
) {
    frame_timer.mark();

    let perception_start = Instant::now();
    let world_state = pipeline.detect(image);
    let perception_elapsed = perception_start.elapsed();

    *frame_number += 1;

    // Serialize complete game state
    let game_state = GameState {
        timestamp_ms: start_time.elapsed().as_millis() as u64,
        frame_number: *frame_number,
        frame_width: image.width(),
        frame_height: image.height(),
        hud: SerializedHudState {
            hp: SerializedMetric {
                percent: world_state.hud.hp.value.as_ref().and_then(|m| m.percent),
                current: world_state.hud.hp.value.as_ref().and_then(|m| m.value),
                max: None, // HudMetric doesn't store max
                confidence: world_state.hud.hp.confidence.value(),
                reliability: format!("{:?}", world_state.hud.hp.reliability),
            },
            mp: SerializedMetric {
                percent: world_state.hud.mp.value.as_ref().and_then(|m| m.percent),
                current: world_state.hud.mp.value.as_ref().and_then(|m| m.value),
                max: None,
                confidence: world_state.hud.mp.confidence.value(),
                reliability: format!("{:?}", world_state.hud.mp.reliability),
            },
            exp: SerializedMetric {
                percent: world_state.hud.exp.value.as_ref().and_then(|m| m.percent),
                current: world_state.hud.exp.value.as_ref().and_then(|m| m.value),
                max: None,
                confidence: world_state.hud.exp.confidence.value(),
                reliability: format!("{:?}", world_state.hud.exp.reliability),
            },
            character: SerializedCharacter {
                name: world_state.hud.player_name.value.clone(),
                job: world_state.hud.character_class.value.clone(),
                level: world_state.hud.level.value.as_ref().and_then(|l| l.parse::<u32>().ok()),
                name_confidence: world_state.hud.player_name.confidence.value(),
                job_confidence: world_state.hud.character_class.confidence.value(),
                level_confidence: world_state.hud.level.confidence.value(),
            },
        },
        motion: SerializedMotionState {
            entities: world_state
                .motion
                .value
                .as_ref()
                .map(|entities| {
                    entities
                        .iter()
                        .map(|e| SerializedEntity {
                            id: e.id,
                            x: e.bounds.x,
                            y: e.bounds.y,
                            width: e.bounds.w,
                            height: e.bounds.h,
                            velocity_x: e.velocity.0,
                            velocity_y: e.velocity.1,
                            age_frames: e.age_frames,
                            is_predicted: e.is_predicted,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            total_count: world_state
                .motion
                .value
                .as_ref()
                .map(|v| v.len())
                .unwrap_or(0),
            confidence: world_state.motion.confidence.value(),
            failure_reason: world_state.motion.failure_reason.clone(),
        },
        dialog: SerializedDialogState {
            present: world_state.dialog.is_present(),
            x: world_state
                .dialog
                .value
                .as_ref()
                .map(|d| d.bounds.x),
            y: world_state
                .dialog
                .value
                .as_ref()
                .map(|d| d.bounds.y),
            width: world_state
                .dialog
                .value
                .as_ref()
                .map(|d| d.bounds.w),
            height: world_state
                .dialog
                .value
                .as_ref()
                .map(|d| d.bounds.h),
            dialog_kind: world_state
                .dialog
                .value
                .as_ref()
                .map(|d| format!("{:?}", d.kind)),
            text: world_state.dialog.value.as_ref().and_then(|d| d.text.clone()),
            confidence: world_state.dialog.confidence.value(),
        },
        panels: SerializedPanelState {
            minimap: world_state
                .minimap
                .value
                .as_ref()
                .map(|m| SerializedPanelInfo {
                    present: true,
                    x: Some(m.bounds.x),
                    y: Some(m.bounds.y),
                    width: Some(m.bounds.w),
                    height: Some(m.bounds.h),
                })
                .unwrap_or(SerializedPanelInfo {
                    present: false,
                    x: None,
                    y: None,
                    width: None,
                    height: None,
                }),
            chat_log: world_state
                .chat_log
                .value
                .as_ref()
                .map(|c| SerializedPanelInfo {
                    present: true,
                    x: Some(c.bounds.x),
                    y: Some(c.bounds.y),
                    width: Some(c.bounds.w),
                    height: Some(c.bounds.h),
                })
                .unwrap_or(SerializedPanelInfo {
                    present: false,
                    x: None,
                    y: None,
                    width: None,
                    height: None,
                }),
            buff_icons: world_state
                .icon_row
                .value
                .as_ref()
                .map(|i| i.icons.len())
                .unwrap_or(0),
        },
        environment: SerializedEnvironment {
            platform_edges: world_state
                .footholds
                .value
                .as_ref()
                .map(|edges| {
                    edges
                        .iter()
                        .map(|e| SerializedEdge {
                            y: e.bounds.y,
                            x_start: e.bounds.x,
                            x_end: e.bounds.x + e.bounds.w,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            total_edges: world_state
                .footholds
                .value
                .as_ref()
                .map(|v| v.len())
                .unwrap_or(0),
        },
        combat: SerializedCombat {
            intensity: world_state
                .combat_intensity
                .value
                .as_ref()
                .map(|c| format!("{:?}", c.intensity))
                .unwrap_or_else(|| "Unknown".to_string()),
            confidence: world_state.combat_intensity.confidence.value(),
        },
        processing_time_ms: perception_elapsed.as_secs_f32() * 1000.0,
    };

    // Display the game state
    println!("{}", game_state.to_display_string());

    fps_counter.add_frame_seconds(frame_timer.mark().as_secs_f64());
    let current_fps = fps_counter.fps();

    println!("📊 Performance: FPS={:.1} | Pipeline={:.2}ms",
        current_fps, game_state.processing_time_ms
    );
    println!("\n📋 Compact JSON State:\n{}\n", game_state.to_json_compact());

    // Periodically show full pretty JSON
    if *frame_number % 10 == 0 {
        println!("📄 Full Structured State (Frame {}):\n{}\n", frame_number, game_state.to_json_pretty());
    }
}
