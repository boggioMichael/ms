//! The workshop, start to end, on a checkout of its own with a stand-in
//! coder and a stand-in cargo: the branch of this PC made from the running
//! program's commit, the coder's change built, tested, committed and
//! staged for the updater; a change out of bounds thrown away; a change
//! that fails the tests thrown away; the last change undone.

use ms::update::Store;
use ms::workshop::{Coder, Programs, Running, Task, Workshop, find_program};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn have_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A script that runs as a program: `.cmd` on Windows, `sh` elsewhere.
fn script(dir: &Path, name: &str, windows: &str, unix: &str) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join(format!("{name}.cmd"));
        fs::write(&path, windows.replace('\n', "\r\n")).unwrap();
        path
    } else {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{unix}")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
}

/// A stand-in coder: appends `line` to src/main.rs, writes `extra` as a
/// new file when given, and leaves a note. `extra` named `BREAK.flag`
/// makes the stand-in cargo's tests fail.
fn coder(dir: &Path, name: &str, line: &str, extra: Option<&str>) -> PathBuf {
    let extra_win = extra
        .map(|p| {
            let parent = Path::new(p).parent().filter(|d| !d.as_os_str().is_empty());
            let mkdir = parent
                .map(|d| {
                    format!(
                        "if not exist \"{}\" mkdir \"{}\"\n",
                        d.display(),
                        d.display()
                    )
                })
                .unwrap_or_default();
            format!("{mkdir}echo x> \"{}\"\n", p.replace('/', "\\"))
        })
        .unwrap_or_default();
    let extra_sh = extra
        .map(|p| format!("mkdir -p \"$(dirname '{p}')\"\necho x > \"{p}\"\n"))
        .unwrap_or_default();
    script(
        dir,
        name,
        &format!(
            "@echo off\necho {line}>> src\\main.rs\n{extra_win}echo The HP warning is now shorter.> WORKSHOP_NOTE.txt\nexit /b 0\n"
        ),
        &format!(
            "echo '{line}' >> src/main.rs\n{extra_sh}echo 'The HP warning is now shorter.' > WORKSHOP_NOTE.txt\n"
        ),
    )
}

/// A stand-in cargo: `build` writes the "program" (MZ + src/main.rs);
/// `test` fails when the checkout holds BREAK.flag.
fn cargo(dir: &Path) -> PathBuf {
    let exe = if cfg!(windows) {
        "maplesyrup.exe"
    } else {
        "maplesyrup"
    };
    script(
        dir,
        "cargo",
        &format!(
            "@echo off\nif \"%1\"==\"build\" goto build\nif \"%1\"==\"test\" goto test\nexit /b 2\n:build\nif not exist target\\release mkdir target\\release\n(echo MZ& type src\\main.rs) > target\\release\\{exe}\necho built\nexit /b 0\n:test\nif exist BREAK.flag (\n  echo test failed\n  exit /b 1\n)\necho test result: ok\nexit /b 0\n"
        ),
        &format!(
            "case \"$1\" in\n  build) mkdir -p target/release; {{ printf MZ; cat src/main.rs; }} > target/release/{exe}; echo built;;\n  test) if [ -f BREAK.flag ]; then echo 'test failed' >&2; exit 1; fi; echo 'test result: ok';;\n  *) exit 2;;\nesac\n"
        ),
    )
}

struct Bench {
    dir: PathBuf,
    repo: PathBuf,
    settings: PathBuf,
    base: String,
}

fn bench(name: &str) -> Bench {
    let dir = std::env::temp_dir().join(format!("ms-workshop-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let repo = dir.join("ms");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"ms\"\nversion = \"0.9.0\"\n",
    )
    .unwrap();
    fs::write(repo.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(repo.join("README.md"), "MapleSyrup\n").unwrap();
    fs::write(repo.join("docs/development.md"), "how it works\n").unwrap();
    fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    git(&repo, &["init", "-q", "-b", "master"]);
    git(
        &repo,
        &["-c", "user.name=t", "-c", "user.email=t@t", "add", "-A"],
    );
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "base",
        ],
    );
    let base = git(&repo, &["rev-parse", "HEAD"]);
    // One more commit on master after the running program was built, so
    // the branch must start from the commit, not from HEAD.
    fs::write(repo.join("README.md"), "MapleSyrup, newer\n").unwrap();
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qam",
            "later",
        ],
    );
    Bench {
        settings: dir.join("settings"),
        dir,
        repo,
        base,
    }
}

