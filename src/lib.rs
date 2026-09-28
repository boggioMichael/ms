//! MapleSyrup vision and game-state library.
//!
//! This crate provides:
//! - **capture**: Windows game window capture via DirectX/WGC.
//! - **vision**: Complete perception pipeline with confidence/temporal reasoning.
//! - **knowledge**: Structured MapleStory mechanics and heuristics.
//! - **util**: Timing, pixel, and image manipulation helpers.
//! - **game_state**: Serializable game state aggregating all vision outputs.
//! - **hud**: Convenience re-export of HUD detection API for backwards compatibility.
//! - **observe**: Live terminal dashboard and graphical preview of the running pipeline.

pub mod capture;
pub mod config;
pub mod frame;
pub mod game_state;
pub mod hud;
pub mod knowledge;
pub mod logging;
pub mod observe;
pub mod util;
pub mod vision;
