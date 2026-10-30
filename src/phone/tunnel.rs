//! A Cloudflare quick tunnel, as thelip-server (syrup) opens one: an
//! `https://<random>.trycloudflare.com` address that reaches the phone link
//! from anywhere, with a certificate the phone already trusts — no warning
//! to click through, no firewall prompt, and the phone need not be on the
//! same Wi-Fi. It needs `cloudflared`, which is fetched from Cloudflare's
//! GitHub releases when it is not already on this PC.
//!
//! cloudflared prints the address before it works ("it may take some time
//! to be reachable"). A phone or a PC that asks for the name too early is
//! told it does not exist, and its DNS keeps that answer for minutes: the
//! link stays dead ("This site can't be reached", `NXDOMAIN`) long after
//! the tunnel is up. So [`open`] returns only once the tunnel is connected
//! and answers at its address — found through Cloudflare's DNS over HTTPS,
//! not this PC's DNS, so that the early "no such name" is never kept here —
//! or once it has waited [`READY_WITHIN`].

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// cloudflared's line once the tunnel is connected to Cloudflare.
const CONNECTED: &str = "Registered tunnel connection";
/// Cloudflare's DNS over HTTPS, through which the new address is looked
/// for while it is being set up.
const DOH: &str = "https://cloudflare-dns.com/dns-query";
/// How long to wait, at most, for a new tunnel to work before its link is
/// given anyway.
pub const READY_WITHIN: Duration = Duration::from_secs(20);
/// Between two looks at the address.
const LOOK_EVERY: Duration = Duration::from_millis(1500);
/// One look, at most.
const LOOK_FOR: Duration = Duration::from_secs(5);
/// With a curl that can't ask DNS over HTTPS (an old Windows 10): how long
/// after the tunnel connected to give its link.
const UNCHECKED_WAIT: Duration = Duration::from_secs(8);

pub struct Tunnel {
    child: Child,
    /// The public address, e.g. `https://words-words.trycloudflare.com`.
    pub url: String,
    /// Whether it was seen to answer at its address before its link was
    /// given: `Some(false)` not within [`READY_WITHIN`] (the link may take
    /// a little longer), `None` it could not be checked.
    pub answered: Option<bool>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    }
}

/// Where a cloudflared may already be: next to MapleSyrup, on the PATH,
/// where thelip-server keeps its own, or where MapleSyrup saved one before.
pub fn find(settings: &Path) -> Option<PathBuf> {
    let name = binary_name();
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join(name));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(name)));
    }
    if cfg!(windows) {
        candidates.push(PathBuf::from(r"C:\Users\Public\thelip-server").join(name));
    }
    candidates.push(settings.join(name));
    candidates.into_iter().find(|p| p.is_file())
}

/// Download cloudflared into `settings` (Windows and Linux on x86-64).
pub fn download(settings: &Path) -> Result<PathBuf, String> {
    let asset = if cfg!(windows) {
        "cloudflared-windows-amd64.exe"
    } else if cfg!(target_os = "linux") {
        "cloudflared-linux-amd64"
    } else {
        return Err("install cloudflared (on macOS: brew install cloudflared)".into());
    };
    std::fs::create_dir_all(settings).map_err(|e| e.to_string())?;
    let dest = settings.join(binary_name());
    let partial_name = format!("{}.partial", binary_name());
    let partial = settings.join(&partial_name);
    let url = format!("https://github.com/cloudflare/cloudflared/releases/latest/download/{asset}");
    // curl ships with Windows 10 and later. It runs in the folder and is
    // given the file's name only: Windows' curl can't take a path with
    // letters outside the PC's code page (a user folder in Hebrew, say).
    let status = Command::new("curl")
        .current_dir(settings)
        .args(["-L", "--fail", "--silent", "--show-error", "-o"])
        .arg(&partial_name)
        .arg(&url)
        .status()
        .map_err(|e| format!("could not run curl to download cloudflared: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("downloading {url} failed"));
    }
    std::fs::rename(&partial, &dest).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(dest)
}

/// The quick tunnel's address in a line of cloudflared's log.
pub fn address_in(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '|' || c == '"')
        .unwrap_or(rest.len());
    let url = &rest[..end];
    url.ends_with(".trycloudflare.com").then(|| url.to_string())
}

/// Whether a line of cloudflared's log says the tunnel is connected.
pub fn connected_in(line: &str) -> bool {
    line.contains(CONNECTED)
}

