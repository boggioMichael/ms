//! The folder Claude Code is started in for a game with MapleSyrup: the
//! MCP config that starts the bridge (`.mcp.json`), the brief Claude reads
//! (`CLAUDE.md`), the permissions that let it talk and look without asking
//! each time (`.claude/settings.json`), and a launcher (`Start Claude.cmd`)
//! that starts MapleSyrup if it is not running and then Claude with the
//! channel. `MapleSyrup --setup-claude` writes it beside the program.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

/// The brief: who Claude is talking to, and how.
pub const BRIEF: &str = include_str!("kit/CLAUDE.md");

/// The folder's name, beside the program.
pub const FOLDER: &str = "Claude";

/// The launcher's name.
pub const LAUNCHER: &str = "Start Claude.cmd";

/// The MCP server's name: Claude's tools are `mcp__maplesyrup__…`, and the
/// development flag names it `server:maplesyrup`.
pub const SERVER: &str = "maplesyrup";

/// `.mcp.json`: Claude Code starts `exe --claude-channel` as the channel.
pub fn mcp_json(exe: &Path) -> String {
    let config = json!({
        "mcpServers": {
            SERVER: {
                "command": exe.to_string_lossy(),
                "args": ["--claude-channel"],
            }
        }
    });
    serde_json::to_string_pretty(&config).unwrap_or_default() + "\n"
}

/// `.claude/settings.json`: the project's server trusted, and its tools
/// (and looking things up on the web) allowed without a prompt each time.
pub fn settings_json() -> String {
    let settings = json!({
        "enableAllProjectMcpServers": true,
        "enabledMcpjsonServers": [SERVER],
        "permissions": {
            "allow": [
                format!("mcp__{SERVER}__say"),
                format!("mcp__{SERVER}__game_status"),
                format!("mcp__{SERVER}__look_at_screen"),
                "WebSearch",
                "WebFetch",
            ]
        }
    });
    serde_json::to_string_pretty(&settings).unwrap_or_default() + "\n"
}

/// `Start Claude.cmd`: Claude Code found (or, when it is not on this PC,
/// installed from claude.ai once the player agrees), MapleSyrup (the
/// program one folder up) started if it is not running — and if an older
/// one without Claude's channel is, a word to close it — then Claude Code
/// with the channel: Sonnet at low effort, quick enough to talk with
/// (`/model` and `/effort` change it), and Remote Control on, so the
/// session is in the Claude app too. (Claude Code asks each time whether to
/// load a development channel: custom channels need that while channels
/// are a research preview.) Plain ASCII: cmd reads it in the console's code
/// page, and the paths with the player's name in them come from variables.
pub fn launcher() -> String {
    [
        "@echo off",
        "rem Claude with MapleSyrup's live channel: MapleSyrup reads the game, Claude talks with you.",
        "cd /d \"%~dp0\"",
        "set \"CLAUDE=claude\"",
        "where claude >nul 2>nul",
        "if not errorlevel 1 goto :maplesyrup",
        "set \"CLAUDE=%USERPROFILE%\\.local\\bin\\claude.exe\"",
        "if exist \"%CLAUDE%\" goto :maplesyrup",
        "echo Claude Code is not installed on this PC yet.",
        "echo Press any key to install it now from claude.ai - it takes about a minute - or close this window.",
        "pause >nul",
        "curl -fsSL https://claude.ai/install.cmd -o \"%TEMP%\\claude-install.cmd\"",
        "call \"%TEMP%\\claude-install.cmd\"",
        "del \"%TEMP%\\claude-install.cmd\" >nul 2>nul",
        "if not exist \"%CLAUDE%\" (",
        "  echo Claude Code did not install. See https://code.claude.com/docs/en/setup",
        "  pause",
        "  exit /b 1",
        ")",
        ":maplesyrup",
        "if exist \"%~dp0.mcp.json\" goto :running",
        "echo Setting this folder up for Claude, once...",
        "echo.| \"%~dp0..\\MapleSyrup.exe\" --setup-claude >nul",
        "if exist \"%~dp0.mcp.json\" goto :running",
        "echo MapleSyrup couldn't set this folder up: is MapleSyrup.exe one folder up?",
        "pause",
        "exit /b 1",
        ":running",
        "tasklist /FI \"IMAGENAME eq MapleSyrup.exe\" 2>nul | find /I \"MapleSyrup.exe\" >nul",
        "if errorlevel 1 goto :start",
        "if exist \"%APPDATA%\\MapleSyrup\\claude-link.json\" goto :claude",
        "echo MapleSyrup is open, but a version without Claude's channel.",
        "echo Close MapleSyrup, then run this again.",
        "pause",
        "exit /b 1",
        ":start",
        "start \"MapleSyrup\" \"%~dp0..\\MapleSyrup.exe\"",
        ":claude",
        "\"%CLAUDE%\" --dangerously-load-development-channels server:maplesyrup --model sonnet --effort low --remote-control MapleSyrup",
        "",
    ]
    .join("\r\n")
}

