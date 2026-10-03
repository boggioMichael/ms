//! Capturing the game window, through Syrup's capture: which of the
//! windows on screen is the game, a capture session that keeps the window
//! between frames, and the one-shot helpers the tools use.

use image::RgbaImage;
use syrup::capture::{CaptureError, Window};

/// Every visible, titled window, for picking one by hand.
pub fn list_windows() -> Vec<String> {
    syrup::capture::list_windows()
}

/// One capture of the first window whose title contains `search_title`:
/// its full title and its pixels. Looks the window up every call; a stream
/// of frames goes through [`GameCapture`].
pub fn capture_window_by_title_info(search_title: &str) -> Option<(String, RgbaImage)> {
    syrup::capture::capture_window_by_title_info(search_title)
}

/// One capture of the game window, if it is there: its title and pixels.
pub fn capture_game_window_info() -> Option<(String, RgbaImage)> {
    let target = pick_game_window(&list_windows())?;
    let mut window = Window::find(&target).ok().filter(|w| w.title() == target)?;
    let image = window.capture().ok()?;
    Some((target, image))
}

/// Windows whose titles can legitimately contain "maplestory" without
/// being the actual game client, e.g. a media player playing a recorded
/// clip or a browser tab with the word in its title.
const NON_GAME_TITLE_MARKERS: &[&str] = &[
    ".mp4",
    ".mkv",
    ".avi",
    ".mov",
    ".wmv",
    ".webm",
    ".gif",
    ".flv",
    "vlc",
    "media player",
    "potplayer",
    "mpc-",
    "mpc-hc",
    "quicktime",
    "youtube",
    "chrome",
    "firefox",
    "edge",
    "obs ",
    "maplesyrup",
];

/// Which of `titles` is the game: one called exactly "MapleStory", or else
/// the first containing it that is not a known non-game window (a video
/// player, a browser, MapleSyrup's own windows).
pub fn pick_game_window(titles: &[String]) -> Option<String> {
    const GAME: &str = "maplestory";
    let lowered: Vec<String> = titles.iter().map(|t| t.to_ascii_lowercase()).collect();
    let exact = lowered.iter().position(|t| t.trim() == GAME);
    let fallback = || {
        lowered
            .iter()
            .position(|t| t.contains(GAME) && !NON_GAME_TITLE_MARKERS.iter().any(|m| t.contains(m)))
    };
    exact.or_else(fallback).map(|i| titles[i].clone())
}

/// What one attempt to capture the game gave.
pub enum Captured {
    /// The game window's title and its pixels.
    Frame {
        title: String,
        image: image::RgbaImage,
    },
    /// No window that looks like the game.
    NotFound,
    /// The window is there but gave no picture, and why.
    Unavailable(String),
}

/// A capture session on the game window: the window is found once and kept
/// between frames (Syrup's [`Window`]), and a failed capture says why, so
/// the companion can tell "minimised" from "closed".
pub struct GameCapture {
    /// A title to match instead of looking for MapleStory.
    query: Option<String>,
    window: Option<Window>,
}

impl GameCapture {
    /// Find the MapleStory client by its title.
    pub fn auto() -> Self {
        Self {
            query: None,
            window: None,
        }
    }

    /// Capture the first window whose title contains `query` instead.
    pub fn titled(query: &str) -> Self {
        Self {
            query: Some(query.to_string()),
            window: None,
        }
    }

    pub fn capture(&mut self) -> Captured {
        if self.window.is_none() {
            let titles = syrup::capture::list_windows();
            let target = match &self.query {
                Some(query) => {
                    let query = query.to_lowercase();
                    titles
                        .iter()
                        .find(|t| t.to_lowercase().contains(&query) && !is_own_window(t))
                        .cloned()
                }
                None => pick_game_window(&titles),
            };
            let Some(target) = target else {
                return Captured::NotFound;
            };
            match Window::find(&target) {
                // syrup matches by "contains"; make sure it is the one chosen.
                Ok(window) if window.title() == target => self.window = Some(window),
                Ok(window) => {
                    return Captured::Unavailable(format!(
                        "another window, \"{}\", has the game's title in its own; close it or start MapleSyrup with --window",
                        window.title()
                    ));
                }
                Err(CaptureError::NotFound) => return Captured::NotFound,
                Err(e) => return Captured::Unavailable(e.to_string()),
            }
        }
        let Some(window) = self.window.as_mut() else {
            return Captured::NotFound;
        };
        match window.capture() {
            Ok(image) => Captured::Frame {
                title: window.title().to_string(),
                image,
            },
            Err(CaptureError::Closed | CaptureError::NotFound) => {
                self.window = None;
                Captured::NotFound
            }
            Err(CaptureError::Minimised) => Captured::Unavailable("it is minimised".into()),
            Err(e) => Captured::Unavailable(e.to_string()),
        }
    }
}

/// MapleSyrup's own windows (the preview, the console) mention the game.
fn is_own_window(title: &str) -> bool {
    title.to_ascii_lowercase().contains("maplesyrup")
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn titles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_exact_title_wins_over_lookalikes_above_it() {
        let list = titles(&["MapleStory boss run.mp4 - VLC", "MapleSyrup", "MapleStory"]);
        assert_eq!(pick_game_window(&list).as_deref(), Some("MapleStory"));
    }

    #[test]
    fn a_decorated_title_is_taken_when_nothing_is_exact() {
        let list = titles(&["MapleStory - YouTube - Google Chrome", "MapleStory v.271"]);
        assert_eq!(pick_game_window(&list).as_deref(), Some("MapleStory v.271"));
    }

    #[test]
    fn no_game_no_pick() {
        let list = titles(&["MapleSyrup — Vision Preview", "Notepad", "maplestory.mkv"]);
        assert_eq!(pick_game_window(&list), None);
    }
}