/// Open a quick tunnel to `http://127.0.0.1:<port>`. cloudflared's log goes
/// to `log`. Waits up to a minute for the address, then until the tunnel
/// answers at it (up to [`READY_WITHIN`]).
pub fn open(binary: &Path, port: u16, log: &Path) -> Result<Tunnel, String> {
    let mut child = Command::new(binary)
        .args(["tunnel", "--no-autoupdate", "--url"])
        .arg(format!("http://127.0.0.1:{port}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", binary.display()))?;
    let stderr = child.stderr.take().ok_or("cloudflared has no log")?;
    let (found, address) = mpsc::channel();
    let (up, connected) = mpsc::channel();
    let log = log.to_path_buf();
    std::thread::spawn(move || {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .ok();
        let (mut sent, mut told) = (false, false);
        // Keep reading after the address is found, or cloudflared blocks
        // on a full pipe.
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(f) = file.as_mut() {
                let _ = writeln!(f, "{line}");
            }
            if !sent && let Some(url) = address_in(&line) {
                let _ = found.send(url);
                sent = true;
            }
            if !told && connected_in(&line) {
                let _ = up.send(());
                told = true;
            }
        }
    });
    match address.recv_timeout(Duration::from_secs(60)) {
        Ok(url) => {
            let answered = wait_until_it_answers(&connected, &url);
            Ok(Tunnel {
                child,
                url,
                answered,
            })
        }
        Err(_) => {
            let _ = child.kill();
            Err("cloudflared did not give an address within a minute".into())
        }
    }
}

/// Wait, up to [`READY_WITHIN`], for the tunnel to connect and then to
/// answer at `url`: `Some(true)` it does, `Some(false)` not in time,
/// `None` it could not be checked (it waited [`UNCHECKED_WAIT`] after the
/// tunnel connected instead).
fn wait_until_it_answers(connected: &mpsc::Receiver<()>, url: &str) -> Option<bool> {
    let deadline = Instant::now() + READY_WITHIN;
    if connected.recv_timeout(READY_WITHIN).is_err() {
        return Some(false);
    }
    loop {
        match answers(url) {
            Some(true) => return Some(true),
            None => {
                std::thread::sleep(UNCHECKED_WAIT);
                return None;
            }
            Some(false) => {}
        }
        if Instant::now() + LOOK_EVERY >= deadline {
            return Some(false);
        }
        std::thread::sleep(LOOK_EVERY);
    }
}

/// Whether MapleSyrup answers at `url` through the tunnel, its name found
/// through Cloudflare's DNS over HTTPS (curl's `--doh-url`), never this
/// PC's: `Some(true)` it does, `Some(false)` not yet, `None` this PC's curl
/// can't look that way.
fn answers(url: &str) -> Option<bool> {
    let mut command = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    command
        .args(["--silent", "--max-time"])
        .arg(LOOK_FOR.as_secs().to_string())
        .args(["--proto", "=https", "--doh-url", DOH])
        .arg(format!("{url}{}", super::HEALTH_PATH))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window of its own.
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    answered(output.status.code(), &output.stdout)
}

/// What a look with curl found, from its exit code and what it printed:
/// curl's own codes for an option or a feature it doesn't have (2, 4) say
/// it can't look this way.
fn answered(code: Option<i32>, body: &[u8]) -> Option<bool> {
    match code {
        Some(2) | Some(4) => None,
        _ => Some(String::from_utf8_lossy(body).trim() == super::HEALTH),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_address_in_cloudflareds_banner() {
        let line = "2026-10-02T10:00:00Z INF |  https://quiet-maple-words-here.trycloudflare.com                          |";
        assert_eq!(
            address_in(line).as_deref(),
            Some("https://quiet-maple-words-here.trycloudflare.com")
        );
        assert_eq!(
            address_in("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
        assert_eq!(address_in("see https://developers.cloudflare.com/x"), None);
    }

    #[test]
    fn sees_the_tunnel_connect_and_answer() {
        assert!(connected_in(
            "2026-10-29T15:27:10Z INF Registered tunnel connection connIndex=0 connection=1f2e event=0 ip=2606:4700:a8::9 location=fra15 protocol=quic"
        ));
        assert!(!connected_in(
            "2026-10-29T15:27:09Z INF Requesting new quick Tunnel on trycloudflare.com..."
        ));
        // MapleSyrup's own answer through the tunnel; anything else (a
        // Cloudflare error page, nothing yet) is not it; a curl that can't
        // look this way is not a "no".
        assert_eq!(
            answered(Some(0), crate::phone::HEALTH.as_bytes()),
            Some(true)
        );
        assert_eq!(answered(Some(0), b"error code: 1033"), Some(false));
        assert_eq!(answered(Some(6), b""), Some(false));
        assert_eq!(answered(Some(2), b""), None);
        assert_eq!(answered(Some(4), b""), None);
        assert_eq!(answered(None, b""), Some(false));
    }
}