/// The brief, with what the player wrote about himself for MapleSyrup
/// (`about-me.txt`: his name, his language, how he likes to be helped,
/// his character) at its end.
pub fn brief(about: Option<&str>) -> String {
    let about = about.map(str::trim).filter(|a| !a.is_empty());
    match about {
        Some(about) => format!("{BRIEF}\n## About him (what he wrote for MapleSyrup)\n\n{about}\n"),
        None => BRIEF.to_string(),
    }
}

/// Write the folder in `dir` for the program at `exe`, with `about` (the
/// player's own words about himself) in the brief. The brief and the
/// launcher are kept when they are there already (the player may have made
/// the brief his own; the launcher may be what is running this); the MCP
/// config and the settings are written fresh. Returns the files written.
pub fn write(dir: &Path, exe: &Path, about: Option<&str>) -> std::io::Result<Vec<PathBuf>> {
    fs::create_dir_all(dir.join(".claude"))?;
    let mut written = Vec::new();
    let mut put = |name: &str, text: &str, keep: bool| -> std::io::Result<()> {
        let path = dir.join(name);
        if keep && path.exists() {
            return Ok(());
        }
        fs::write(&path, text)?;
        written.push(path);
        Ok(())
    };
    put(".mcp.json", &mcp_json(exe), false)?;
    put(".claude/settings.json", &settings_json(), false)?;
    // (The launcher runs this when the folder is not set up yet: a batch
    // file rewritten while it runs is read on from where it was.)
    put(LAUNCHER, &launcher(), true)?;
    put("CLAUDE.md", &brief(about), true)?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mcp_config_starts_the_bridge_from_a_path_with_hebrew_in_it() {
        let exe = Path::new(r"C:\Users\מיכאל\Desktop\MapleSyrup\MapleSyrup.exe");
        let config: serde_json::Value = serde_json::from_str(&mcp_json(exe)).unwrap();
        assert_eq!(
            config["mcpServers"]["maplesyrup"]["command"],
            r"C:\Users\מיכאל\Desktop\MapleSyrup\MapleSyrup.exe"
        );
        assert_eq!(
            config["mcpServers"]["maplesyrup"]["args"][0],
            "--claude-channel"
        );
    }

    #[test]
    fn the_tools_are_allowed_and_the_launcher_needs_no_hebrew() {
        let settings: serde_json::Value = serde_json::from_str(&settings_json()).unwrap();
        let allowed: Vec<&str> = settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(allowed.contains(&"mcp__maplesyrup__say"));
        assert!(allowed.contains(&"mcp__maplesyrup__look_at_screen"));
        assert_eq!(settings["enabledMcpjsonServers"][0], "maplesyrup");
        let launcher = launcher();
        assert!(
            launcher.is_ascii(),
            "cmd reads the file in the console's code page"
        );
        assert!(launcher.contains("--dangerously-load-development-channels server:maplesyrup"));
        assert!(
            launcher.contains("--remote-control"),
            "the session is in the Claude app too"
        );
        assert!(
            launcher.contains("--setup-claude"),
            "a folder not set up is set up first"
        );
        assert!(launcher.contains("\r\n"));
    }

    #[test]
    fn the_brief_tells_claude_to_talk_through_say_and_keeps_the_players_own() {
        assert!(BRIEF.contains("`say`"));
        assert!(BRIEF.contains("Classic World"));
        // What he wrote about himself goes at its end; nothing, nothing.
        let about = "My name is Michael (מיכאל). Talk to me in Hebrew.";
        let with = brief(Some(about));
        assert!(with.starts_with(BRIEF));
        assert!(with.ends_with(&format!("{about}\n")));
        assert_eq!(brief(Some("  \n")), BRIEF);
        let dir = std::env::temp_dir().join(format!(
            "ms-claude-kit-{}",
            crate::phone::tls::random_hex(4)
        ));
        let exe = Path::new("/opt/MapleSyrup/maplesyrup");
        let written = write(&dir, exe, Some(about)).unwrap();
        assert_eq!(written.len(), 4);
        assert!(dir.join(".claude/settings.json").exists());
        assert!(
            fs::read_to_string(dir.join("CLAUDE.md"))
                .unwrap()
                .contains("Michael (מיכאל)")
        );
        fs::write(dir.join("CLAUDE.md"), "my own brief").unwrap();
        let again = write(&dir, exe, Some(about)).unwrap();
        assert_eq!(
            again.len(),
            2,
            "his brief is his, the launcher is left as it is"
        );
        assert_eq!(
            fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
            "my own brief"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
