//! The workshop: MapleSyrup rewrites itself, on this PC alone.
//!
//! The player asks for a change ("make the HP warning shorter", "stop
//! calling out the quest marker"). A coding agent installed on this PC —
//! Claude Code or Codex CLI, whichever is there — makes it in the local
//! checkout of MapleSyrup's source; the program is built and its tests run;
//! and the new program is staged for the next start through the updater
//! (`update`): the one running is kept beside it, and a build that does not
//! come up twice is rolled back and never offered again. "Undo the last
//! change" reverts the last of the workshop's commits and builds again.
//!
//! What never happens here: nothing is pushed, nothing is fetched from the
//! channel while the workshop is on (a release would wipe the local work),
//! and the work is always on a branch of this PC's own (`local/<pc>`),
//! never on master. The release pipeline, the installer and the update
//! channel's key are out of bounds for the coder, and a change that touches
//! them is thrown away. This is the player's own copy, improved for the
//! player, by the player's own tools — not the product.
//!
//! Everything the workshop does is logged under `<settings>/workshop/`, one
//! folder per job: the prompt, the coder's output, the build and the tests.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::update::{Staged, Store, sha256_of};

/// The coding agents the workshop knows how to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Coder {
    /// Claude Code (`claude -p`).
    Claude,
    /// Codex CLI (`codex exec`).
    Codex,
}

impl Coder {
    pub fn label(self) -> &'static str {
        match self {
            Coder::Claude => "Claude Code",
            Coder::Codex => "Codex",
        }
    }

    /// The program's name on the PATH.
    fn program(self) -> &'static str {
        match self {
            Coder::Claude => "claude",
            Coder::Codex => "codex",
        }
    }

    pub fn parse(text: &str) -> Option<Coder> {
        match text.trim().to_lowercase().as_str() {
            "claude" | "claude code" | "claude-code" => Some(Coder::Claude),
            "codex" | "codex cli" => Some(Coder::Codex),
            _ => None,
        }
    }
}

/// How long each step may take.
const CODER_TIMEOUT: Duration = Duration::from_secs(25 * 60);
const BUILD_TIMEOUT: Duration = Duration::from_secs(45 * 60);
const TEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Paths the coder may not touch: the release pipeline, the installer, and
/// the updater's key and channel (checked by their constants' names).
const OUT_OF_BOUNDS: &[&str] = &[".github/", "installer/"];
const GUARDED_CONSTANTS: &[&str] = &["PUBLIC_KEY", "CHANNEL"];

/// The note the coder leaves about what it did.
const NOTE_FILE: &str = "WORKSHOP_NOTE.txt";

/// What the main loop hands the workshop.
#[derive(Debug, Clone, PartialEq)]
pub enum Task {
    /// Make a change: what the player asked for, and what was going on
    /// (the end of the session's log).
    Change {
        instruction: String,
        context: String,
    },
    /// Revert the last change the workshop made, and build again.
    Undo,
}

/// What the workshop has to say.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// For the player (said and shown).
    Said(String),
    /// For the log alone.
    Noted(String),
    /// A build was staged: the updater's status should say so.
    Staged,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub ok: bool,
    pub summary: String,
    /// The commit made, when one was.
    pub commit: Option<String>,
    pub when: String,
    /// Where the job's logs are.
    pub logs: PathBuf,
}

/// Where the workshop stands.
#[derive(Debug, Clone)]
pub struct State {
    pub on: bool,
    pub coder: Option<Coder>,
    /// The coders this PC has.
    pub coders: Vec<Coder>,
    pub repo: PathBuf,
    /// What it is doing now, and since when.
    pub working: Option<(String, Instant)>,
    pub last: Option<Outcome>,
    /// Jobs waiting behind the one under way.
    pub queued: usize,
}

impl State {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "on": self.on,
            "coder": self.coder.map(Coder::label),
            "coders": self.coders.iter().map(|c| c.label()).collect::<Vec<_>>(),
            "repo": self.repo.display().to_string(),
            "working": self.working.as_ref().map(|(stage, _)| stage.clone()),
            "working_secs": self.working.as_ref().map(|(_, since)| since.elapsed().as_secs()),
            "queued": self.queued,
            "last": self.last.as_ref().map(|o| serde_json::json!({
                "ok": o.ok,
                "summary": o.summary,
                "commit": o.commit,
                "when": o.when,
            })),
        })
    }
}

