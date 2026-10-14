//! MapleSyrup vision and game-state library.
//!
//! This crate provides:
//! - **capture**: Windows game window capture via DirectX/WGC.
//! - **vision**: Complete perception pipeline with confidence/temporal reasoning.
//! - **knowledge**: Structured MapleStory mechanics and heuristics.
//! - **util**: Per-stage timing of the vision path, from its tracing spans.
//! - **game_state**: Serializable game state aggregating all vision outputs.
//! - **hud**: Convenience re-export of HUD detection API for backwards compatibility.
//! - **observe**: Live terminal dashboard and graphical preview of the running pipeline.
//! - **companion**: What MapleSyrup says and when: warnings, level-ups, voice commands, EXP/hour.
//! - **coach**: When MapleSyrup speaks up on its own while the player plays — what it watches for, and
//!   how often a model gets to look.
//! - **phone**: The phone link — the page that makes a phone the companion's microphone and second screen.
//! - **platform**: The console, DPI awareness and the voice (Windows; quiet stand-ins elsewhere).
//! - **app**: The standalone companion's screen and session files (`maplesyrup` binary).
//! - **sight**: What MapleSyrup learned about the player's own screen (from a vision model and from
//!   the player): where the HUD is, the character's facts, and things it was taught to recognise.
//! - **perceive**: One frame through the companion's eyes — the detectors it still needs, then the
//!   sight — the same function for the companion and for `vision_bench`.
//! - **metrics**: What each session came to, in numbers (kept on the PC), and — only when the
//!   player turns it on — the same numbers made ready to share with partners; nothing is sent.
//! - **claude**: Claude, live — MapleSyrup as a channel into a Claude Code session: everything it
//!   reads pushed as it changes, what the player says passed on as said, Claude's words spoken.

pub mod ai;
pub mod app;
pub mod capture;
pub mod claude;
pub mod coach;
pub mod companion;
pub mod config;
pub mod game_state;
pub mod hud;
pub mod knowledge;
pub mod logging;
pub mod metrics;
pub mod observe;
pub mod perceive;
pub mod phone;
pub mod platform;
pub mod sight;
pub mod update;
pub mod util;
pub mod vision;
pub mod workshop;
