//! MapleSyrup CLI.
//!
//! M0 wires the pipeline end to end with the mock vision backend, so
//! the seams between stages are exercised before any model or renderer
//! exists. Run `ms monitors` to confirm capture works on your machine.

use ms_agent::Agent;
use ms_capture::{Capturer, ScreenCapturer, Target};
use ms_overlay::OverlayState;
use ms_vision::{ChangeDetector, MockVision, Vision};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");

    let result = match command {
        "monitors" => monitors(),
        "capture" => capture_once(),
        "run" => run_once(args.get(1).cloned()),
        "help" | "--help" | "-h" => {
            help();
            Ok(())
        }
        other => {
            eprintln!("ms: unknown command {other:?}\n");
            help();
            return std::process::ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ms: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn help() {
    println!(
        "MapleSyrup - a desktop AI agent that sees the screen\n\
         \n\
         USAGE:\n\
         \x20 ms monitors      list attached monitors\n\
         \x20 ms capture       capture one frame and report its geometry\n\
         \x20 ms run [goal]    run one pipeline cycle (mock vision backend)\n\
         \n\
         M0 scaffold: capture is real, vision is a mock that claims no\n\
         understanding, and the overlay has no renderer yet. See\n\
         ARCHITECTURE.md for the roadmap."
    );
}

fn monitors() -> ms_core::Result<()> {
    let found = ms_capture::monitors()?;
    if found.is_empty() {
        println!("no monitors detected");
        return Ok(());
    }
    for m in found {
        let primary = if m.is_primary { " (primary)" } else { "" };
        println!(
            "  {:<24} {}x{} at ({}, {}){}",
            m.name, m.width, m.height, m.origin.x, m.origin.y, primary
        );
    }
    Ok(())
}

fn capture_once() -> ms_core::Result<()> {
    let mut capturer = ScreenCapturer::new(Target::PrimaryMonitor);
    match capturer.next_frame()? {
        Some(frame) => {
            println!("captured {frame:?}");
            Ok(())
        }
        None => {
            println!("capture source produced no frame");
            Ok(())
        }
    }
}

fn run_once(goal: Option<String>) -> ms_core::Result<()> {
    let goal = goal.unwrap_or_else(|| "observe the screen".to_string());
    let mut capturer = ScreenCapturer::new(Target::PrimaryMonitor);
    let mut detector = ChangeDetector::new(0.01);
    let mut vision = MockVision;
    let mut agent = Agent::new(&goal);
    let mut overlay = OverlayState::new();

    println!("goal: {goal}");

    let Some(frame) = capturer.next_frame()? else {
        println!("no frame available");
        return Ok(());
    };
    println!("captured {frame:?}");

    if !detector.is_changed(&frame) {
        println!("frame unchanged; vision skipped");
        return Ok(());
    }

    let perception = vision.perceive(&frame)?;
    println!(
        "perception [{}]: {}",
        perception.backend, perception.summary
    );

    let annotations = agent.observe(perception);
    println!("agent produced {} annotation(s)", annotations.len());

    overlay.replace(annotations);
    println!(
        "overlay would draw {} item(s) (no renderer until M2)",
        overlay.visible().len()
    );
    Ok(())
}
