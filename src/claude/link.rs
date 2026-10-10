//! Where the bridge finds the running companion: the port of its local
//! board and the key every request carries, written to the settings folder
//! by the companion as it starts, read by `MapleSyrup --claude-channel`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file, in the settings folder.
pub const FILE: &str = "claude-link.json";
/// The key's own file: kept across runs, like the phone link's.
const KEY_FILE: &str = "claude-key.txt";
/// The port the board asks for first (it takes the next free one after).
pub const DEFAULT_PORT: u16 = 8790;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub port: u16,
    pub key: String,
    /// The companion's process, for a bridge that wonders whether it is
    /// still the one running.
    pub pid: u32,
}

/// The board's key: saved, or made (128 bits) and saved for next time.
pub fn key(dir: &Path) -> String {
    let path = dir.join(KEY_FILE);
    if let Ok(saved) = fs::read_to_string(&path) {
        let saved = saved.trim();
        if saved.len() >= 32 && saved.chars().all(|c| c.is_ascii_hexdigit()) {
            return saved.to_string();
        }
    }
    let key = crate::phone::tls::random_hex(16);
    if fs::create_dir_all(dir).is_ok() {
        let _ = fs::write(&path, &key);
    }
    key
}

/// Write the link (whole, or not at all: a bridge never reads half of it).
pub fn save(dir: &Path, link: &Link) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(link).map_err(std::io::Error::other)?;
    let partial = dir.join(format!("{FILE}.{}.partial", std::process::id()));
    fs::write(&partial, json)?;
    fs::rename(&partial, dir.join(FILE)).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })
}

/// The link the running companion wrote, if there is one that parses.
pub fn load(dir: &Path) -> Option<Link> {
    let text = fs::read_to_string(dir.join(FILE)).ok()?;
    serde_json::from_str(&text)
        .ok()
        .filter(|l: &Link| l.port != 0 && !l.key.is_empty())
}

/// The companion is stopping: its link goes with it, unless another run
/// wrote its own meanwhile.
pub fn remove(dir: &Path, pid: u32) {
    if load(dir).is_some_and(|l| l.pid == pid) {
        let _ = fs::remove_file(dir.join(FILE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ms-claude-link-{name}-{}",
            crate::phone::tls::random_hex(4)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_key_is_made_once_and_kept() {
        let dir = temp_dir("key");
        let first = key(&dir);
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(key(&dir), first);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_is_saved_read_and_removed_only_by_its_own_run() {
        let dir = temp_dir("link");
        assert_eq!(load(&dir), None);
        let link = Link {
            port: 8790,
            key: "ab".repeat(16),
            pid: 42,
        };
        save(&dir, &link).unwrap();
        assert_eq!(load(&dir), Some(link.clone()));
        // Another run's link is not this one's to remove.
        remove(&dir, 7);
        assert_eq!(load(&dir), Some(link));
        remove(&dir, 42);
        assert_eq!(load(&dir), None);
        // Garbage is no link.
        fs::write(dir.join(FILE), "{not json").unwrap();
        assert_eq!(load(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
