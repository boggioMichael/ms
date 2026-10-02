//! A Cloudflare quick tunnel, as thelip-server (syrup) opens one: an
//! `https://<random>.trycloudflare.com` address that reaches the phone link
//! from anywhere, with a certificate the phone already trusts — no warning
//! to click through, no firewall prompt, and the phone need not be on the
//! same Wi-Fi. It needs `cloudflared`, which is fetched from Cloudflare's
//! GitHub releases when it is not already on this PC.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub struct Tunnel {
    child: Child,
    /// The public address, e.g. `https://words-words.trycloudflare.com`.
    pub url: String,
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
    let partial = settings.join(format!("{}.partial", binary_name()));
    let url = format!("https://github.com/cloudflare/cloudflared/releases/latest/download/{asset}");
    // curl ships with Windows 10 and later.
    let status = Command::new("curl")
        .args(["-L", "--fail", "--silent", "--show-error", "-o"])
        .arg(&partial)
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

/// Open a quick tunnel to `http://127.0.0.1:<port>`. cloudflared's log goes
/// to `log`. Waits up to a minute for the address.
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
    let log = log.to_path_buf();
    std::thread::spawn(move || {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .ok();
        let mut sent = false;
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
        }
    });
    match address.recv_timeout(Duration::from_secs(60)) {
        Ok(url) => Ok(Tunnel { child, url }),
        Err(_) => {
            let _ = child.kill();
            Err("cloudflared did not give an address within a minute".into())
        }
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
}