/// What this program is: its version and the commit it was built from.
#[derive(Debug, Clone)]
pub struct Running {
    pub version: String,
    pub commit: String,
}

/// The programs the workshop runs.
#[derive(Debug, Clone)]
pub struct Programs {
    pub git: PathBuf,
    pub cargo: PathBuf,
    /// The coders found, in the order they are preferred.
    pub coders: Vec<(Coder, PathBuf)>,
}

impl Programs {
    /// What this PC has: git and cargo (cargo also looked for where rustup
    /// puts it), and the coders on the PATH.
    pub fn find() -> Programs {
        let home = home_dir();
        let cargo = find_program("cargo").unwrap_or_else(|| {
            home.map(|h| h.join(".cargo").join("bin").join(exe_name("cargo")))
                .unwrap_or_else(|| PathBuf::from(exe_name("cargo")))
        });
        let coders = [Coder::Claude, Coder::Codex]
            .into_iter()
            .filter_map(|c| find_program(c.program()).map(|p| (c, p)))
            .collect();
        Programs {
            git: find_program("git").unwrap_or_else(|| PathBuf::from(exe_name("git"))),
            cargo,
            coders,
        }
    }
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Where `name` is on the PATH, with Windows's extensions (`.exe`, `.cmd`,
/// `.bat`: the coders install as `.cmd` shims under npm).
pub fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|e| e.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &extensions {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The default checkout: `GitHub\ms` under the home folder, or where
/// `MAPLESYRUP_REPO` says.
pub fn default_repo() -> PathBuf {
    if let Some(repo) = std::env::var_os("MAPLESYRUP_REPO") {
        return PathBuf::from(repo);
    }
    home_dir()
        .map(|h| h.join("GitHub").join("ms"))
        .unwrap_or_else(|| PathBuf::from("ms"))
}

/// This PC's name, for the branch: letters, digits and dashes.
pub fn host_name() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| {
            Command::new("hostname")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_else(|| "pc".into());
    let name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() { "pc".into() } else { name }
}

pub struct Workshop {
    repo: PathBuf,
    home: PathBuf,
    programs: Programs,
    running: Running,
    branch: String,
    /// The coder to use (the first found when None).
    preferred: Mutex<Option<Coder>>,
    /// Whether the tests run after the build (they take minutes too).
    tests: bool,
    state: Mutex<State>,
    tasks: Mutex<Option<Sender<Task>>>,
    events: Mutex<Option<Sender<Event>>>,
}

impl Workshop {
    /// A workshop for the checkout at `repo`, writing under
    /// `<settings>/workshop/`, with the programs found on this PC.
    pub fn new(settings: &Path, repo: PathBuf, running: Running, on: bool) -> Workshop {
        Workshop::with(settings, repo, running, on, Programs::find(), true)
    }

    pub fn with(
        settings: &Path,
        repo: PathBuf,
        running: Running,
        on: bool,
        programs: Programs,
        tests: bool,
    ) -> Workshop {
        let home = settings.join("workshop");
        let last = fs::read(home.join("last.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let coder = programs.coders.first().map(|(c, _)| *c);
        Workshop {
            branch: format!("local/{}", host_name()),
            state: Mutex::new(State {
                on,
                coder,
                coders: programs.coders.iter().map(|(c, _)| *c).collect(),
                repo: repo.clone(),
                working: None,
                last,
                queued: 0,
            }),
            repo,
            home,
            programs,
            running,
            preferred: Mutex::new(None),
            tests,
            tasks: Mutex::new(None),
            events: Mutex::new(None),
        }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_on(&self) -> bool {
        self.state().on
    }

    pub fn set_on(&self, on: bool) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).on = on;
    }

    /// The coders this PC has.
    pub fn coders(&self) -> Vec<Coder> {
        self.programs.coders.iter().map(|(c, _)| *c).collect()
    }

    /// Use this coder from now on (when this PC has it).
    pub fn prefer(&self, coder: Coder) -> Result<(), String> {
        if !self.coders().contains(&coder) {
            return Err(format!("{} is not installed on this PC", coder.label()));
        }
        *self.preferred.lock().unwrap_or_else(|e| e.into_inner()) = Some(coder);
        self.state.lock().unwrap_or_else(|e| e.into_inner()).coder = Some(coder);
        Ok(())
    }

    fn coder(&self) -> Option<(Coder, PathBuf)> {
        let preferred = *self.preferred.lock().unwrap_or_else(|e| e.into_inner());
        self.programs
            .coders
            .iter()
            .find(|(c, _)| preferred.is_none_or(|p| p == *c))
            .cloned()
    }

    fn say(&self, event: Event) {
        if let Some(tx) = self
            .events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = tx.send(event);
        }
    }

    fn stage_of(&self, stage: &str) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).working =
            Some((stage.to_string(), Instant::now()));
    }

