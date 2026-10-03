//! MapleSyrup updates itself the way Android updates its APEX modules: a
//! release is published as a signed manifest beside its files; the running
//! program finds it, fetches the new program in the background, checks the
//! signature and the hash, stages it, and activates it on the next start —
//! atomically, with the previous program kept beside it to go back to if
//! the new one does not come up.
//!
//! The pieces:
//! - the **channel**: `manifest.json` and `manifest.json.sig` at a fixed
//!   address ([`CHANNEL`]); the manifest names the version, the files, their
//!   sizes and SHA-256 hashes and where to fetch them; the signature is
//!   Ed25519 over the manifest's bytes, by the key the release pipeline
//!   holds, checked against [`PUBLIC_KEY`] built into the program. A
//!   manifest that does not verify is ignored, whatever it says.
//! - the **checker** ([`Updater`]): a thread that looks at the channel on a
//!   cadence (and when asked), fetches a newer program into the settings
//!   folder, verifies it and **stages** it (`updates/staged.json`).
//! - **activation** ([`at_start`]): the next start finds the staged program,
//!   keeps the running one as `MapleSyrup.old.exe`, puts the new one in its
//!   place with renames alone, starts it and leaves. Nothing is deleted
//!   until the new program has run long enough to be **committed**
//!   ([`commit`]). If it fails to come up twice, the next start **rolls
//!   back** to the kept program and never offers that version again.
//!
//! Everything the updater keeps lives under `<settings>/updates/`; what it
//! does is written to `updates/log.txt` as well as the session log.

use ring::digest::{Context, SHA256};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Where releases are announced: the manifest, with its signature beside
/// it as `manifest.json.sig`. `MAPLESYRUP_UPDATE_URL` overrides it (tests, a
/// mirror).
pub const CHANNEL: &str = "https://datta-syrup.ai/downloads/manifest.json";

/// The release pipeline's Ed25519 public key (raw, 32 bytes). The private
/// half is a secret of the repository's CI and nowhere else; a manifest not
/// signed by it is not an update.
pub const PUBLIC_KEY: [u8; 32] = [
    0xa3, 0x0a, 0xd1, 0x00, 0x8d, 0x14, 0xb1, 0xd0, 0xcc, 0x8a, 0xc4, 0xd9, 0x59, 0x05, 0xa6, 0xa7,
    0xc7, 0x15, 0xe5, 0x5a, 0x8b, 0xd6, 0x55, 0x62, 0x6b, 0xcb, 0x9e, 0x4d, 0x23, 0xc0, 0x28, 0x64,
];

/// How long after the start the channel is first looked at (the start has
/// enough to do), and how often after that.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(45);
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// After a check that failed (no network, a bad answer), the next one waits
/// this long, doubling each time up to `CHECK_EVERY`.
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);
/// A manifest larger than this is not one.
const MAX_MANIFEST: u64 = 256 * 1024;
/// A program larger than this is not ours.
const MAX_PROGRAM: u64 = 200 * 1024 * 1024;
/// How many starts a new version gets to come up and be committed before
/// the start after them goes back to the kept program.
pub const BOOTS_BEFORE_ROLLBACK: u32 = 2;
/// How long the program must have run, watching the game or talking to a
/// phone, before a new version counts as come up.
pub const HEALTHY_AFTER: Duration = Duration::from_secs(90);

/// A release, as the channel announces it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// When it was published (`YYYY-MM-DD`).
    #[serde(default)]
    pub published: String,
    /// The commit it was built from.
    #[serde(default)]
    pub commit: String,
    /// A line or two about what is new.
    #[serde(default)]
    pub notes: String,
    pub files: Vec<File>,
}

/// One file of a release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct File {
    pub name: String,
    /// `exe` (the program alone, what the updater fetches), `portable` (the
    /// zip) or `installer`.
    pub kind: String,
    pub size: u64,
    /// SHA-256 of the file, hex.
    pub sha256: String,
    pub url: String,
}

impl Manifest {
    /// The program itself, for the updater.
    pub fn program(&self) -> Option<&File> {
        self.files.iter().find(|f| f.kind == "exe")
    }
}

/// A version, `MAJOR.MINOR.PATCH` with an optional `-pre` (which comes
/// before the release of the same number); build metadata (`+…`) ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split('+').next()?;
        let (numbers, pre) = match text.split_once('-') {
            Some((n, p)) => (n, Some(p.to_string())),
            None => (text, None),
        };
        let mut parts = numbers.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // A pre-release comes before the release.
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

