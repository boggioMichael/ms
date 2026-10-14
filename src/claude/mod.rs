//! Claude, live: MapleSyrup as a channel into a Claude Code session.
//!
//! The player talks with Claude, and Claude sees the game as it happens:
//! MapleSyrup pushes everything it reads off the screen into a Claude Code
//! session that runs on the same PC, the moment it changes. The session
//! is Claude's own (the player's subscription, what Claude Code knows of
//! the player and their projects, Remote Control from the Claude app);
//! MapleSyrup is its eyes, ears and mouth — it thinks nothing of its own
//! while Claude is connected.
//!
//! ```text
//!   MapleSyrup (the companion, running)          Claude Code (a session)
//!   ───────────────────────────────────          ───────────────────────
//!   sight + vision ─▶ Scene ─▶ Feed ─▶ events ┐
//!   phone mic ─▶ what the player said ────────┤   /local/events (long poll)
//!                                             ├─▶ maplesyrup --claude-channel ─▶ <channel> events
//!   Board (on 127.0.0.1, with a key) ◀────────┤   /local/status, /local/look  ◀─ tools
//!   voice ◀─ what Claude says ◀───────────────┘   /local/say                  ◀─ say
//! ```
//!
//! - [`feed`]: what the screen shows now ([`feed::Scene`]) and what changed
//!   since the last look ([`feed::Feed`]): a death, a level-up, a new map,
//!   HP in danger, a dialog, a fight, a taught thing.
//! - [`board`]: the running companion's side, served on this PC only: the
//!   events as they come, the reading now, the screen, and what Claude
//!   asks to have said.
//! - [`channel`]: the bridge Claude Code starts (`MapleSyrup --claude-channel`):
//!   an MCP server on stdio that declares Claude Code's `claude/channel`
//!   capability and turns each event into a `notifications/claude/channel`.
//! - [`link`]: where the bridge finds the running companion (its port and
//!   key, in the settings folder).
//! - [`kit`]: the folder Claude Code is started in — its MCP config, the
//!   brief, the permissions and a launcher.

pub mod board;
pub mod channel;
pub mod feed;
pub mod kit;
pub mod link;
