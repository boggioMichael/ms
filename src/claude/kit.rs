//! The folder Claude Code is started in for a game with MapleSyrup: the
//! MCP config that starts the bridge (`.mcp.json`), the brief Claude reads
//! (`CLAUDE.md`), an output style that makes the session Claude for
//! anything he asks, as in the Claude app, rather than a coding assistant
//! (`.claude/output-styles/maplesyrup.md`), the settings that choose it and
//! let Claude talk, look, search the web and read his folders without
//! asking each time (`.claude/settings.json`), and a launcher
//! (`Start Claude.cmd`) that starts MapleSyrup if it is not running and
//! then Claude with the channel. `MapleSyrup --setup-claude` writes it
//! beside the program, and the launcher runs that each time, so the folder
//! keeps up with the program.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

/// The brief: MapleSyrup's channel and tools, and the game.
pub const BRIEF: &str = include_str!("kit/CLAUDE.md");

/// The output style: Claude for anything, with MapleSyrup besides.
pub const STYLE: &str = include_str!("kit/style.md");

/// The style's name (its `name:`), as the settings choose it.
pub const STYLE_NAME: &str = "MapleSyrup";

/// Where the style is, in the folder.
pub const STYLE_FILE: &str = ".claude/output-styles/maplesyrup.md";

/// How a brief MapleSyrup wrote begins (and so may write again): its
/// marker line now, the brief's title before there was one.
const OURS: [&str; 2] = [
    "<!-- Written by MapleSyrup,",
    "# You are his MapleStory companion",
];

/// The folders in his home Claude reads without asking.
const READABLE: [&str; 7] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Pictures",
    "Videos",
    "Music",
    "OneDrive",
];

/// Where new projects go (the style says so), under his home: Claude
/// writes there without asking.
const PROJECTS: &str = "Documents/Claude projects";

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
                "args": ["--mcp"],
            }
        }
    });
    serde_json::to_string_pretty(&config).unwrap_or_default() + "\n"
}

/// `.claude/settings.json`: the project's server trusted, the style
/// chosen, and allowed without a prompt each time: all of MapleSyrup's
/// tools, the web, reading his folders (in `home`) and writing his new
/// projects. (Everything else is the permission mode's: the launcher starts
/// Claude in auto mode, where a check of its own stands in for the prompts.)
pub fn settings_json(home: Option<&Path>) -> String {
    let mut allow = vec![
        format!("mcp__{SERVER}"),
        "WebSearch".to_string(),
        "WebFetch".to_string(),
    ];
    if let Some(home) = home {
        let home = rule_path(home);
        allow.extend(
            READABLE
                .iter()
                .map(|folder| format!("Read({home}/{folder}/**)")),
        );
        allow.push(format!("Edit({home}/{PROJECTS}/**)"));
    }
    let settings = json!({
        "enableAllProjectMcpServers": true,
        "enabledMcpjsonServers": [SERVER],
        "outputStyle": STYLE_NAME,
        "permissions": {"allow": allow},
    });
    serde_json::to_string_pretty(&settings).unwrap_or_default() + "\n"
}

/// A path as Claude Code's permission rules write an absolute one: from the
/// root, with forward slashes, a Windows drive as its lower-case letter
/// (`C:\Users\x` is `//c/Users/x`).
pub fn rule_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/');
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        format!(
            "//{}{}",
            (bytes[0] as char).to_ascii_lowercase(),
            &text[2..]
        )
    } else if text.starts_with('/') {
        format!("/{text}")
    } else {
        text.to_string()
    }
}