fn workshop(b: &Bench, coder_path: PathBuf) -> Workshop {
    Workshop::with(
        &b.settings,
        b.repo.clone(),
        Running {
            version: "0.9.0".into(),
            commit: b.base.clone(),
        },
        true,
        Programs {
            git: find_program("git").unwrap(),
            cargo: cargo(&b.dir),
            coders: vec![(Coder::Claude, coder_path)],
        },
        true,
    )
}

#[test]
fn a_change_is_coded_built_tested_committed_and_staged_on_this_pcs_branch() {
    if !have_git() {
        return;
    }
    let b = bench("change");
    let good = coder(&b.dir, "coder", "// shorter warning", None);
    let shop = workshop(&b, good);
    let outcome = shop.run(Task::Change {
        instruction: "make the HP warning shorter".into(),
        context: "00:01 [reply] Pot now.".into(),
    });
    assert!(outcome.ok, "{}", outcome.summary);
    assert!(
        outcome
            .summary
            .starts_with("Built: The HP warning is now shorter."),
        "{}",
        outcome.summary
    );
    // On this PC's branch, from the running program's commit (not master's
    // newer HEAD), with the change committed and the note gone.
    let branch = git(&b.repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert!(branch.starts_with("local/"), "{branch}");
    let parent = git(&b.repo, &["rev-parse", "HEAD~1"]);
    assert_eq!(parent, b.base);
    assert_eq!(
        git(&b.repo, &["log", "-1", "--format=%s"]),
        "workshop: make the HP warning shorter"
    );
    assert!(git(&b.repo, &["log", "-1", "--format=%b"]).contains("The HP warning is now shorter."));
    assert!(
        git(&b.repo, &["status", "--porcelain"]).is_empty(),
        "clean after"
    );
    assert!(!b.repo.join("WORKSHOP_NOTE.txt").exists());
    assert!(
        fs::read_to_string(b.repo.join("src/main.rs"))
            .unwrap()
            .contains("shorter warning")
    );
    // Staged for the updater: the built program, marked local.
    let staged = Store::new(&b.settings).staged().expect("staged");
    assert!(staged.local);
    assert_eq!(
        staged.version,
        format!("0.9.0+local.{}", outcome.commit.as_deref().unwrap())
    );
    let program = fs::read(&staged.file).unwrap();
    assert!(program.starts_with(b"MZ"));
    assert!(String::from_utf8_lossy(&program).contains("shorter warning"));
    assert!(outcome.logs.join("coder.log").is_file());
    assert!(outcome.logs.join("build.log").is_file());
    assert!(outcome.logs.join("test.log").is_file());
    assert!(outcome.logs.join("prompt.txt").is_file());
    let prompt = fs::read_to_string(outcome.logs.join("prompt.txt")).unwrap();
    assert!(prompt.contains("make the HP warning shorter") && prompt.contains("Pot now."));
    // The state says so too.
    let state = shop.state();
    assert!(state.last.as_ref().is_some_and(|o| o.ok));
    assert_eq!(state.to_json()["last"]["ok"], true);

    // Undo: the change reverted, built and staged again.
    let undone = shop.run(Task::Undo);
    assert!(undone.ok, "{}", undone.summary);
    assert!(
        undone
            .summary
            .starts_with("Undone: the change \"make the HP warning shorter\" was taken back."),
        "{}",
        undone.summary
    );
    assert_eq!(
        fs::read_to_string(b.repo.join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    let staged = Store::new(&b.settings).staged().expect("staged again");
    assert_ne!(
        staged.version,
        format!("0.9.0+local.{}", outcome.commit.as_deref().unwrap())
    );
    // Nothing else to undo: the last commit is a revert, not a change.
    let again = shop.run(Task::Undo);
    assert!(!again.ok);
    assert!(
        again.summary.contains("not the workshop's"),
        "{}",
        again.summary
    );
    let _ = fs::remove_dir_all(&b.dir);
}

#[test]
fn a_change_out_of_bounds_or_failing_the_tests_is_thrown_away() {
    if !have_git() {
        return;
    }
    let b = bench("thrown");
    // Touching the release pipeline.
    let bad = coder(
        &b.dir,
        "coder-bounds",
        "// fine",
        Some(".github/workflows/x.yml"),
    );
    let shop = workshop(&b, bad);
    let outcome = shop.run(Task::Change {
        instruction: "publish me".into(),
        context: String::new(),
    });
    assert!(!outcome.ok);
    assert!(
        outcome.summary.contains("out of bounds"),
        "{}",
        outcome.summary
    );
    assert!(
        git(&b.repo, &["status", "--porcelain"]).is_empty(),
        "clean after"
    );
    assert!(!b.repo.join(".github").exists());
    assert_eq!(git(&b.repo, &["log", "-1", "--format=%s"]), "base");
    assert!(Store::new(&b.settings).staged().is_none());
    // Breaking the tests.
    let breaking = coder(&b.dir, "coder-break", "// breaks", Some("BREAK.flag"));
    let shop = workshop(&b, breaking);
    let outcome = shop.run(Task::Change {
        instruction: "break things".into(),
        context: String::new(),
    });
    assert!(!outcome.ok);
    assert!(
        outcome.summary.contains("the tests failed"),
        "{}",
        outcome.summary
    );
    assert_eq!(
        fs::read_to_string(b.repo.join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    assert!(
        !b.repo.join("BREAK.flag").exists(),
        "the coder's files are gone"
    );
    assert_eq!(git(&b.repo, &["log", "-1", "--format=%s"]), "base");
    // A checkout with changes of its own is left alone.
    fs::write(b.repo.join("src/main.rs"), "fn main() { /* mine */ }\n").unwrap();
    let good = coder(&b.dir, "coder-good", "// fine", None);
    let shop = workshop(&b, good);
    let outcome = shop.run(Task::Change {
        instruction: "anything".into(),
        context: String::new(),
    });
    assert!(!outcome.ok);
    assert!(
        outcome.summary.contains("changes of its own"),
        "{}",
        outcome.summary
    );
    assert_eq!(
        fs::read_to_string(b.repo.join("src/main.rs")).unwrap(),
        "fn main() { /* mine */ }\n"
    );
    let _ = fs::remove_dir_all(&b.dir);
}

#[test]
fn the_workshop_refuses_what_it_cannot_do_and_says_why() {
    let b = bench("refuses");
    let shop = Workshop::with(
        &b.settings,
        b.repo.clone(),
        Running {
            version: "0.9.0".into(),
            commit: b.base.clone(),
        },
        false,
        Programs {
            git: PathBuf::from("git"),
            cargo: PathBuf::from("cargo"),
            coders: Vec::new(),
        },
        false,
    );
    assert!(shop.ask(Task::Undo).unwrap_err().contains("off"));
    shop.set_on(true);
    assert!(
        shop.ask(Task::Change {
            instruction: "x".into(),
            context: String::new()
        })
        .unwrap_err()
        .contains("no coding agent")
    );
    assert!(
        shop.prefer(Coder::Codex)
            .unwrap_err()
            .contains("not installed")
    );
    assert_eq!(shop.coders(), Vec::<Coder>::new());
    // No checkout at all.
    let shop = Workshop::with(
        &b.settings,
        b.dir.join("nowhere"),
        Running {
            version: "0.9.0".into(),
            commit: "unknown".into(),
        },
        true,
        Programs {
            git: find_program("git").unwrap_or_else(|| PathBuf::from("git")),
            cargo: PathBuf::from("cargo"),
            coders: vec![(Coder::Codex, PathBuf::from("codex"))],
        },
        false,
    );
    let outcome = shop.run(Task::Change {
        instruction: "x".into(),
        context: String::new(),
    });
    assert!(!outcome.ok);
    assert!(
        outcome.summary.contains("no checkout"),
        "{}",
        outcome.summary
    );
    let _ = fs::remove_dir_all(&b.dir);
}