/// The manifest in `bytes`, if `signature` is the signature of exactly
/// those bytes by the key `public_key` is the public half of.
pub fn verify(bytes: &[u8], signature: &[u8], public_key: &[u8]) -> Result<Manifest, String> {
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(bytes, signature)
        .map_err(|_| "the manifest's signature is not the release key's".to_string())?;
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|e| format!("the manifest is not readable: {e}"))?;
    Version::parse(&manifest.version)
        .ok_or_else(|| format!("the manifest's version {:?} is not one", manifest.version))?;
    Ok(manifest)
}

/// SHA-256 of a file, hex.
pub fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut context = Context::new(&SHA256);
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        context.update(&buffer[..n]);
    }
    Ok(hex(context.finish().as_ref()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `path` is the program `file` describes: the size, the hash, and
/// a Windows program's first two bytes.
pub fn accept_program(path: &Path, file: &File) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("the download is missing: {e}"))?;
    if meta.len() != file.size {
        return Err(format!(
            "the download is {} bytes, the manifest says {}",
            meta.len(),
            file.size
        ));
    }
    let hash = sha256_of(path).map_err(|e| format!("the download could not be read: {e}"))?;
    if !hash.eq_ignore_ascii_case(&file.sha256) {
        return Err("the download's hash is not the manifest's".to_string());
    }
    let mut head = [0u8; 2];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| format!("the download could not be read: {e}"))?;
    if &head != b"MZ" {
        return Err("the download is not a Windows program".to_string());
    }
    Ok(())
}

/// A program fetched and verified, waiting for the next start.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Staged {
    pub version: String,
    /// The program, under the updates folder.
    pub file: PathBuf,
    pub sha256: String,
    pub size: u64,
    /// The version it was fetched by.
    pub from: String,
    pub when: String,
    #[serde(default)]
    pub notes: String,
}

/// A new version activated and not yet committed: the start counts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pending {
    pub from: String,
    pub to: String,
    /// The kept previous program.
    pub old: PathBuf,
    /// How many times the new version has been started.
    pub boots: u32,
    pub since: String,
}

/// The updater's files under the settings folder.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(settings: &Path) -> Store {
        Store {
            dir: settings.join("updates"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn read<T: for<'a> Deserialize<'a>>(&self, name: &str) -> Option<T> {
        let text = fs::read(self.dir.join(name)).ok()?;
        serde_json::from_slice(&text).ok()
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(format!("{name}.tmp"));
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, self.dir.join(name)).map_err(|e| e.to_string())
    }

    fn remove(&self, name: &str) {
        let _ = fs::remove_file(self.dir.join(name));
    }

    pub fn staged(&self) -> Option<Staged> {
        self.read("staged.json")
    }

    pub fn set_staged(&self, staged: &Staged) -> Result<(), String> {
        self.write("staged.json", staged)
    }

    pub fn clear_staged(&self) {
        if let Some(staged) = self.staged() {
            let _ = fs::remove_file(&staged.file);
        }
        self.remove("staged.json");
    }

    pub fn pending(&self) -> Option<Pending> {
        self.read("pending.json")
    }

    pub fn set_pending(&self, pending: &Pending) -> Result<(), String> {
        self.write("pending.json", pending)
    }

    pub fn clear_pending(&self) {
        self.remove("pending.json");
    }

    /// Versions that were activated and did not come up: never again.
    pub fn blocked(&self) -> Vec<String> {
        self.read("blocked.json").unwrap_or_default()
    }

    pub fn block(&self, version: &str) {
        let mut blocked = self.blocked();
        if !blocked.iter().any(|v| v == version) {
            blocked.push(version.to_string());
            let _ = self.write("blocked.json", &blocked);
        }
    }

    /// A line in `updates/log.txt`, dated.
    pub fn log(&self, line: &str) {
        let _ = fs::create_dir_all(&self.dir);
        if let Ok(mut file) = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.dir.join("log.txt"))
        {
            let _ = writeln!(file, "{} {line}", now_text());
        }
    }
}

fn now_text() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// What the updater has to say: to the player (a new version fetched, a
/// version ready) or for the log alone (a look that failed; the channel may
/// simply have nothing published yet).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Said(String),
    Noted(String),
}

/// What the start of the program should do.
#[derive(Debug, PartialEq)]
pub enum Start {
    /// Nothing to activate: run as usual.
    CarryOn,
    /// A program was put in place (a new version, or the kept one back):
    /// start it and leave.
    Relaunch(PathBuf),
}

/// Beside the program: the kept previous one, and the new one on its way in.
fn beside(exe: &Path, suffix: &str) -> PathBuf {
    let stem = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("MapleSyrup");
    let ext = exe.extension().and_then(|s| s.to_str()).unwrap_or("exe");
    exe.with_file_name(format!("{stem}.{suffix}.{ext}"))
}