/// `Start Claude.cmd`: Claude Code found (or, when it is not on this PC,
/// installed from claude.ai once the player agrees); a new MapleSyrup left
/// beside the program as `MapleSyrup-new.exe` put in its place while
/// neither runs; the folder set up again by the program (so it keeps up
/// with it); MapleSyrup (one folder up) started if it is not running — and
/// if an older one without Claude's channel is, a word to close it — then
/// Claude Code with the channel: Sonnet at low effort, quick enough to talk
/// with (`/model` and `/effort` change it); auto mode, where Claude's own
/// check of each action stands in for the prompts nobody would answer while
/// he plays; and Remote Control on, so the session is in the Claude app
/// too. (Claude Code asks each time whether to load a development channel:
/// custom channels need that while channels are a research preview.) Plain
/// ASCII: cmd reads it in the console's code page, and the paths with the
/// player's name in them come from variables.
pub fn launcher() -> String {
    [
        "@echo off",
        "rem Claude, as in the Claude app, with MapleSyrup's live view of your game and its voice.",
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
        "if not exist \"%~dp0..\\MapleSyrup-new.exe\" goto :setup",
        "tasklist /FI \"IMAGENAME eq MapleSyrup.exe\" 2>nul | find /I \"MapleSyrup.exe\" >nul",
        "if not errorlevel 1 goto :setup",
        "move /y \"%~dp0..\\MapleSyrup-new.exe\" \"%~dp0..\\MapleSyrup.exe\" >nul",
        ":setup",
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
        "\"%CLAUDE%\" --dangerously-load-development-channels server:maplesyrup --model sonnet --effort low --permission-mode auto --remote-control MapleSyrup",
        "",
    ]
    .join("\r\n")
}