    /// Hand the workshop a task. Returns what to tell the player at once.
    pub fn ask(&self, task: Task) -> Result<String, String> {
        if !self.is_on() {
            return Err("the workshop is off (Settings on the phone turns it on)".into());
        }
        if self.coder().is_none() && matches!(task, Task::Change { .. }) {
            return Err(
                "no coding agent on this PC: install Claude Code or Codex CLI and start MapleSyrup again"
                    .into(),
            );
        }
        let line = match &task {
            Task::Change { .. } => "On it. A few minutes: I'll say when it's built.",
            Task::Undo => "Undoing the last change. A few minutes.",
        };
        let tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tx) = tasks.as_ref() else {
            return Err("the workshop is not running".into());
        };
        tx.send(task)
            .map_err(|_| "the workshop stopped".to_string())?;
        self.state.lock().unwrap_or_else(|e| e.into_inner()).queued += 1;
        Ok(line.to_string())
    }

    /// Start the workshop's thread; what it does comes out on `events`.
    pub fn spawn(self: Arc<Self>, events: Sender<Event>) {
        let (tx, rx) = mpsc::channel::<Task>();
        *self.tasks.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        *self.events.lock().unwrap_or_else(|e| e.into_inner()) = Some(events);
        let _ = std::thread::Builder::new()
            .name("workshop".into())
            .spawn(move || self.serve(rx));
    }

    fn serve(&self, rx: Receiver<Task>) {
        while let Ok(task) = rx.recv() {
            {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                state.queued = state.queued.saturating_sub(1);
            }
            let outcome = self.run(task);
            if outcome.ok {
                self.say(Event::Staged);
            }
            self.say(Event::Said(outcome.summary.clone()));
        }
    }

    /// One job, start to end; what went wrong is the summary.
    pub fn run(&self, task: Task) -> Outcome {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
        let logs = self.home.join(&stamp);
        let _ = fs::create_dir_all(&logs);
        let when = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let result = match &task {
            Task::Change {
                instruction,
                context,
            } => self.change(instruction, context, &logs),
            Task::Undo => self.undo(&logs),
        };
        let outcome = match result {
            Ok((commit, note)) => Outcome {
                ok: true,
                summary: match &task {
                    Task::Change { .. } => format!(
                        "Built: {note} Restart MapleSyrup to get it (or Update now on the phone)."
                    ),
                    Task::Undo => format!(
                        "Undone: {note} Restart MapleSyrup to get it (or Update now on the phone)."
                    ),
                },
                commit: Some(commit),
                when,
                logs: logs.clone(),
            },
            Err(why) => Outcome {
                ok: false,
                summary: format!("Couldn't: {why}"),
                commit: None,
                when,
                logs: logs.clone(),
            },
        };
        let _ = fs::write(logs.join("result.txt"), &outcome.summary);
        if let Ok(text) = serde_json::to_vec_pretty(&outcome) {
            let _ = fs::write(self.home.join("last.json"), text);
        }
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.working = None;
            state.last = Some(outcome.clone());
        }
        self.say(Event::Noted(format!(
            "workshop: {} (logs in {})",
            outcome.summary,
            logs.display()
        )));
        outcome
    }

    /// Make the change: the coder, the checks, the build, the commit, the
    /// staging. Returns the commit and the coder's note.
    fn change(
        &self,
        instruction: &str,
        context: &str,
        logs: &Path,
    ) -> Result<(String, String), String> {
        let (coder, program) = self.coder().ok_or("no coding agent on this PC")?;
        self.stage_of("getting the checkout ready");
        self.prepare()?;
        self.stage_of(&format!("{} is writing the change", coder.label()));
        let prompt = self.prompt(instruction, context);
        let _ = fs::write(logs.join("prompt.txt"), &prompt);
        let coded = self.code(coder, &program, &prompt, logs);
        let built = coded.and_then(|()| {
            self.stage_of("checking what changed");
            self.check_changes()?;
            let note = self.take_note(instruction);
            self.build_and_test(logs)?;
            self.stage_of("keeping the change");
            let commit =
                self.commit(&format!("workshop: {}", first_line(instruction, 72)), &note)?;
            self.stage_of("staging the new program");
            self.stage(&commit, &note)?;
            Ok((commit, note))
        });
        if built.is_err() {
            // The tree was clean before: whatever the coder left goes.
            let _ = self.git(&["checkout", "--", "."]);
            let _ = self.git(&["clean", "-fdq"]);
        }
        built
    }

    /// Revert the workshop's last commit and build again.
    fn undo(&self, logs: &Path) -> Result<(String, String), String> {
        self.stage_of("getting the checkout ready");
        self.prepare()?;
        let last = self.git(&["log", "-1", "--format=%s"])?;
        if !last.starts_with("workshop: ") {
            return Err(format!(
                "the last change in the checkout is not the workshop's ({})",
                first_line(&last, 60)
            ));
        }
        let what = last.trim_start_matches("workshop: ").trim().to_string();
        self.stage_of("reverting the last change");
        self.git(&[
            "-c",
            "user.name=MapleSyrup workshop",
            "-c",
            "user.email=workshop@maplesyrup.local",
            "revert",
            "--no-edit",
            "HEAD",
        ])
        .map_err(|e| format!("could not revert it: {e}"))?;
        let note = format!("the change \"{what}\" was taken back.");
        let built = self.build_and_test(logs).and_then(|()| {
            let commit = self.git(&["rev-parse", "--short", "HEAD"])?;
            self.stage_of("staging the new program");
            self.stage(&commit, &note)?;
            Ok((commit, note))
        });
        if let Err(why) = &built {
            // The revert stays (it is a commit of its own); only say so.
            self.say(Event::Noted(format!(
                "workshop: reverted, but the build failed: {why}"
            )));
        }
        built
    }

    /// The checkout is there, clean, and on this PC's branch (made from
    /// the running program's commit the first time).
    fn prepare(&self) -> Result<(), String> {
        if !self.repo.join(".git").exists() {
            return Err(format!(
                "no checkout of MapleSyrup at {} (clone github.com/boggioMichael/ms there, or set MAPLESYRUP_REPO)",
                self.repo.display()
            ));
        }
        let dirty = self.git(&["status", "--porcelain"])?;
        if !dirty.trim().is_empty() {
            return Err(format!(
                "the checkout at {} has changes of its own; commit or stash them first",
                self.repo.display()
            ));
        }
        let current = self.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        if current.trim() == self.branch {
            return Ok(());
        }
        let exists = self
            .git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{}", self.branch),
            ])
            .is_ok();
        if exists {
            self.git(&["checkout", "-q", &self.branch])?;
            self.say(Event::Noted(format!("workshop: on {} again", self.branch)));
            return Ok(());
        }
        // The first time: from the commit this program was built from, so
        // the change is to the program the player is running. The commit
        // may have to be fetched; without it, from where the checkout is.
        let commit = &self.running.commit;
        let mut base = "HEAD".to_string();
        if commit != "unknown" {
            if self
                .git(&["cat-file", "-e", &format!("{commit}^{{commit}}")])
                .is_err()
            {
                let _ = self.git(&["fetch", "-q", "--all"]);
            }
            if self
                .git(&["cat-file", "-e", &format!("{commit}^{{commit}}")])
                .is_ok()
            {
                base = commit.clone();
            } else {
                self.say(Event::Noted(format!(
                    "workshop: the running program's commit {} is not in the checkout; starting from its HEAD",
                    &commit[..7.min(commit.len())]
                )));
            }
        }
        self.git(&["checkout", "-q", "-B", &self.branch, &base])?;
        self.say(Event::Noted(format!(
            "workshop: branch {} made from {}",
            self.branch,
            &base[..7.min(base.len())]
        )));
        Ok(())
    }

    /// What the coder is told.
    fn prompt(&self, instruction: &str, context: &str) -> String {
        let mut text = format!(
            "You are changing MapleSyrup — the Rust program in this repository (a MapleStory companion that \
watches the game and talks with the player) — for the one player who runs it on this PC, who asked, in \
their own words:\n\n    {instruction}\n\nRead README.md and docs/development.md first. Make the change \
small and local to what was asked, in the spirit of the code around it. Keep `cargo build --release --bin \
maplesyrup` and `cargo test --release --lib` green: run them, and fix what you broke. Do not touch \
.github/, installer/, or the update channel's key and address in src/update.rs; do not change the \
version; do not run git commands that change anything (no commit, no push, no checkout): the workshop \
commits for you. When you are done, write one sentence saying what you changed, for the player to hear, \
into {NOTE_FILE} in the repository root.\n"
        );
        if !context.trim().is_empty() {
            text.push_str("\nWhat was going on when they asked (the end of the session's log):\n");
            text.push_str(context.trim());
            text.push('\n');
        }
        text.push_str(&format!(
            "\nThe program they are running is version {} from commit {}.\n",
            self.running.version, self.running.commit
        ));
        text
    }

    /// Run the coder on the checkout.
    fn code(&self, coder: Coder, program: &Path, prompt: &str, logs: &Path) -> Result<(), String> {
        let mut command = command_for(program);
        match coder {
            Coder::Claude => {
                command.args([
                    "-p",
                    prompt,
                    "--permission-mode",
                    "acceptEdits",
                    "--allowedTools",
                ]);
                command.arg("Read,Edit,MultiEdit,Write,Glob,Grep,LS,Bash(cargo *),Bash(rustfmt *)");
            }
            Coder::Codex => {
                command.args(["exec", "--full-auto", "--skip-git-repo-check", prompt]);
            }
        }
        command.current_dir(&self.repo);
        // The player's OpenAI key, for a coder that takes one from the
        // environment (a coder signed in on its own ignores it).
        if std::env::var_os("OPENAI_API_KEY").is_none()
            && let Some(key) = crate::ai::load_key(self.home.parent().unwrap_or(&self.home))
        {
            command.env("OPENAI_API_KEY", key);
        }
        let output = run_logged(command, CODER_TIMEOUT, &logs.join("coder.log"))?;
        if !output.success {
            return Err(format!("{} failed: {}", coder.label(), output.tail(3)));
        }
        Ok(())
    }

    /// The coder changed something, and nothing out of bounds.
    fn check_changes(&self) -> Result<(), String> {
        let status = self.git(&["status", "--porcelain"])?;
        let changed: Vec<String> = status
            .lines()
            .filter_map(|l| l.get(3..).map(|p| p.trim().trim_matches('"').to_string()))
            .filter(|p| p != NOTE_FILE)
            .collect();
        if changed.is_empty() {
            return Err("the coder changed nothing".into());
        }
        for path in &changed {
            let path = path.replace('\\', "/");
            if OUT_OF_BOUNDS.iter().any(|p| path.starts_with(p)) {
                return Err(format!(
                    "the change touched {path}, which is out of bounds; thrown away"
                ));
            }
        }
        if changed
            .iter()
            .any(|p| p.replace('\\', "/") == "src/update.rs")
        {
            let diff = self.git(&["diff", "-U0", "--", "src/update.rs"])?;
            let guarded = diff.lines().any(|l| {
                (l.starts_with('+') || l.starts_with('-'))
                    && !l.starts_with("+++")
                    && !l.starts_with("---")
                    && GUARDED_CONSTANTS
                        .iter()
                        .any(|c| l.contains(&format!("const {c}")))
            });
            if guarded {
                return Err(
                    "the change touched the update channel's key or address; thrown away".into(),
                );
            }
        }
        Ok(())
    }

    /// The coder's note about what it did (and the file gone), or the
    /// instruction itself.
    fn take_note(&self, instruction: &str) -> String {
        let path = self.repo.join(NOTE_FILE);
        let note = fs::read_to_string(&path)
            .ok()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        let _ = fs::remove_file(&path);
        let mut note = note.unwrap_or_else(|| first_line(instruction, 160));
        if !note.ends_with(['.', '!', '?']) {
            note.push('.');
        }
        note
    }

    fn build_and_test(&self, logs: &Path) -> Result<(), String> {
        self.stage_of("building");
        let mut build = command_for(&self.programs.cargo);
        build
            .args(["build", "--release", "--bin", "maplesyrup"])
            .current_dir(&self.repo);
        gently(&mut build);
        let output = run_logged(build, BUILD_TIMEOUT, &logs.join("build.log"))?;
        if !output.success {
            return Err(format!("the build failed: {}", output.tail(4)));
        }
        if self.tests {
            self.stage_of("running the tests");
            let mut test = command_for(&self.programs.cargo);
            test.args(["test", "--release", "--lib"])
                .current_dir(&self.repo);
            gently(&mut test);
            let output = run_logged(test, TEST_TIMEOUT, &logs.join("test.log"))?;
            if !output.success {
                return Err(format!("the tests failed: {}", output.tail(4)));
            }
        }
        Ok(())
    }

    /// The program just built.
    fn built_program(&self) -> PathBuf {
        self.repo
            .join("target")
            .join("release")
            .join(exe_name("maplesyrup"))
    }

    fn commit(&self, subject: &str, note: &str) -> Result<String, String> {
        self.git(&["add", "-A"])?;
        self.git(&[
            "-c",
            "user.name=MapleSyrup workshop",
            "-c",
            "user.email=workshop@maplesyrup.local",
            "commit",
            "-q",
            "-m",
            subject,
            "-m",
            note,
        ])?;
        self.git(&["rev-parse", "--short", "HEAD"])
    }

    /// The built program into the updater's hands, for the next start.
    fn stage(&self, commit: &str, note: &str) -> Result<(), String> {
        let built = self.built_program();
        if !built.is_file() {
            return Err(format!("no program at {}", built.display()));
        }
        let settings = self.home.parent().unwrap_or(&self.home);
        let store = Store::new(settings);
        fs::create_dir_all(store.dir()).map_err(|e| e.to_string())?;
        let target = store
            .dir()
            .join(format!("MapleSyrup-local-{}.exe", commit.trim()));
        fs::copy(&built, &target).map_err(|e| format!("could not copy the program: {e}"))?;
        let staged = Staged {
            version: format!("{}+local.{}", self.running.version, commit.trim()),
            sha256: sha256_of(&target).map_err(|e| e.to_string())?,
            size: fs::metadata(&target).map_err(|e| e.to_string())?.len(),
            file: target,
            from: self.running.version.clone(),
            when: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            notes: note.to_string(),
            local: true,
        };
        store.set_staged(&staged)?;
        store.log(&format!("workshop staged {} ({})", staged.version, note));
        Ok(())
    }

    /// `git` in the checkout; its stdout, trimmed.
    fn git(&self, args: &[&str]) -> Result<String, String> {
        let mut command = Command::new(&self.programs.git);
        command.args(args).current_dir(&self.repo);
        let output = run_quiet(command, GIT_TIMEOUT)?;
        if !output.success {
            return Err(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or(""),
                output.tail(2)
            ));
        }
        Ok(output.stdout.trim().to_string())
    }
}

