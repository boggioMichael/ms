//! Serializable game state snapshot combining all vision perception data.
//!
//! This module provides a clean, JSON-serializable representation of the
//! complete game state derived from screen perception. All detection data
//! from the vision pipeline is serialized into this single comprehensive
//! object for consumption by downstream AI systems.

use serde::{Deserialize, Serialize};

/// Serializable version of a single HUD metric (HP, MP, EXP, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedMetric {
    pub percent: Option<f32>,
    pub current: Option<u64>,
    pub max: Option<u64>,
    pub confidence: f32,
    pub reliability: String,
}

/// Serializable character information from name/job/level plates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedCharacter {
    pub name: Option<String>,
    pub job: Option<String>,
    pub level: Option<u32>,
    pub name_confidence: f32,
    pub job_confidence: f32,
    pub level_confidence: f32,
}

/// Serializable HUD state: all character metrics and info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedHudState {
    pub hp: SerializedMetric,
    pub mp: SerializedMetric,
    pub exp: SerializedMetric,
    pub character: SerializedCharacter,
}

/// Serializable moving entity (player, monster, item, effect)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedEntity {
    pub id: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub velocity_x: f32,
    pub velocity_y: f32,
    pub age_frames: u32,
    pub is_predicted: bool,
}

/// Serializable motion/entity detection state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedMotionState {
    pub entities: Vec<SerializedEntity>,
    pub total_count: usize,
    pub confidence: f32,
    pub failure_reason: Option<String>,
}

/// Serializable dialog/popup state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedDialogState {
    pub present: bool,
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub dialog_kind: Option<String>,
    pub text: Option<String>,
    pub confidence: f32,
}