/// The Claude desktop app's config files on this PC (Windows: the .exe
/// install's in `%APPDATA%\\Claude`, the Microsoft Store one's under
/// `%LOCALAPPDATA%\\Packages\\Claude_*`; elsewhere `~/.config/Claude` or
/// `~/Library/Application Support/Claude`): those whose folder exists — the
/// app was installed and opened.
pub fn desktop_configs() -> Vec<PathBuf> {
    const FILE: &str = "claude_desktop_config.json";
    let mut found = Vec::new();
    let mut consider = |dir: PathBuf| {
        if dir.is_dir() {
            found.push(dir.join(FILE));
        }
    };
    if let Some(appdata) = std::env::var_os("APPDATA") {
        consider(PathBuf::from(appdata).join("Claude"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA")
        && let Ok(packages) = fs::read_dir(PathBuf::from(local).join("Packages"))
    {
        for package in packages.flatten() {
            if package.file_name().to_string_lossy().starts_with("Claude_") {
                consider(
                    package
                        .path()
                        .join("LocalCache")
                        .join("Roaming")
                        .join("Claude"),
                );
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        consider(
            home.join("Library")
                .join("Application Support")
                .join("Claude"),
        );
        consider(home.join(".config").join("Claude"));
    }
    found
}

/// Add MapleSyrup to a Claude desktop app config (`mcpServers.maplesyrup`:
/// `exe --mcp`), keeping everything else in it; the file as it was is kept
/// beside it first. Returns whether anything changed. A file that is not a
/// JSON object is left alone (one mistake there turns all servers off).
pub fn add_to_desktop_config(path: &Path, exe: &Path) -> Result<bool, String> {
    let before = fs::read_to_string(path).unwrap_or_default();
    // (Saved by some editors, it starts with a byte-order mark.)
    let text = before.trim_start_matches('\u{feff}');
    let mut config: serde_json::Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text)
            .map_err(|e| format!("{} is not valid JSON ({e}); not touched", path.display()))?
    };
    let Some(fields) = config.as_object_mut() else {
        return Err(format!(
            "{} is not a JSON object; not touched",
            path.display()
        ));
    };
    let servers = fields.entry("mcpServers").or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err(format!(
            "mcpServers in {} is not an object; not touched",
            path.display()
        ));
    };
    let entry = json!({"command": exe.to_string_lossy(), "args": ["--mcp"]});
    if servers.get(SERVER) == Some(&entry) {
        return Ok(false);
    }
    servers.insert(SERVER.to_string(), entry);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if !before.is_empty() {
        let kept = path.with_extension("json.before-maplesyrup");
        fs::write(&kept, &before).map_err(|e| format!("couldn't keep a copy first: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())? + "\n";
    fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(true)
}

/// `Add MapleSyrup to Claude.cmd`, beside the program: runs
/// `--install-claude` and leaves the window open to read.
pub fn installer_cmd() -> String {
    [
        "@echo off",
        "rem Adds MapleSyrup to the Claude desktop app: Claude then sees the game through it.",
        "\"%~dp0MapleSyrup.exe\" --install-claude",
        "pause",
        "",
    ]
    .join("\r\n")
}

/// The brief, with MapleSyrup's file about the player (`about-me.txt`: what
/// he told it — his name, his language, how he likes to be helped, his
/// character) at its end.
pub fn brief(about: Option<&str>) -> String {
    let about = about.map(str::trim).filter(|a| !a.is_empty());
    match about {
        Some(about) => format!("{BRIEF}\n## About him (MapleSyrup's file about him)\n\n{about}\n"),
        None => BRIEF.to_string(),
    }
}

/// Whether a brief is one MapleSyrup wrote, and may write again.
fn ours(brief: &str) -> bool {
    let first = brief.trim_start_matches('\u{feff}').trim_start();
    OURS.iter().any(|start| first.starts_with(start))
}

/// Write the folder in `dir` for the program at `exe`, with `about` (its
/// file about the player) in the brief and his `home` folders readable.
/// The MCP config, the settings and the style are written fresh, and so is
/// the brief while it is MapleSyrup's own (it starts with its marker line:
/// a player who deletes that line keeps his own); the launcher is kept when
/// it is there (it is what runs this, and a batch file rewritten while it
/// runs is read on from where it was). Returns the files written.
pub fn write(
    dir: &Path,
    exe: &Path,
    about: Option<&str>,
    home: Option<&Path>,
) -> std::io::Result<Vec<PathBuf>> {
    fs::create_dir_all(dir.join(".claude").join("output-styles"))?;
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
    put(".claude/settings.json", &settings_json(home), false)?;
    put(STYLE_FILE, STYLE, false)?;
    put(LAUNCHER, &launcher(), true)?;
    let mine = fs::read_to_string(dir.join("CLAUDE.md")).is_ok_and(|brief| !ours(&brief));
    put("CLAUDE.md", &brief(about), mine)?;
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
        assert_eq!(config["mcpServers"]["maplesyrup"]["args"][0], "--mcp");
    }

    #[test]
    fn claude_may_use_maplesyrup_the_web_and_his_folders_and_the_launcher_needs_no_hebrew() {
        let home = Path::new(r"C:\Users\מיכאל");
        let settings: serde_json::Value = serde_json::from_str(&settings_json(Some(home))).unwrap();
        let allowed: Vec<&str> = settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(allowed.contains(&"mcp__maplesyrup"), "all its tools");
        assert!(allowed.contains(&"WebSearch") && allowed.contains(&"WebFetch"));
        assert!(
            allowed.contains(&"Read(//c/Users/מיכאל/Documents/**)"),
            "{allowed:?}"
        );
        assert!(allowed.contains(&"Read(//c/Users/מיכאל/Downloads/**)"));
        assert!(allowed.contains(&"Edit(//c/Users/מיכאל/Documents/Claude projects/**)"));
        assert_eq!(settings["enabledMcpjsonServers"][0], "maplesyrup");
        assert_eq!(settings["outputStyle"], STYLE_NAME);
        // Without a home, the folders are left to the permission mode.
        let plain: serde_json::Value = serde_json::from_str(&settings_json(None)).unwrap();
        assert_eq!(plain["permissions"]["allow"].as_array().unwrap().len(), 3);

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
            launcher.contains("--permission-mode auto"),
            "no prompt nobody answers while he plays"
        );
        // The folder is set up each time, after a new program went in.
        let swap = launcher.find("MapleSyrup-new.exe").unwrap();
        let setup = launcher.find("--setup-claude").unwrap();
        let claude = launcher.find("--dangerously-load").unwrap();
        assert!(swap < setup && setup < claude);
        assert!(!launcher.contains("if exist \"%~dp0.mcp.json\" goto :running\r\necho Setting"));
        assert!(launcher.contains("\r\n"));
    }

    #[test]
    fn a_path_as_permission_rules_write_it() {
        assert_eq!(rule_path(Path::new(r"C:\Users\מיכאל")), "//c/Users/מיכאל");
        assert_eq!(rule_path(Path::new(r"D:\Games\")), "//d/Games");
        assert_eq!(rule_path(Path::new("/home/michael")), "//home/michael");
    }

    #[test]
    fn the_style_makes_it_claude_for_anything_and_is_the_one_chosen() {
        let front = STYLE.split("---").nth(1).unwrap();
        assert!(front.contains(&format!("name: {STYLE_NAME}\n")), "{front}");
        assert!(front.contains("keep-coding-instructions: true"));
        assert!(STYLE.contains("never steer him back to the game"));
        assert!(STYLE.contains("WebSearch"));
        assert!(STYLE.contains("Documents\\Claude projects"));
        assert!(
            PROJECTS
                .replace('/', "\\")
                .ends_with("Documents\\Claude projects"),
            "the style and the settings name the same folder"
        );
    }

    #[test]
    fn maplesyrup_is_added_to_the_desktop_config_keeping_the_rest() {
        let dir = std::env::temp_dir().join(format!(
            "ms-claude-desktop-{}",
            crate::phone::tls::random_hex(4)
        ));
        let config = dir.join("claude_desktop_config.json");
        let exe = Path::new(r"C:\Users\מיכאל\Desktop\MapleSyrup\MapleSyrup.exe");
        // No file yet: made.
        assert_eq!(add_to_desktop_config(&config, exe), Ok(true));
        let made: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(
            made["mcpServers"]["maplesyrup"]["command"],
            r"C:\Users\מיכאל\Desktop\MapleSyrup\MapleSyrup.exe"
        );
        assert_eq!(made["mcpServers"]["maplesyrup"]["args"][0], "--mcp");
        // Again: nothing to change.
        assert_eq!(add_to_desktop_config(&config, exe), Ok(false));
        // Someone else's servers and settings stay, and the file as it was
        // is kept beside it.
        fs::write(
            &config,
            r#"{"globalShortcut": "Ctrl+Space", "mcpServers": {"filesystem": {"command": "npx", "args": ["x"]}}}"#,
        )
        .unwrap();
        assert_eq!(add_to_desktop_config(&config, exe), Ok(true));
        let merged: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(merged["globalShortcut"], "Ctrl+Space");
        assert_eq!(merged["mcpServers"]["filesystem"]["command"], "npx");
        assert_eq!(merged["mcpServers"]["maplesyrup"]["args"][0], "--mcp");
        assert!(config.with_extension("json.before-maplesyrup").exists());
        // One an editor saved with a byte-order mark is read all the same.
        fs::write(&config, "\u{feff}{\"mcpServers\": {}}").unwrap();
        assert_eq!(add_to_desktop_config(&config, exe), Ok(true));
        let read: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(read["mcpServers"]["maplesyrup"]["args"][0], "--mcp");
        // A broken file is not touched.
        fs::write(&config, "{ broken").unwrap();
        assert!(add_to_desktop_config(&config, exe).is_err());
        assert_eq!(fs::read_to_string(&config).unwrap(), "{ broken");
        assert!(installer_cmd().contains("--install-claude"));
        assert!(installer_cmd().is_ascii());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_brief_tells_claude_to_talk_through_say_and_keeps_the_players_own() {
        assert!(BRIEF.contains("`say`"));
        assert!(BRIEF.contains("Classic World"));
        assert!(
            BRIEF.contains("It can be about\n  anything, not only the game"),
            "what he says isn't only about the game"
        );
        assert!(ours(BRIEF), "the brief carries its marker");
        // MapleSyrup's file about him goes at its end; nothing, nothing.
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
        let home = Path::new("/home/michael");
        let written = write(&dir, exe, Some(about), Some(home)).unwrap();
        assert_eq!(written.len(), 5);
        assert!(dir.join(".claude/settings.json").exists());
        assert_eq!(fs::read_to_string(dir.join(STYLE_FILE)).unwrap(), STYLE);
        assert!(
            fs::read_to_string(dir.join("CLAUDE.md"))
                .unwrap()
                .contains("Michael (מיכאל)")
        );
        // Started again, with more in his file: the brief keeps up (it is
        // MapleSyrup's); the launcher is left as it is.
        let more = format!("{about}\n- Wants level 30 this week.");
        let again = write(&dir, exe, Some(&more), Some(home)).unwrap();
        assert_eq!(again.len(), 4);
        assert!(
            fs::read_to_string(dir.join("CLAUDE.md"))
                .unwrap()
                .contains("level 30")
        );
        // The brief of a version before the marker is MapleSyrup's too.
        fs::write(
            dir.join("CLAUDE.md"),
            "# You are his MapleStory companion\n\nOld words.\n",
        )
        .unwrap();
        write(&dir, exe, Some(about), Some(home)).unwrap();
        assert!(ours(&fs::read_to_string(dir.join("CLAUDE.md")).unwrap()));
        // One he made his own is his.
        fs::write(dir.join("CLAUDE.md"), "my own brief").unwrap();
        let mine = write(&dir, exe, Some(about), Some(home)).unwrap();
        assert_eq!(mine.len(), 3, "his brief is his");
        assert_eq!(
            fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
            "my own brief"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