/// A command for `program`: a `.cmd`/`.bat` shim (npm's) through `cmd.exe`,
/// anything else directly.
fn command_for(program: &Path) -> Command {
    let shim = program
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if cfg!(windows) && shim {
        let mut command = Command::new("cmd.exe");
        command.arg("/c").arg(program);
        command
    } else {
        Command::new(program)
    }
}

/// A build should not take the game's frames: fewer jobs, lower priority.
fn gently(command: &mut Command) {
    let jobs = std::thread::available_parallelism()
        .map(|n| (n.get() / 2).max(1))
        .unwrap_or(1);
    command.env("CARGO_BUILD_JOBS", jobs.to_string());
    command.env("CARGO_TERM_COLOR", "never");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW
        command.creation_flags(0x0000_4000 | 0x0800_0000);
    }
}

/// What a command left: whether it succeeded, and its output.
struct Ran {
    success: bool,
    stdout: String,
    stderr: String,
}

impl Ran {
    /// The last `n` non-empty lines of what it said (stderr first).
    fn tail(&self, n: usize) -> String {
        let lines: Vec<&str> = self
            .stderr
            .lines()
            .chain(self.stdout.lines())
            .filter(|l| !l.trim().is_empty())
            .collect();
        let start = lines.len().saturating_sub(n);
        lines[start..].join(" | ").chars().take(400).collect()
    }
}