/// Put `staged` in the running program's place: the new program copied
/// beside it first, then two renames (the running program can be renamed
/// while it runs, and a rename on one volume is atomic), so at no moment is
/// there no program. The previous one is kept as `MapleSyrup.old.exe` until
/// the new one is committed.
fn activate(store: &Store, exe: &Path, running: &str, staged: &Staged) -> Result<(), String> {
    accept_program(
        &staged.file,
        &File {
            name: String::new(),
            kind: "exe".into(),
            size: staged.size,
            sha256: staged.sha256.clone(),
            url: String::new(),
        },
    )?;
    let new = beside(exe, "new");
    let old = beside(exe, "old");
    fs::copy(&staged.file, &new)
        .map_err(|e| format!("could not copy the new program beside this one: {e}"))?;
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| {
        let _ = fs::remove_file(&new);
        format!("could not set the running program aside: {e}")
    })?;
    if let Err(e) = fs::rename(&new, exe) {
        // Back as it was.
        let _ = fs::rename(&old, exe);
        let _ = fs::remove_file(&new);
        return Err(format!("could not put the new program in place: {e}"));
    }
    store.set_pending(&Pending {
        from: running.to_string(),
        to: staged.version.clone(),
        old: old.clone(),
        boots: 0,
        since: now_text(),
    })?;
    store.clear_staged();
    store.log(&format!(
        "activated {} (was {running}); the previous program kept as {}",
        staged.version,
        old.display()
    ));
    Ok(())
}

/// The first thing the program does: count this start of a version not yet
/// committed and roll back when it has had its chances; activate a staged
/// program. `exe` is the running program, `running` its version.
pub fn at_start(settings: &Path, exe: &Path, running: &str) -> Start {
    let store = Store::new(settings);
    let Some(running_version) = Version::parse(running) else {
        return Start::CarryOn;
    };
    if let Some(mut pending) = store.pending() {
        if pending.to == running {
            pending.boots += 1;
            if pending.boots > BOOTS_BEFORE_ROLLBACK && pending.old.exists() {
                // It had its chances: back to the kept program.
                let bad = beside(exe, "failed");
                let _ = fs::remove_file(&bad);
                match fs::rename(exe, &bad).and_then(|_| fs::rename(&pending.old, exe)) {
                    Ok(()) => {
                        store.block(&pending.to);
                        store.clear_pending();
                        store.clear_staged();
                        store.log(&format!(
                            "{} did not come up in {} starts: back to {}",
                            pending.to, BOOTS_BEFORE_ROLLBACK, pending.from
                        ));
                        let _ = fs::remove_file(&bad);
                        return Start::Relaunch(exe.to_path_buf());
                    }
                    Err(e) => {
                        // Whatever was moved, moved back.
                        if !exe.exists() {
                            let _ = fs::rename(&bad, exe);
                        }
                        store.log(&format!("could not go back to {}: {e}", pending.from));
                    }
                }
            }
            let _ = store.set_pending(&pending);
        } else {
            // Another program than the one activated is running (put there
            // by hand): nothing to watch over.
            let _ = fs::remove_file(&pending.old);
            store.clear_pending();
        }
    }
    if let Some(staged) = store.staged() {
        match Version::parse(&staged.version) {
            Some(version)
                if version > running_version && !store.blocked().contains(&staged.version) =>
            {
                match activate(&store, exe, running, &staged) {
                    Ok(()) => return Start::Relaunch(exe.to_path_buf()),
                    Err(e) => {
                        store.log(&format!("could not activate {}: {e}", staged.version));
                        store.clear_staged();
                    }
                }
            }
            _ => store.clear_staged(),
        }
    }
    Start::CarryOn
}

/// The new version has run long enough: the kept program goes, and the
/// start no longer counts. Returns what was committed, for the log.
pub fn commit(settings: &Path) -> Option<String> {
    let store = Store::new(settings);
    let pending = store.pending()?;
    let _ = fs::remove_file(&pending.old);
    store.clear_pending();
    store.log(&format!("{} committed", pending.to));
    Some(pending.to)
}

/// Start `exe` with `args` as a program of its own and let it go. On
/// Windows it gets a console window of its own, since this one closes with
/// this process: `CreateProcessW` with `CREATE_NEW_CONSOLE` and no standard
/// handles handed down (the standard library always hands this process's
/// down, which would tie the new program to the closing window).
#[cfg(windows)]
pub fn relaunch(exe: &Path, args: &[String]) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        CREATE_NEW_CONSOLE, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };
    use windows::core::{PCWSTR, PWSTR};

    let mut line = quote_argument(&exe.to_string_lossy());
    for arg in args {
        line.push(' ');
        line.push_str(&quote_argument(arg));
    }
    let mut wide: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: the command line is a writable, NUL-terminated buffer that
    // outlives the call; the structures are initialised; the handles
    // returned are closed below.
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(wide.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NEW_CONSOLE,
            None,
            PCWSTR::null(),
            &startup,
            &mut process,
        )
        .map_err(|e| format!("could not start {}: {e}", exe.display()))?;
        let _ = CloseHandle(process.hProcess);
        let _ = CloseHandle(process.hThread);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn relaunch(exe: &Path, args: &[String]) -> Result<(), String> {
    Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start {}: {e}", exe.display()))
}

