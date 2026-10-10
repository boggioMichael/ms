//! On Windows, gives MapleSyrup.exe its icon (the dog) and its file details
//! (name, description, version) — what Explorer, the taskbar, Task Manager
//! and the installer show — compiled with the Windows SDK's resource
//! compiler. Anywhere else, or without the SDK, it does nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/maplesyrup.ico");
    // The commit this program is built from (`env!("MS_COMMIT")`): what
    // the workshop starts its local branch from, and what the log shows.
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=.git/HEAD");
    // A new commit moves the branch HEAD names, not HEAD itself: watch the
    // branch's ref too (loose or packed), but only files that exist — a
    // missing one would run this script, and rebuild, every time.
    let head = std::fs::read_to_string(".git/HEAD").unwrap_or_default();
    if let Some(branch) = head.strip_prefix("ref: ").map(str::trim) {
        let loose = Path::new(".git").join(branch);
        if loose.is_file() {
            println!("cargo:rerun-if-changed={}", loose.display());
        }
    }
    if Path::new(".git/packed-refs").is_file() {
        println!("cargo:rerun-if-changed=.git/packed-refs");
    }
    let commit = std::env::var("GITHUB_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .filter(|s| s.len() >= 7)
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=MS_COMMIT={commit}");
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows-msvc") {
        return;
    }
    let Some(rc) = resource_compiler() else {
        println!("cargo:warning=rc.exe not found: MapleSyrup.exe gets no icon");
        return;
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = root.join("assets").join("maplesyrup.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let numbers: Vec<String> = version
        .split(['.', '-', '+'])
        .filter_map(|p| p.parse::<u32>().ok())
        .chain(std::iter::repeat(0))
        .take(4)
        .map(|n| n.to_string())
        .collect();
    let numbers = numbers.join(",");
    let icon_path = icon.display().to_string().replace('\\', "\\\\");
    let script = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon_path}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "MapleSyrup"
      VALUE "FileDescription", "MapleSyrup - the MapleStory companion"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "MapleSyrup"
      VALUE "OriginalFilename", "MapleSyrup.exe"
      VALUE "ProductName", "MapleSyrup"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let rc_file = out.join("maplesyrup.rc");
    if std::fs::write(&rc_file, script).is_err() {
        return;
    }
    let res = out.join("maplesyrup.res");
    let compiled = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg(&rc_file)
        .status()
        .is_ok_and(|s| s.success());
    if compiled {
        // The linker takes a compiled resource file as an input.
        println!("cargo:rustc-link-arg-bin=maplesyrup={}", res.display());
    } else {
        println!("cargo:warning=rc.exe failed: MapleSyrup.exe gets no icon");
    }
}

/// rc.exe: from `RC`, on the PATH (a developer prompt), or in the newest
/// Windows 10/11 SDK.
fn resource_compiler() -> Option<PathBuf> {
    if let Ok(rc) = std::env::var("RC") {
        return Some(rc.into());
    }
    if Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some("rc.exe".into());
    }
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("x64").join("rc.exe").exists())
        .collect();
    versions.sort();
    versions.pop().map(|p| p.join("x64").join("rc.exe"))
}