/// Run `command` with a time limit, its output kept in memory.
fn run_quiet(mut command: Command, timeout: Duration) -> Result<Ran, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not run {:?}: {e}", command.get_program()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out = std::thread::spawn(move || read_all(stdout));
    let err = std::thread::spawn(move || read_all(stderr));
    let status = wait_with_timeout(&mut child, timeout)?;
    Ok(Ran {
        success: status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// Run `command` with a time limit, what it was and what it said written
/// to `log` (and kept, for the summary).
fn run_logged(command: Command, timeout: Duration, log: &Path) -> Result<Ran, String> {
    let header = format!(
        "{} {:?}\nin {:?}\nat {}\n",
        command.get_program().to_string_lossy(),
        command
            .get_args()
            .map(|a| a.to_string_lossy().chars().take(2000).collect::<String>())
            .collect::<Vec<_>>(),
        command.get_current_dir(),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    let _ = fs::write(log, &header);
    let ran = run_quiet(command, timeout);
    if let Ok(mut file) = fs::OpenOptions::new().append(true).create(true).open(log) {
        match &ran {
            Ok(ran) => {
                let _ = writeln!(
                    file,
                    "--- stdout ---\n{}\n--- stderr ---\n{}\n--- {}",
                    ran.stdout,
                    ran.stderr,
                    if ran.success { "ok" } else { "failed" }
                );
            }
            Err(e) => {
                let _ = writeln!(file, "--- could not run: {e}");
            }
        }
    }
    ran
}

fn read_all(pipe: Option<impl std::io::Read>) -> String {
    let mut text = String::new();
    if let Some(mut pipe) = pipe {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        text = String::from_utf8_lossy(&bytes).into_owned();
    }
    text
}

/// Wait for `child`, killing it past `timeout`. Whether it succeeded.
fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Result<bool, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "it took longer than {} minutes",
                    timeout.as_secs() / 60
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The first line of `text`, at most `max` characters.
fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        let cut: String = line.chars().take(max - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Whether the player asked for a change to MapleSyrup itself, and what:
/// "change yourself: …", "rewrite yourself so that …", "תשנה את עצמך: …".
pub fn request(sentence: &str) -> Option<String> {
    let text = sentence.trim();
    let lower = text.to_lowercase();
    const OPENERS: &[&str] = &[
        "change yourself",
        "change your code",
        "rewrite yourself",
        "rewrite your code",
        "modify yourself",
        "improve yourself",
        "fix yourself",
        "update your code",
        "תשנה את עצמך",
        "תשנה את הקוד שלך",
        "תשכתב את עצמך",
        "תשפר את עצמך",
        "תתקן את עצמך",
    ];
    for opener in OPENERS {
        if let Some(at) = lower.find(opener) {
            let rest: String = text
                .chars()
                .skip(lower[..at + opener.len()].chars().count())
                .collect();
            let rest = rest
                .trim()
                .trim_start_matches([':', ',', '-', '–', '—'])
                .trim()
                .trim_start_matches("so that")
                .trim_start_matches("to ")
                .trim_start_matches("כך ש")
                .trim_start_matches("ש")
                .trim();
            if rest.chars().filter(|c| c.is_alphanumeric()).count() >= 6 {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// Whether the player asked to undo the workshop's last change.
pub fn undo_request(sentence: &str) -> bool {
    let lower = sentence.trim().to_lowercase();
    [
        "undo the last change",
        "undo your last change",
        "revert the last change",
        "revert your last change",
        "take back the last change",
        "בטל את השינוי האחרון",
        "תבטל את השינוי האחרון",
        "תחזיר את השינוי האחרון",
    ]
    .iter()
    .any(|w| lower.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_to_change_itself_is_recognised_with_what_to_change() {
        assert_eq!(
            request("Change yourself: make the HP warning shorter").as_deref(),
            Some("make the HP warning shorter")
        );
        assert_eq!(
            request("hey, rewrite yourself so that the coach stays quiet in towns").as_deref(),
            Some("the coach stays quiet in towns")
        );
        assert_eq!(
            request("תשנה את עצמך: שהאזהרה על HP תהיה קצרה יותר").as_deref(),
            Some("האזהרה על HP תהיה קצרה יותר")
        );
        assert_eq!(request("change yourself"), None, "nothing to change");
        assert_eq!(request("how much HP do I have"), None);
        assert!(undo_request("undo the last change"));
        assert!(undo_request("תבטל את השינוי האחרון"));
        assert!(!undo_request("undo"));
    }

    #[test]
    fn programs_are_found_on_the_path_and_the_host_names_the_branch() {
        // Something every PC has.
        assert!(find_program(if cfg!(windows) { "cmd" } else { "sh" }).is_some());
        assert!(find_program("no-such-program-anywhere-xyz").is_none());
        let host = host_name();
        assert!(!host.is_empty());
        assert!(
            host.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
        assert_eq!(first_line("a short line\nmore", 72), "a short line");
        assert_eq!(first_line(&"x".repeat(100), 10), "xxxxxxxxx…");
    }
}