/// `arg` as one argument of a Windows command line (the rules
/// `CommandLineToArgvW` reads by).
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_argument(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| c == ' ' || c == '\t' || c == '"') {
        return arg.to_string();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Where the updater stands, for the phone and the log.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Not looking (asked not to, or no way to).
    Off,
    /// Nothing newer than what runs (`checked`: when it last looked).
    UpToDate,
    /// A newer version is on its way in.
    Downloading(String),
    /// A newer version is fetched and verified: it installs at the next start.
    Staged(String),
    /// A newer version is announced but was blocked (it did not come up
    /// before) — the one after it will do.
    Blocked(String),
    /// The last look failed (`why`); it tries again later.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Status {
    pub running: String,
    pub phase: Phase,
    /// Whether it looks on its own.
    pub auto: bool,
    pub last_check: Option<Instant>,
    /// The newest version announced, when known.
    pub latest: Option<String>,
    pub notes: String,
}

impl Status {
    pub fn to_json(&self) -> serde_json::Value {
        let (state, detail) = match &self.phase {
            Phase::Off => ("off", String::new()),
            Phase::UpToDate => ("up-to-date", String::new()),
            Phase::Downloading(v) => ("downloading", v.clone()),
            Phase::Staged(v) => ("staged", v.clone()),
            Phase::Blocked(v) => ("blocked", v.clone()),
            Phase::Failed(why) => ("failed", why.clone()),
        };
        serde_json::json!({
            "version": self.running,
            "state": state,
            "detail": detail,
            "latest": self.latest,
            "auto": self.auto,
            "checked_secs_ago": self.last_check.map(|t| t.elapsed().as_secs()),
            "notes": self.notes,
        })
    }
}

/// What a verified manifest means for this program.
#[derive(Debug, PartialEq)]
pub enum Plan {
    UpToDate,
    Blocked(String),
    AlreadyStaged(String),
    Fetch(File),
}

/// The checker.
pub struct Updater {
    store: Store,
    running: Version,
    running_text: String,
    channel: String,
    public_key: Vec<u8>,
    curl: String,
    status: Mutex<Status>,
    /// Woken to look now.
    wake: (Mutex<bool>, Condvar),
    /// What it did, for the session log and the player.
    log: Mutex<Option<mpsc::Sender<Event>>>,
}

impl Updater {
    /// `running`: this program's version. The channel is [`CHANNEL`] unless
    /// `MAPLESYRUP_UPDATE_URL` says otherwise.
    pub fn new(settings: &Path, running: &str, auto: bool) -> Updater {
        let channel =
            std::env::var("MAPLESYRUP_UPDATE_URL").unwrap_or_else(|_| CHANNEL.to_string());
        Updater::with(settings, running, auto, &channel, &PUBLIC_KEY)
    }

    pub fn with(
        settings: &Path,
        running: &str,
        auto: bool,
        channel: &str,
        public_key: &[u8],
    ) -> Updater {
        let store = Store::new(settings);
        let staged = store.staged();
        let phase = if !auto {
            Phase::Off
        } else if let Some(staged) = &staged {
            Phase::Staged(staged.version.clone())
        } else {
            Phase::UpToDate
        };
        Updater {
            store,
            running: Version::parse(running).unwrap_or(Version {
                major: 0,
                minor: 0,
                patch: 0,
                pre: None,
            }),
            running_text: running.to_string(),
            channel: channel.to_string(),
            public_key: public_key.to_vec(),
            curl: if cfg!(windows) {
                "curl.exe".into()
            } else {
                "curl".into()
            },
            status: Mutex::new(Status {
                running: running.to_string(),
                phase,
                auto,
                last_check: None,
                latest: staged.as_ref().map(|s| s.version.clone()),
                notes: staged.map(|s| s.notes).unwrap_or_default(),
            }),
            wake: (Mutex::new(false), Condvar::new()),
            log: Mutex::new(None),
        }
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    fn set_phase(&self, phase: Phase) {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).phase = phase;
    }