/// Serializable UI panel states
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedPanelState {
    pub minimap: SerializedPanelInfo,
    pub chat_log: SerializedPanelInfo,
    pub buff_icons: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedPanelInfo {
    pub present: bool,
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Serializable environment state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedEnvironment {
    pub platform_edges: Vec<SerializedEdge>,
    pub total_edges: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedEdge {
    pub y: u32,
    pub x_start: u32,
    pub x_end: u32,
}

/// Serializable combat state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedCombat {
    pub intensity: String,
    pub confidence: f32,
}

/// Complete serializable game state: everything the vision system perceived this frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    /// Frame metadata
    pub timestamp_ms: u64,
    pub frame_number: u64,
    pub frame_width: u32,
    pub frame_height: u32,

    /// Character state (HP, MP, EXP, name, job, level)
    pub hud: SerializedHudState,

    /// All moving entities detected this frame
    pub motion: SerializedMotionState,

    /// Dialog/popup/notification state
    pub dialog: SerializedDialogState,

    /// UI panel states (minimap, chat, buffs)
    pub panels: SerializedPanelState,

    /// Environment (platforms, footholds, edges)
    pub environment: SerializedEnvironment,

    /// Combat intensity and status
    pub combat: SerializedCombat,

    /// Per-frame performance metrics
    pub processing_time_ms: f32,
}

impl GameState {
    /// Create a GameState from a WorldState (vision pipeline output)
    pub fn from_world_state(world_state: crate::vision::snapshot::WorldState, frame_number: u64, width: u32, height: u32) -> Self {
        let timestamp_ms = chrono::Local::now().timestamp_millis() as u64;
        
        // Extract HUD data
        let hud_hp = world_state.hud.hp.value.as_ref();
        let hud_mp = world_state.hud.mp.value.as_ref();
        let hud_exp = world_state.hud.exp.value.as_ref();
        
        let hp = SerializedMetric {
            percent: hud_hp.map(|m| m.percent).flatten(),
            current: hud_hp.and_then(|m| m.value),
            max: None,
            confidence: world_state.hud.hp.confidence.value(),
            reliability: format!("{:?}", world_state.hud.hp.reliability),
        };
        
        let mp = SerializedMetric {
            percent: hud_mp.map(|m| m.percent).flatten(),
            current: hud_mp.and_then(|m| m.value),
            max: None,
            confidence: world_state.hud.mp.confidence.value(),
            reliability: format!("{:?}", world_state.hud.mp.reliability),
        };
        
        let exp = SerializedMetric {
            percent: hud_exp.map(|m| m.percent).flatten(),
            current: hud_exp.and_then(|m| m.value),
            max: None,
            confidence: world_state.hud.exp.confidence.value(),
            reliability: format!("{:?}", world_state.hud.exp.reliability),
        };
        
        let character = SerializedCharacter {
            name: world_state.hud.player_name.value.clone(),
            job: world_state.hud.character_class.value.clone(),
            level: world_state.hud.level.value.as_ref().and_then(|s| s.parse::<u32>().ok()),
            name_confidence: world_state.hud.player_name.confidence.value(),
            job_confidence: world_state.hud.character_class.confidence.value(),
            level_confidence: world_state.hud.level.confidence.value(),
        };
        
        let hud = SerializedHudState { hp, mp, exp, character };
        
        // Extract motion entities
        let entities = world_state.motion.value.as_deref().unwrap_or(&[])
            .iter()
            .map(|e| SerializedEntity {
                id: e.id,
                x: e.bounds.x as u32,
                y: e.bounds.y as u32,
                width: e.bounds.w as u32,
                height: e.bounds.h as u32,
                velocity_x: e.velocity.0,
                velocity_y: e.velocity.1,
                age_frames: e.age_frames,
                is_predicted: e.is_predicted,
            })
            .collect();
        
        let motion = SerializedMotionState {
            entities,
            total_count: world_state.motion.value.as_deref().map(|v| v.len()).unwrap_or(0),
            confidence: world_state.motion.confidence.value(),
            failure_reason: world_state.motion.failure_reason.clone(),
        };
        
        // Extract dialog
        let dialog = if let Some(dialog_reading) = world_state.dialog.value.as_ref() {
            SerializedDialogState {
                present: true,
                x: Some(dialog_reading.bounds.x as u32),
                y: Some(dialog_reading.bounds.y as u32),
                width: Some(dialog_reading.bounds.w as u32),
                height: Some(dialog_reading.bounds.h as u32),
                dialog_kind: Some(format!("{:?}", dialog_reading.kind)),
                text: dialog_reading.text.clone(),
                confidence: world_state.dialog.confidence.value(),
            }
        } else {
            SerializedDialogState {
                present: false,
                x: None,
                y: None,
                width: None,
                height: None,
                dialog_kind: None,
                text: None,
                confidence: world_state.dialog.confidence.value(),
            }
        };
        
        // Extract panels
        let panels = SerializedPanelState {
            minimap: SerializedPanelInfo {
                present: world_state.minimap.value.is_some(),
                x: world_state.minimap.value.as_ref().map(|m| m.bounds.x as u32),
                y: world_state.minimap.value.as_ref().map(|m| m.bounds.y as u32),
                width: world_state.minimap.value.as_ref().map(|m| m.bounds.w as u32),
                height: world_state.minimap.value.as_ref().map(|m| m.bounds.h as u32),
            },
            chat_log: SerializedPanelInfo {
                present: world_state.chat_log.value.is_some(),
                x: world_state.chat_log.value.as_ref().map(|c| c.bounds.x as u32),
                y: world_state.chat_log.value.as_ref().map(|c| c.bounds.y as u32),
                width: world_state.chat_log.value.as_ref().map(|c| c.bounds.w as u32),
                height: world_state.chat_log.value.as_ref().map(|c| c.bounds.h as u32),
            },
            buff_icons: world_state.icon_row.value.as_ref().map(|i| i.icons.len()).unwrap_or(0),
        };
        
        // Extract environment
        let platform_edges = world_state.footholds.value.as_deref().unwrap_or(&[])
            .iter()
            .map(|e| SerializedEdge {
                y: e.bounds.y as u32,
                x_start: e.bounds.x as u32,
                x_end: (e.bounds.x + e.bounds.w) as u32,
            })
            .collect();
        
        let environment = SerializedEnvironment {
            platform_edges,
            total_edges: world_state.footholds.value.as_deref().map(|v| v.len()).unwrap_or(0),
        };
        
        // Extract combat
        let combat = SerializedCombat {
            intensity: world_state.combat_intensity.value.as_ref()
                .map(|c| format!("{:?}", c.intensity))
                .unwrap_or_else(|| "Unknown".to_string()),
            confidence: world_state.combat_intensity.confidence.value(),
        };
        
        GameState {
            timestamp_ms,
            frame_number,
            frame_width: width,
            frame_height: height,
            hud,
            motion,
            dialog,
            panels,
            environment,
            combat,
            processing_time_ms: 0.0,
        }
    }

    /// Serialize game state to pretty-printed JSON string
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Serialize game state to compact JSON string
    pub fn to_json_compact(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Create a formatted display string for console output
    pub fn to_display_string(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "╔════════════════════════════════════════════════════════════════╗\n"
        ));
        output.push_str(&format!(
            "║ FRAME #{:<50} ║\n",
            self.frame_number
        ));
        output.push_str(&format!(
            "║ Time: {}ms | Resolution: {}x{:<29} ║\n",
            self.timestamp_ms,
            self.frame_width,
            self.frame_height
        ));
        output.push_str(&format!(
            "║ Process: {:.2}ms                                             ║\n",
            self.processing_time_ms
        ));
        output.push_str(&format!(
            "╠════════════════════════════════════════════════════════════════╣\n"
        ));

        // HUD State
        output.push_str(&format!(
            "║ [HUD STATE]                                                    ║\n"
        ));
        output.push_str(&format!(
            "║   HP:    {:<3}% (conf: {:.0}%) | MP: {:<3}% | EXP: {:<3}%           ║\n",
            self.hud.hp.percent.unwrap_or(0.0) as u32,
            self.hud.hp.confidence * 100.0,
            self.hud.mp.percent.unwrap_or(0.0) as u32,
            self.hud.exp.percent.unwrap_or(0.0) as u32,
        ));
        output.push_str(&format!(
            "║   Char:  {} Lv.{}                                  ║\n",
            self.hud.character.name.as_deref().unwrap_or("???"),
            self.hud.character.level.unwrap_or(0)
        ));
        output.push_str(&format!(
            "║   Job:   {}                                           ║\n",
            self.hud.character.job.as_deref().unwrap_or("???")
        ));
        output.push_str(&format!(
            "╠════════════════════════════════════════════════════════════════╣\n"
        ));

        // Motion State
        output.push_str(&format!(
            "║ [MOTION] {} entities detected (conf: {:.0}%)           ║\n",
            self.motion.total_count,
            self.motion.confidence * 100.0
        ));
        if self.motion.total_count > 0 {
            for (i, entity) in self.motion.entities.iter().take(3).enumerate() {
                output.push_str(&format!(
                    "║   Entity {}: ID={} pos=({},{}) vel=({:.1},{:.1}) age={}  ║\n",
                    i + 1,
                    entity.id,
                    entity.x,
                    entity.y,
                    entity.velocity_x,
                    entity.velocity_y,
                    entity.age_frames
                ));
            }
            if self.motion.total_count > 3 {
                output.push_str(&format!(
                    "║   ... and {} more                                      ║\n",
                    self.motion.total_count - 3
                ));
            }
        } else if let Some(reason) = &self.motion.failure_reason {
            output.push_str(&format!(
                "║   Reason: {}                         ║\n",
                reason
            ));
        }
        output.push_str(&format!(
            "╠════════════════════════════════════════════════════════════════╣\n"
        ));

        // Dialog State
        output.push_str(&format!(
            "║ [DIALOG] {} | {}                         ║\n",
            if self.dialog.present {
                "🔔 PRESENT"
            } else {
                "  absent "
            },
            self.dialog
                .dialog_kind
                .as_deref()
                .unwrap_or("           ")
        ));
        if let Some(text) = &self.dialog.text {
            let text_display = if text.len() > 50 {
                format!("{}...", &text[..47])
            } else {
                text.clone()
            };
            output.push_str(&format!(
                "║   Text: {}                                  ║\n",
                text_display
            ));
        }
        output.push_str(&format!(
            "╠════════════════════════════════════════════════════════════════╣\n"
        ));

        // Panels & Environment
        output.push_str(&format!(
            "║ [UI] Minimap: {} | Chat: {} | Buffs: {}                   ║\n",
            if self.panels.minimap.present {
                "✓"
            } else {
                "✗"
            },
            if self.panels.chat_log.present {
                "✓"
            } else {
                "✗"
            },
            self.panels.buff_icons
        ));
        output.push_str(&format!(
            "║ [ENV] {} platform edges detected                         ║\n",
            self.environment.total_edges
        ));
        output.push_str(&format!(
            "║ [COMBAT] {}                                     ║\n",
            self.combat.intensity
        ));
        output.push_str(&format!(
            "╚════════════════════════════════════════════════════════════════╝\n"
        ));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_json() {
        let state = GameState {
            timestamp_ms: 1000,
            frame_number: 1,
            frame_width: 1366,
            frame_height: 767,
            hud: SerializedHudState {
                hp: SerializedMetric {
                    percent: Some(75.0),
                    current: Some(750),
                    max: Some(1000),
                    confidence: 0.95,
                    reliability: "Corroborated".to_string(),
                },
                mp: SerializedMetric {
                    percent: Some(50.0),
                    current: Some(500),
                    max: Some(1000),
                    confidence: 0.92,
                    reliability: "Corroborated".to_string(),
                },
                exp: SerializedMetric {
                    percent: Some(33.0),
                    current: Some(3300),
                    max: Some(10000),
                    confidence: 0.88,
                    reliability: "Heuristic".to_string(),
                },
                character: SerializedCharacter {
                    name: Some("Knight".to_string()),
                    job: Some("Paladin".to_string()),
                    level: Some(100),
                    name_confidence: 0.85,
                    job_confidence: 0.80,
                    level_confidence: 0.90,
                },
            },
            motion: SerializedMotionState {
                entities: vec![],
                total_count: 0,
                confidence: 0.0,
                failure_reason: Some("warming up".to_string()),
            },
            dialog: SerializedDialogState {
                present: false,
                x: None,
                y: None,
                width: None,
                height: None,
                dialog_kind: None,
                text: None,
                confidence: 0.0,
            },
            panels: SerializedPanelState {
                minimap: SerializedPanelInfo {
                    present: true,
                    x: Some(0),
                    y: Some(0),
                    width: Some(100),
                    height: Some(100),
                },
                chat_log: SerializedPanelInfo {
                    present: true,
                    x: Some(0),
                    y: Some(667),
                    width: Some(200),
                    height: Some(100),
                },
                buff_icons: 3,
            },
            environment: SerializedEnvironment {
                platform_edges: vec![],
                total_edges: 0,
            },
            combat: SerializedCombat {
                intensity: "Idle".to_string(),
                confidence: 0.8,
            },
            processing_time_ms: 5.5,
        };

        let json = state.to_json_compact();
        assert!(!json.is_empty());
        assert!(json.contains("Knight"));
        assert!(json.contains("75"));
    }

    #[test]
    fn display_string_is_readable() {
        let state = GameState {
            timestamp_ms: 1000,
            frame_number: 42,
            frame_width: 1366,
            frame_height: 767,
            hud: SerializedHudState {
                hp: SerializedMetric {
                    percent: Some(100.0),
                    current: Some(1000),
                    max: Some(1000),
                    confidence: 1.0,
                    reliability: "Corroborated".to_string(),
                },
                mp: SerializedMetric {
                    percent: Some(100.0),
                    current: Some(1000),
                    max: Some(1000),
                    confidence: 1.0,
                    reliability: "Corroborated".to_string(),
                },
                exp: SerializedMetric {
                    percent: Some(0.0),
                    current: Some(0),
                    max: Some(10000),
                    confidence: 0.9,
                    reliability: "Heuristic".to_string(),
                },
                character: SerializedCharacter {
                    name: Some("Warrior".to_string()),
                    job: Some("Warrior".to_string()),
                    level: Some(50),
                    name_confidence: 0.9,
                    job_confidence: 0.85,
                    level_confidence: 0.95,
                },
            },
            motion: SerializedMotionState {
                entities: vec![SerializedEntity {
                    id: 1,
                    x: 100,
                    y: 200,
                    width: 32,
                    height: 48,
                    velocity_x: 2.5,
                    velocity_y: 0.0,
                    age_frames: 5,
                    is_predicted: false,
                }],
                total_count: 1,
                confidence: 0.85,
                failure_reason: None,
            },
            dialog: SerializedDialogState {
                present: false,
                x: None,
                y: None,
                width: None,
                height: None,
                dialog_kind: None,
                text: None,
                confidence: 0.0,
            },
            panels: SerializedPanelState {
                minimap: SerializedPanelInfo {
                    present: true,
                    x: Some(0),
                    y: Some(0),
                    width: Some(100),
                    height: Some(100),
                },
                chat_log: SerializedPanelInfo {
                    present: true,
                    x: Some(0),
                    y: Some(667),
                    width: Some(200),
                    height: Some(100),
                },
                buff_icons: 5,
            },
            environment: SerializedEnvironment {
                platform_edges: vec![],
                total_edges: 0,
            },
            combat: SerializedCombat {
                intensity: "Light".to_string(),
                confidence: 0.75,
            },
            processing_time_ms: 8.2,
        };

        let display = state.to_display_string();
        assert!(display.contains("FRAME #42"));
        assert!(display.contains("Warrior"));
        assert!(display.contains("50"));
    }
}
