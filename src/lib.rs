//! MapleSyrup vision and game-state library.
//!
//! This crate provides:
//! - **capture**: Windows game window capture via DirectX/WGC.
//! - **vision**: Complete perception pipeline with confidence/temporal reasoning.
//! - **knowledge**: Structured MapleStory mechanics and heuristics.
//! - **util**: Timing, pixel, and image manipulation helpers.
//! - **hud**: Convenience re-export of HUD detection API for backwards compatibility.

pub mod capture;
pub mod config;
pub mod frame;
pub mod hud;
pub mod knowledge;
pub mod logging;
pub mod util;
pub mod vision;