    /// Whether it looks on its own.
    pub fn set_auto(&self, auto: bool) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        status.auto = auto;
        if !auto {
            status.phase = Phase::Off;
        } else if status.phase == Phase::Off {
            status.phase = Phase::UpToDate;
        }
        drop(status);
        if auto {
            self.check_now();
        }
    }

    /// Look at the channel now (from the phone, or when turned on).
    pub fn check_now(&self) {
        let (flag, condvar) = &self.wake;
        *flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
        condvar.notify_all();
    }

    /// A line for the player (and the logs).
    fn say(&self, line: String) {
        self.store.log(&line);
        if let Some(tx) = self.log.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(Event::Said(line));
        }
    }

    /// A line for the logs alone.
    fn note(&self, line: String) {
        self.store.log(&line);
        if let Some(tx) = self.log.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(Event::Noted(line));
        }
    }

    /// The checker's thread: a look after the start, then every hour, or
    /// when woken; looks on its own only while `auto`.
    pub fn spawn(self: Arc<Self>, log: mpsc::Sender<Event>) {
        *self.log.lock().unwrap_or_else(|e| e.into_inner()) = Some(log);
        let _ = std::thread::Builder::new()
            .name("updates".into())
            .spawn(move || {
                let mut wait = FIRST_CHECK_AFTER;
                let mut retry = RETRY_AFTER;
                loop {
                    let woken = {
                        let (flag, condvar) = &self.wake;
                        let guard = flag.lock().unwrap_or_else(|e| e.into_inner());
                        let (mut guard, _) = condvar
                            .wait_timeout_while(guard, wait, |woken| !*woken)
                            .unwrap_or_else(|e| e.into_inner());
                        std::mem::take(&mut *guard)
                    };
                    let auto = self.status().auto;
                    if !auto && !woken {
                        wait = CHECK_EVERY;
                        continue;
                    }
                    match self.check_once() {
                        Ok(()) => {
                            wait = CHECK_EVERY;
                            retry = RETRY_AFTER;
                        }
                        Err(why) => {
                            self.set_phase(Phase::Failed(why.clone()));
                            self.note(format!("update check failed: {why}"));
                            wait = retry;
                            retry = (retry * 2).min(CHECK_EVERY);
                        }
                    }
                }
            });
    }

    /// One look at the channel, fetching and staging what it announces.
    pub fn check_once(&self) -> Result<(), String> {
        let bytes = self.fetch(&self.channel, MAX_MANIFEST)?;
        let signature = self.fetch(&format!("{}.sig", self.channel), 4096)?;
        let manifest = verify(&bytes, &signature, &self.public_key)?;
        {
            let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
            status.last_check = Some(Instant::now());
            status.latest = Some(manifest.version.clone());
            status.notes = manifest.notes.clone();
        }
        match self.plan(&manifest) {
            Plan::UpToDate => {
                self.set_phase(Phase::UpToDate);
                Ok(())
            }
            Plan::Blocked(v) => {
                self.set_phase(Phase::Blocked(v));
                Ok(())
            }
            Plan::AlreadyStaged(v) => {
                self.set_phase(Phase::Staged(v));
                Ok(())
            }
            Plan::Fetch(file) => {
                self.set_phase(Phase::Downloading(manifest.version.clone()));
                self.say(format!(
                    "{} {} is out (this is {}): fetching it",
                    manifest.name, manifest.version, self.running_text
                ));
                let staged = self.fetch_program(&manifest, &file)?;
                self.store.set_staged(&staged)?;
                self.set_phase(Phase::Staged(staged.version.clone()));
                self.say(format!(
                    "{} {} is ready: it installs the next time MapleSyrup starts",
                    manifest.name, manifest.version
                ));
                Ok(())
            }
        }
    }

    /// What `manifest` means for this program.
    pub fn plan(&self, manifest: &Manifest) -> Plan {
        let Some(version) = Version::parse(&manifest.version) else {
            return Plan::UpToDate;
        };
        if version <= self.running {
            // Whatever was staged for an older program is stale.
            if self.store.staged().is_some() {
                self.store.clear_staged();
            }
            return Plan::UpToDate;
        }
        if self.store.blocked().contains(&manifest.version) {
            return Plan::Blocked(manifest.version.clone());
        }
        if let Some(staged) = self.store.staged() {
            if staged.version == manifest.version && staged.file.exists() {
                return Plan::AlreadyStaged(staged.version);
            }
            // Something else was staged (an older announcement): replaced.
            self.store.clear_staged();
        }
        match manifest.program() {
            Some(file) => Plan::Fetch(file.clone()),
            None => Plan::UpToDate,
        }
    }

    /// Fetch `file` of `manifest` into the updates folder and verify it.
    fn fetch_program(&self, manifest: &Manifest, file: &File) -> Result<Staged, String> {
        if file.size > MAX_PROGRAM {
            return Err(format!(
                "the program is {} bytes; too big to be ours",
                file.size
            ));
        }
        fs::create_dir_all(self.store.dir()).map_err(|e| e.to_string())?;
        let target = self
            .store
            .dir()
            .join(format!("MapleSyrup-{}.exe", manifest.version));
        let part = target.with_extension("exe.part");
        let _ = fs::remove_file(&part);
        self.fetch_to(&file.url, &part, file.size)?;
        if let Err(why) = accept_program(&part, file) {
            let _ = fs::remove_file(&part);
            return Err(why);
        }
        fs::rename(&part, &target).map_err(|e| e.to_string())?;
        Ok(Staged {
            version: manifest.version.clone(),
            file: target,
            sha256: file.sha256.clone(),
            size: file.size,
            from: self.running_text.clone(),
            when: now_text(),
            notes: manifest.notes.clone(),
        })
    }

    /// A small file over HTTPS, in memory.
    fn fetch(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        let output = self
            .curl(url, max, Duration::from_secs(30))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("curl could not run: {e}"))?;
        if !output.status.success() {
            return Err(curl_failure(&output.stderr, url));
        }
        Ok(output.stdout)
    }

    /// A large file over HTTPS, to `path`.
    fn fetch_to(&self, url: &str, path: &Path, max: u64) -> Result<(), String> {
        let output = self
            .curl(url, max, Duration::from_secs(15 * 60))
            .arg("-o")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("curl could not run: {e}"))?;
        if !output.status.success() {
            let _ = fs::remove_file(path);
            return Err(curl_failure(&output.stderr, url));
        }
        Ok(())
    }

    fn curl(&self, url: &str, max: u64, timeout: Duration) -> Command {
        let mut command = Command::new(&self.curl);
        command
            .args([
                "--silent",
                "--show-error",
                "--fail",
                "--location",
                "--connect-timeout",
                "8",
                "--max-time",
            ])
            .arg(timeout.as_secs().to_string())
            .arg("--max-filesize")
            .arg(max.to_string())
            // HTTPS only; a file on this PC too, for tests of the whole way
            // (whoever can write files here can replace the program anyway).
            .args(["--proto", "=https,file", "--proto-redir", "=https"])
            .arg(url)
            .stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window of its own.
            command.creation_flags(0x0800_0000);
        }
        command
    }

    /// The program this updater's staged version would be activated as:
    /// put in place now (the phone's "update now"), to be relaunched.
    pub fn install_now(&self, exe: &Path) -> Result<String, String> {
        let staged = self.store.staged().ok_or("nothing is staged")?;
        let version = staged.version.clone();
        activate(&self.store, exe, &self.running_text, &staged)?;
        self.say(format!("{version} put in place: restarting"));
        Ok(version)
    }
}

fn curl_failure(stderr: &[u8], url: &str) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    if text.is_empty() {
        format!("could not fetch {url}")
    } else {
        text.chars().take(200).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ms-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn arguments_are_quoted_the_way_windows_reads_them() {
        assert_eq!(quote_argument("--fps"), "--fps");
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(
            quote_argument("C:\\Program Files\\MapleSyrup.exe"),
            "\"C:\\Program Files\\MapleSyrup.exe\""
        );
        assert_eq!(quote_argument("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_argument("ends\\ with\\"), "\"ends\\ with\\\\\"");
    }

    #[test]
    fn versions_order_as_releases_do() {
        assert!(v("0.9.0") > v("0.8.0"));
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.9.0-rc1") < v("0.9.0"));
        assert!(v("0.9.0-rc1") > v("0.8.9"));
        assert_eq!(v("v0.9.0+abc"), v("0.9.0"));
        assert_eq!(Version::parse("0.9"), None);
        assert_eq!(Version::parse("nine"), None);
        assert_eq!(v("1.2.3-beta").to_string(), "1.2.3-beta");
    }

    fn manifest(version: &str, file: &File) -> Vec<u8> {
        serde_json::to_vec(&Manifest {
            name: "MapleSyrup".into(),
            version: version.into(),
            published: "2026-10-04".into(),
            commit: "abc".into(),
            notes: "a note".into(),
            files: vec![file.clone()],
        })
        .unwrap()
    }

    fn keypair() -> (Ed25519KeyPair, Vec<u8>) {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = pair.public_key().as_ref().to_vec();
        (pair, public)
    }

    fn a_file() -> File {
        File {
            name: "MapleSyrup-0.9.0.exe".into(),
            kind: "exe".into(),
            size: 4,
            sha256: String::new(),
            url: "https://example.invalid/x.exe".into(),
        }
    }

    #[test]
    fn a_manifest_is_believed_only_with_the_release_keys_signature() {
        let (pair, public) = keypair();
        let bytes = manifest("0.9.0", &a_file());
        let signature = pair.sign(&bytes);
        let read = verify(&bytes, signature.as_ref(), &public).expect("signed");
        assert_eq!(read.version, "0.9.0");
        assert_eq!(
            read.program().map(|f| f.name.as_str()),
            Some("MapleSyrup-0.9.0.exe")
        );
        // A byte changed after signing.
        let mut tampered = bytes.clone();
        let at = tampered.iter().position(|&b| b == b'9').unwrap();
        tampered[at] = b'8';
        assert!(verify(&tampered, signature.as_ref(), &public).is_err());
        // Another key.
        let (_, other) = keypair();
        assert!(verify(&bytes, signature.as_ref(), &other).is_err());
        // Signed, but not a manifest.
        let junk = b"{\"version\": \"nine\"}".to_vec();
        assert!(verify(&junk, pair.sign(&junk).as_ref(), &public).is_err());
    }

    /// A program file of a few bytes with the hash the manifest wants.
    fn program(dir: &Path, name: &str, body: &[u8]) -> (PathBuf, File) {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        let file = File {
            name: name.into(),
            kind: "exe".into(),
            size: body.len() as u64,
            sha256: sha256_of(&path).unwrap(),
            url: String::new(),
        };
        (path, file)
    }

    #[test]
    fn a_download_is_accepted_only_whole_and_as_a_program() {
        let dir = temp("accept");
        let (path, file) = program(&dir, "p.exe", b"MZ..new");
        assert_eq!(accept_program(&path, &file), Ok(()));
        let short = File {
            size: 3,
            ..file.clone()
        };
        assert!(accept_program(&path, &short).unwrap_err().contains("bytes"));
        let other = File {
            sha256: "00".repeat(32),
            ..file.clone()
        };
        assert!(accept_program(&path, &other).unwrap_err().contains("hash"));
        let (text, text_file) = program(&dir, "t.exe", b"hello..");
        assert!(
            accept_program(&text, &text_file)
                .unwrap_err()
                .contains("not a Windows program")
        );
    }

    fn stage(store: &Store, version: &str, body: &[u8]) -> Staged {
        let (path, file) = program(store.dir(), &format!("MapleSyrup-{version}.exe"), body);
        let staged = Staged {
            version: version.into(),
            file: path,
            sha256: file.sha256,
            size: file.size,
            from: "0.8.0".into(),
            when: "now".into(),
            notes: String::new(),
        };
        store.set_staged(&staged).unwrap();
        staged
    }

    #[test]
    fn a_staged_program_is_activated_at_the_start_kept_beside_and_committed() {
        let dir = temp("activate");
        let settings = dir.join("settings");
        let store = Store::new(&settings);
        fs::create_dir_all(store.dir()).unwrap();
        let exe = dir.join("MapleSyrup.exe");
        fs::write(&exe, b"MZ..old").unwrap();
        stage(&store, "0.9.0", b"MZ..new");

        // The old program starts: the new one goes in its place.
        assert_eq!(
            at_start(&settings, &exe, "0.8.0"),
            Start::Relaunch(exe.clone())
        );
        assert_eq!(fs::read(&exe).unwrap(), b"MZ..new");
        let old = dir.join("MapleSyrup.old.exe");
        assert_eq!(fs::read(&old).unwrap(), b"MZ..old");
        assert!(store.staged().is_none(), "the staged program is spent");
        let pending = store.pending().expect("pending");
        assert_eq!(
            (pending.from.as_str(), pending.to.as_str(), pending.boots),
            ("0.8.0", "0.9.0", 0)
        );

        // The new one starts: counted, not yet committed.
        assert_eq!(at_start(&settings, &exe, "0.9.0"), Start::CarryOn);
        assert_eq!(store.pending().unwrap().boots, 1);
        assert!(old.exists());
        // It ran well: committed, the kept program gone.
        assert_eq!(commit(&settings).as_deref(), Some("0.9.0"));
        assert!(store.pending().is_none());
        assert!(!old.exists());
        assert_eq!(commit(&settings), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_that_does_not_come_up_is_rolled_back_and_never_offered_again() {
        let dir = temp("rollback");
        let settings = dir.join("settings");
        let store = Store::new(&settings);
        fs::create_dir_all(store.dir()).unwrap();
        let exe = dir.join("MapleSyrup.exe");
        fs::write(&exe, b"MZ..old").unwrap();
        stage(&store, "0.9.0", b"MZ..bad");
        assert_eq!(
            at_start(&settings, &exe, "0.8.0"),
            Start::Relaunch(exe.clone())
        );
        // Two starts of the new one, neither committed.
        assert_eq!(at_start(&settings, &exe, "0.9.0"), Start::CarryOn);
        assert_eq!(at_start(&settings, &exe, "0.9.0"), Start::CarryOn);
        assert_eq!(store.pending().unwrap().boots, 2);
        // The third goes back.
        assert_eq!(
            at_start(&settings, &exe, "0.9.0"),
            Start::Relaunch(exe.clone())
        );
        assert_eq!(fs::read(&exe).unwrap(), b"MZ..old");
        assert!(store.pending().is_none());
        assert_eq!(store.blocked(), vec!["0.9.0".to_string()]);
        assert!(!dir.join("MapleSyrup.old.exe").exists());
        assert!(!dir.join("MapleSyrup.failed.exe").exists());
        // The old program is back and runs as usual; the same version
        // staged again is not activated.
        stage(&store, "0.9.0", b"MZ..bad");
        assert_eq!(at_start(&settings, &exe, "0.8.0"), Start::CarryOn);
        assert_eq!(fs::read(&exe).unwrap(), b"MZ..old");
        assert!(store.staged().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_staged_program_no_newer_than_the_running_one_is_dropped() {
        let dir = temp("stale");
        let settings = dir.join("settings");
        let store = Store::new(&settings);
        fs::create_dir_all(store.dir()).unwrap();
        let exe = dir.join("MapleSyrup.exe");
        fs::write(&exe, b"MZ..now").unwrap();
        let staged = stage(&store, "0.9.0", b"MZ..new");
        assert_eq!(at_start(&settings, &exe, "0.9.0"), Start::CarryOn);
        assert_eq!(fs::read(&exe).unwrap(), b"MZ..now");
        assert!(store.staged().is_none());
        assert!(!staged.file.exists());
        // A program replaced by hand while a version was pending: nothing to
        // watch over any more.
        store
            .set_pending(&Pending {
                from: "0.8.0".into(),
                to: "0.8.5".into(),
                old: dir.join("MapleSyrup.old.exe"),
                boots: 1,
                since: "then".into(),
            })
            .unwrap();
        fs::write(dir.join("MapleSyrup.old.exe"), b"MZ..older").unwrap();
        assert_eq!(at_start(&settings, &exe, "0.9.0"), Start::CarryOn);
        assert!(store.pending().is_none());
        assert!(!dir.join("MapleSyrup.old.exe").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_plan_for_a_manifest() {
        let dir = temp("plan");
        let settings = dir.join("settings");
        let updater = Updater::with(
            &settings,
            "0.8.0",
            true,
            "https://example.invalid/m.json",
            &[0u8; 32],
        );
        let file = a_file();
        let read =
            |version: &str| serde_json::from_slice::<Manifest>(&manifest(version, &file)).unwrap();
        assert_eq!(updater.plan(&read("0.8.0")), Plan::UpToDate);
        assert_eq!(updater.plan(&read("0.7.9")), Plan::UpToDate);
        assert_eq!(updater.plan(&read("0.9.0")), Plan::Fetch(file.clone()));
        updater.store().block("0.9.0");
        assert_eq!(updater.plan(&read("0.9.0")), Plan::Blocked("0.9.0".into()));
        assert_eq!(updater.plan(&read("0.9.1")), Plan::Fetch(file.clone()));
        fs::create_dir_all(updater.store().dir()).unwrap();
        stage(updater.store(), "0.9.1", b"MZ..new");
        assert_eq!(
            updater.plan(&read("0.9.1")),
            Plan::AlreadyStaged("0.9.1".into())
        );
        // A newer announcement replaces what was staged.
        assert_eq!(updater.plan(&read("0.9.2")), Plan::Fetch(file.clone()));
        assert!(updater.store().staged().is_none());
        // Status, as the phone sees it.
        let status = updater.status();
        assert_eq!(status.running, "0.8.0");
        assert!(status.auto);
        let json = status.to_json();
        assert_eq!(json["state"], "up-to-date");
        updater.set_auto(false);
        assert_eq!(updater.status().to_json()["state"], "off");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_now_puts_the_staged_program_in_place() {
        let dir = temp("now");
        let settings = dir.join("settings");
        let updater = Updater::with(
            &settings,
            "0.8.0",
            true,
            "https://example.invalid/m.json",
            &[0u8; 32],
        );
        assert!(
            updater.install_now(&dir.join("MapleSyrup.exe")).is_err(),
            "nothing staged"
        );
        fs::create_dir_all(updater.store().dir()).unwrap();
        let exe = dir.join("MapleSyrup.exe");
        fs::write(&exe, b"MZ..old").unwrap();
        stage(updater.store(), "0.9.0", b"MZ..new");
        assert_eq!(updater.status().to_json()["state"], "up-to-date");
        assert_eq!(updater.install_now(&exe), Ok("0.9.0".into()));
        assert_eq!(fs::read(&exe).unwrap(), b"MZ..new");
        assert_eq!(
            fs::read(dir.join("MapleSyrup.old.exe")).unwrap(),
            b"MZ..old"
        );
        assert_eq!(updater.store().pending().unwrap().to, "0.9.0");
        let _ = fs::remove_dir_all(&dir);
    }
}
